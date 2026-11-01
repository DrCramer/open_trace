//! The `opentrace://` scheme: every request of the web UI is answered in-process.
//!
//! * `/`, `/idx/*`, `/api/*` → `opentrace_core::server::App::handle` — the same code the HTTP server runs, called
//!   directly (no socket, no localhost port, nothing reachable from other programs). FSA bytes arrive as the binary
//!   request body exactly as the web UI already sends them.
//! * `/desktop/*` → desktop pages and the desktop API (server connection, FSA sync queue, window lifecycle,
//!   self-test). These are only reachable from this application's own webviews: a custom scheme is not exposed to
//!   other processes, and navigation away from it is blocked (see `windows.rs`).
use crate::state::DesktopState;
use serde_json::{json, Value};
use tauri::http::{header, Request, Response, StatusCode};
use tauri::{AppHandle, Manager};

pub const SHIM_JS: &str = include_str!("../../ui/shim.js");
const CONNECTION_HTML: &str = include_str!("../../ui/connection.html");
const CONNECTION_JS: &str = include_str!("../../ui/connection.js");

/// CSP for HTML pages: own scheme only (no remote scripts, frames or form targets); inline code is what the
/// existing pages use.
const CSP: &str = "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; \
font-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'self'";

pub fn response(code: u16, kind: &str, body: Vec<u8>) -> Response<Vec<u8>> {
    let mut b = Response::builder()
        .status(StatusCode::from_u16(code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR))
        .header(header::CONTENT_TYPE, kind)
        .header(header::CACHE_CONTROL, "no-store")
        .header("X-Content-Type-Options", "nosniff");
    if kind.starts_with("text/html") {
        b = b.header("Content-Security-Policy", CSP);
    }
    b.body(body).unwrap_or_else(|_| Response::new(Vec::new()))
}

pub fn json_response(code: u16, v: &Value) -> Response<Vec<u8>> {
    response(code, "application/json; charset=utf-8", serde_json::to_vec(v).unwrap_or_default())
}

/// Translates a webview request into the core's request type. The page's own Host/Origin (`opentrace://localhost` or
/// `http://opentrace.localhost`) are replaced by the loopback session host the core expects; the session token check
/// (`X-Session-Token`) stays in force.
pub fn to_core(req: &Request<Vec<u8>>, port: u16) -> opentrace_core::server::Request {
    let target = match req.uri().query() {
        Some(q) => format!("{}?{}", req.uri().path(), q),
        None => req.uri().path().to_string(),
    };
    let mut headers: Vec<(String, String)> = vec![("host".into(), format!("127.0.0.1:{}", port))];
    for (k, v) in req.headers() {
        let k = k.as_str().to_ascii_lowercase();
        if k == "host" || k == "origin" || k == "referer" {
            continue;
        }
        if let Ok(v) = v.to_str() {
            headers.push((k, v.to_string()));
        }
    }
    opentrace_core::server::Request { method: req.method().as_str().to_string(), target, headers, body: req.body().clone() }
}

/// `X-OT-Body-Length` (set by the shim for binary bodies) must equal what arrived; protects against short reads
/// of request bodies in the platform webview (seen in WebKitGTK custom-scheme handling).
pub fn body_length_ok(req: &Request<Vec<u8>>) -> Result<(), String> {
    match req.headers().get("x-ot-body-length").and_then(|v| v.to_str().ok()).map(|v| v.trim().parse::<usize>()) {
        None => Ok(()),
        Some(Ok(n)) if n == req.body().len() => Ok(()),
        Some(Ok(n)) => Err(format!("Тело запроса передано не полностью: {} из {} байт. Повторите операцию.", req.body().len(), n)),
        Some(Err(_)) => Err("Bad X-OT-Body-Length".into()),
    }
}

pub fn handle(app: &AppHandle, st: &DesktopState, label: &str, req: Request<Vec<u8>>) -> Response<Vec<u8>> {
    if let Err(e) = body_length_ok(&req) {
        return json_response(400, &json!({"error": e}));
    }
    let path = req.uri().path().to_string();
    if let Some(rest) = path.strip_prefix("/desktop/") {
        return desktop(app, st, label, rest, &req);
    }
    let limit = if path == "/api/analyze" || path == "/api/sample-info" { opentrace_core::abif::MAX_FILE_BYTES } else { opentrace_core::server::MAX_JSON_BYTES };
    if req.body().len() > limit {
        return json_response(413, &json!({"error": format!("Request too large: {} bytes (limit {} MiB)", req.body().len(), limit / (1024 * 1024))}));
    }
    let r = st.core.handle(&to_core(&req, st.core.bound_port()));
    response(r.code, &r.kind, r.body)
}

fn body_json(req: &Request<Vec<u8>>) -> Value {
    serde_json::from_slice(req.body()).unwrap_or(Value::Null)
}

fn query_param(req: &Request<Vec<u8>>, key: &str) -> Option<String> {
    let q = req.uri().query()?;
    url::form_urlencoded::parse(q.as_bytes()).find(|(k, _)| k == key).map(|(_, v)| v.into_owned())
}

fn remote_err(e: crate::remote::RemoteError) -> Response<Vec<u8>> {
    let v = e.to_json();
    let code = match v["status"].as_u64() {
        Some(0) | None => 502,
        Some(c) => c as u16,
    };
    json_response(code, &v)
}

fn desktop(app: &AppHandle, st: &DesktopState, label: &str, rest: &str, req: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let post = req.method() == tauri::http::Method::POST;
    match (rest, post) {
        ("connection.html", false) => response(200, "text/html; charset=utf-8", CONNECTION_HTML.as_bytes().to_vec()),
        ("connection.js", false) => response(200, "text/javascript; charset=utf-8", CONNECTION_JS.as_bytes().to_vec()),
        ("api/info", false) => {
            let cfg = st.config();
            let s = st.remote.session();
            json_response(200, &json!({
                "app_version": env!("CARGO_PKG_VERSION"),
                "analysis_version": opentrace_core::server_analyze::analysis_version(),
                "math_backend": opentrace_core::cmath::BACKEND,
                "platform": std::env::consts::OS,
                "workers": st.pool.size,
                "data_dir": st.data_dir.display().to_string(),
                "server": {"url": cfg.server_url, "allow_insecure_http": cfg.allow_insecure_http, "username": cfg.username,
                           "ca_cert_pem": cfg.ca_cert_pem, "sync_fsa": cfg.sync_fsa,
                           "logged_in": s.is_some(), "user": s.as_ref().map(|s| s.user.clone()), "roles": s.as_ref().map(|s| s.roles.clone())},
                "research_only": true,
            }))
        }
        ("api/server/configure", true) => {
            let b = body_json(req);
            let allow = b["allow_insecure_http"].as_bool().unwrap_or(false);
            let url = match crate::config::normalize_server_url(b["url"].as_str().unwrap_or(""), allow) {
                Ok(u) => u,
                Err(e) => return json_response(400, &json!({"error": e})),
            };
            let ca = b["ca_cert_pem"].as_str().filter(|s| !s.trim().is_empty()).map(std::path::PathBuf::from);
            if let Some(p) = &ca {
                if !p.is_file() {
                    return json_response(400, &json!({"error": format!("Файл сертификата не найден: {}", p.display())}));
                }
            }
            let changed = st.config().server_url.as_deref() != Some(url.as_str());
            if changed {
                st.remote.logout(st.config().ca_cert_pem.as_deref());
            }
            st.update_config(|c| {
                c.server_url = Some(url.clone());
                c.allow_insecure_http = allow;
                c.ca_cert_pem = ca.clone();
                if let Some(s) = b["sync_fsa"].as_bool() {
                    c.sync_fsa = s;
                }
            });
            st.remote.reset_agent();
            *st.sync.enabled.lock().unwrap_or_else(|e| e.into_inner()) = st.config().sync_fsa;
            json_response(200, &json!({"ok": true, "url": url, "insecure": url.starts_with("http://")}))
        }
        ("api/server/check", true) => {
            let cfg = st.config();
            let Some(url) = cfg.server_url.clone() else { return json_response(400, &json!({"error": "Адрес сервера не задан"})) };
            match st.remote.check(&url, cfg.ca_cert_pem.as_deref()) {
                Ok(v) => json_response(200, &v),
                Err(e) => remote_err(e),
            }
        }
        ("api/server/login", true) => {
            let b = body_json(req);
            let cfg = st.config();
            let Some(url) = cfg.server_url.clone() else { return json_response(400, &json!({"error": "Адрес сервера не задан"})) };
            let user = b["username"].as_str().unwrap_or("").trim().to_string();
            let pw = b["password"].as_str().unwrap_or("");
            match st.remote.login(&url, cfg.ca_cert_pem.as_deref(), &user, pw) {
                Ok(s) => {
                    st.update_config(|c| c.username = Some(user.clone()));
                    st.sync.resume();
                    json_response(200, &json!({"ok": true, "user": s.user, "roles": s.roles, "expires_ms": s.expires_ms}))
                }
                Err(e) => remote_err(e),
            }
        }
        ("api/server/logout", true) => {
            st.remote.logout(st.config().ca_cert_pem.as_deref());
            st.sync.pause(Some("Нет входа на сервер".into()));
            json_response(200, &json!({"ok": true}))
        }
        ("api/server/projects", false) => match st.remote.get_json(st.config().ca_cert_pem.as_deref(), "/api/v1/projects") {
            Ok(v) => json_response(200, &v),
            Err(e) => remote_err(e),
        },
        ("api/server/lab-profiles", false) => match st.remote.get_json(st.config().ca_cert_pem.as_deref(), "/api/v1/lab-profiles") {
            Ok(v) => json_response(200, &v),
            Err(e) => remote_err(e),
        },
        ("api/fsa/sync", true) => {
            if !st.config().sync_fsa {
                return json_response(200, &json!({"queued": false, "reason": "sync disabled"}));
            }
            let name = query_param(req, "name").unwrap_or_else(|| "sample.fsa".into());
            match st.sync.add(&name, req.body()) {
                Ok(sha) => json_response(200, &json!({"queued": true, "sha256": sha})),
                Err(e) => json_response(400, &json!({"error": e})),
            }
        }
        ("api/sync/status", false) => {
            let mut s = st.sync.status();
            s["server"] = json!(st.config().server_url);
            s["logged_in"] = json!(st.remote.session().is_some());
            json_response(200, &s)
        }
        ("api/sync/retry", true) => {
            st.sync.resume();
            json_response(200, &json!({"ok": true}))
        }
        ("api/window/close-confirmed", true) => {
            if let Some(w) = app.get_webview_window(label) {
                st.allow_close(label);
                let _ = w.close();
            }
            json_response(200, &json!({"ok": true}))
        }
        ("api/selftest/fsa", false) if st.selftest.is_some() => response(200, "application/octet-stream", opentrace_core::demo::demo_bytes()),
        ("api/selftest/report", true) if st.selftest.is_some() => {
            let v = body_json(req);
            if let Some(t) = &st.selftest {
                t.record(app, label, v);
            }
            json_response(200, &json!({"ok": true}))
        }
        _ => json_response(404, &json!({"error": "Not found"})),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_request_gets_loopback_host_and_no_origin() {
        let req = Request::builder()
            .method("POST")
            .uri("http://opentrace.localhost/api/analyze?filename=a.fsa")
            .header("Host", "opentrace.localhost")
            .header("Origin", "http://opentrace.localhost")
            .header("X-Session-Token", "tok")
            .body(b"ABIF".to_vec())
            .unwrap();
        let c = to_core(&req, 0);
        assert_eq!(c.target, "/api/analyze?filename=a.fsa");
        assert_eq!(c.header("host"), Some("127.0.0.1:0"));
        assert_eq!(c.header("origin"), None);
        assert_eq!(c.header("x-session-token"), Some("tok"));
        assert_eq!(c.body, b"ABIF");
    }

    #[test]
    fn short_bodies_are_rejected() {
        let mk = |len: &str, body: &[u8]| Request::builder().uri("opentrace://localhost/api/analyze").header("X-OT-Body-Length", len).body(body.to_vec()).unwrap();
        assert!(body_length_ok(&mk("4", b"ABIF")).is_ok());
        assert!(body_length_ok(&mk("5", b"ABIF")).is_err());
        assert!(body_length_ok(&Request::builder().uri("/").body(vec![]).unwrap()).is_ok());
    }

    #[test]
    fn html_gets_csp_and_no_store() {
        let r = response(200, "text/html; charset=utf-8", b"<p>".to_vec());
        assert!(r.headers().get("Content-Security-Policy").unwrap().to_str().unwrap().contains("object-src 'none'"));
        assert_eq!(r.headers().get(header::CACHE_CONTROL).unwrap(), "no-store");
        assert!(response(200, "application/json", vec![]).headers().get("Content-Security-Policy").is_none());
    }
}
