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
const STANDARDS_HTML: &str = include_str!("../../ui/standards.html");
const STANDARDS_JS: &str = include_str!("../../ui/standards.js");
const PROVENANCE_HTML: &str = include_str!("../../ui/provenance.html");
const PROVENANCE_JS: &str = include_str!("../../ui/provenance.js");

/// Canonical JSON (object keys sorted recursively, compact) — must equal OpenTrace Server's `standards::canonical`,
/// whose SHA-256 identifies a standard version (checked against the server crate in tests/workspace_server.rs).
pub fn canonical(v: &Value) -> String {
    fn sort(v: &Value) -> Value {
        match v {
            Value::Object(m) => {
                let mut keys: Vec<&String> = m.keys().collect();
                keys.sort();
                let mut out = serde_json::Map::new();
                for k in keys {
                    out.insert(k.clone(), sort(&m[k]));
                }
                Value::Object(out)
            }
            Value::Array(a) => Value::Array(a.iter().map(sort).collect()),
            other => other.clone(),
        }
    }
    serde_json::to_string(&sort(v)).unwrap_or_default()
}

/// API v1 paths the desktop pages may call through Rust (the token never reaches JavaScript).
fn proxy_allowed(path: &str, method: &str) -> bool {
    let p = path.trim_start_matches('/');
    (p == "me" && method == "GET")
        || p == "standards"
        || p.starts_with("standards/")
        || p == "lab-profiles"
        || (method == "GET" && p.starts_with("projects/") && p.ends_with("/analyses") && p.matches('/').count() == 2)
}

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
    if label != "main" {
        st.scheme_seen.lock().unwrap_or_else(|e| e.into_inner()).insert(label.to_string());
    }
    if let Err(e) = body_length_ok(&req) {
        return json_response(400, &json!({"error": e}));
    }
    let path = req.uri().path().to_string();
    if let Some(rest) = path.strip_prefix("/desktop/") {
        return desktop(app, st, label, rest, &req);
    }
    // A local draft of a server project (see drafts.rs): only writes to a draft name registered by `drafts/begin`.
    // The header carries the name percent-encoded (header values are Latin-1).
    if let Some(draft) = req.headers().get("x-ot-draft").and_then(|v| v.to_str().ok()).map(|v| {
        url::form_urlencoded::parse(format!("d={}", v.replace('+', "%2B")).as_bytes()).next().map(|(_, d)| d.into_owned()).unwrap_or_default()
    }) {
        let body: Value = serde_json::from_slice(req.body()).unwrap_or(Value::Null);
        let action = body["action"].as_str().unwrap_or("");
        if path != "/api/idx-project" || !matches!(action, "save_meta" | "save_sample") || body["name"].as_str() != Some(draft.as_str()) || !st.drafts.is_open(&draft) {
            return json_response(403, &json!({"error": "draft write refused"}));
        }
        let mut creq = to_core(&req, st.core.bound_port());
        // A sample opened from the server has no FSA in the window: take the original from the local FSA cache, or
        // from the server when it is reachable, so the draft is complete (verified by SHA-256 either way).
        if action == "save_sample" && body["fsa_b64"].is_null() {
            if let Some(sha) = body["data"]["analysis"]["source_sha256"].as_str() {
                let ca = st.config().ca_cert_pem.map(std::path::PathBuf::from);
                let bytes = st.sync.cached(sha).or_else(|| st.remote.fsa_get(ca.as_deref(), sha).ok());
                if let Some(b) = bytes {
                    let mut nb = body.clone();
                    nb["fsa_b64"] = json!(opentrace_core::matrix::b64encode(&b));
                    creq.body = serde_json::to_vec(&nb).unwrap_or_default();
                    creq.headers.retain(|(k, _)| k != "content-length" && k != "x-ot-body-length");
                    creq.headers.push(("content-length".into(), creq.body.len().to_string()));
                    let r = st.core.handle(&creq);
                    let mut out: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
                    if r.code == 200 && out.is_object() {
                        out["draft_fsa"] = json!(true);
                    }
                    return json_response(r.code, &out);
                }
            }
        }
        let r = st.core.handle(&creq);
        return response(r.code, &r.kind, r.body);
    }
    if path == "/api/idx-project" && req.method() == tauri::http::Method::POST && st.server_projects() {
        let body: Value = match serde_json::from_slice(req.body()) {
            Ok(v) => v,
            Err(e) => return json_response(400, &json!({"error": e.to_string()})),
        };
        return match st.workspace.handle(&body) {
            Ok(v) => json_response(200, &v),
            Err(e) => json_response(e.status, &e.body),
        };
    }
    let limit = if path == "/api/analyze" || path == "/api/sample-info" { opentrace_core::abif::MAX_FILE_BYTES } else { opentrace_core::server::MAX_JSON_BYTES };
    if req.body().len() > limit {
        return json_response(413, &json!({"error": format!("Request too large: {} bytes (limit {} MiB)", req.body().len(), limit / (1024 * 1024))}));
    }
    let r = st.core.handle(&to_core(&req, st.core.bound_port()));
    if r.code == 200 && st.remote.session().is_some() {
        if path == "/api/analyze" {
            st.workspace.ledger.record(req.uri().query(), req.body(), &r.body);
        } else if path == "/api/import-gm-kit" || path == "/api/str-run" {
            note_genotyping(st, &path, req.body(), &r.body);
        }
    }
    let body = if r.code == 200 && r.kind.starts_with("text/html") { inject_scripts(r.body, st.selftest.is_some()) } else { r.body };
    response(r.code, &r.kind, body)
}

/// With an active laboratory profile: which kits the UI builds (checked against the profile) and which kit each
/// sample is genotyped with (see `provenance.rs`).
fn note_genotyping(st: &DesktopState, path: &str, req: &[u8], resp: &[u8]) {
    let profile = st.workspace.profile.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let Ok(Some(active)) = profile else { return };
    let Ok(body) = serde_json::from_slice::<Value>(req) else { return };
    if path == "/api/import-gm-kit" {
        if body["panel"].is_null() {
            return;
        }
        let Ok(out) = serde_json::from_slice::<Value>(resp) else { return };
        let verdict = crate::labprofile::check_kit_build(&active, &body);
        st.workspace.ledger.note_kit_build(crate::labprofile::norm_hash(&out["kit"]), verdict);
    } else if let Some(fsa) = body["sample"]["source_sha256"].as_str() {
        st.workspace.ledger.note_genotyping(fsa, crate::labprofile::norm_hash(&body["kit"]));
    }
}

/// Desktop scripts go into every HTML page as the first element of `<head>`, so they run before the page's own
/// scripts in every window — including windows handed to WebView2 through `SetNewWindow`, where
/// `initialization_script` was observed not to run (Windows 11, WebView2 142+).
pub fn inject_scripts(html: Vec<u8>, selftest: bool) -> Vec<u8> {
    let mut tags = String::from("<script src=\"/desktop/shim.js\"></script>");
    if selftest {
        tags.push_str("<script>window.__OT_SELFTEST__=true;");
        if std::env::var_os("OPENTRACE_SELFTEST_SERVER_URL").is_some() {
            tags.push_str("window.__OT_SELFTEST_SERVER__=true;");
        }
        if let Some((name, version)) = crate::selftest::profile() {
            tags.push_str(&format!("window.__OT_SELFTEST_PROFILE__={};", json!({"name": name, "version": version})));
        }
        if let Some(n) = crate::selftest::genotype_size() {
            tags.push_str(&format!("window.__OT_SELFTEST_GENOTYPE__={};", n));
        }
        if let Some(n) = crate::selftest::batch_size() {
            tags.push_str(&format!("window.__OT_SELFTEST_BATCH__={};", n));
        }
        tags.push_str("</script><script src=\"/desktop/selftest.js\"></script>");
    }
    let lower: Vec<u8> = html.iter().take(4096).map(|b| b.to_ascii_lowercase()).collect();
    let at = match lower.windows(6).position(|w| w == b"<head>") {
        Some(i) => i + 6,
        None => return html,
    };
    let mut out = Vec::with_capacity(html.len() + tags.len());
    out.extend_from_slice(&html[..at]);
    out.extend_from_slice(tags.as_bytes());
    out.extend_from_slice(&html[at..]);
    out
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
        ("connection.html", false) => response(200, "text/html; charset=utf-8", inject_scripts(CONNECTION_HTML.as_bytes().to_vec(), false)),
        ("connection.js", false) => response(200, "text/javascript; charset=utf-8", CONNECTION_JS.as_bytes().to_vec()),
        ("shim.js", false) => response(200, "text/javascript; charset=utf-8", SHIM_JS.as_bytes().to_vec()),
        ("selftest.js", false) if st.selftest.is_some() => response(200, "text/javascript; charset=utf-8", crate::selftest::SELFTEST_JS.as_bytes().to_vec()),
        ("api/drafts/begin", true) => {
            let b = body_json(req);
            let project = b["project"].as_str().unwrap_or("").to_string();
            // Existing local project names (core list with the page's own session/ticket headers).
            let mut list_req = to_core(req, st.core.bound_port());
            list_req.target = "/api/idx-project".into();
            list_req.body = serde_json::to_vec(&json!({"action": "list"})).unwrap_or_default();
            list_req.headers.retain(|(k, _)| k != "content-length" && k != "x-ot-body-length" && k != "content-type");
            list_req.headers.push(("content-type".into(), "application/json".into()));
            list_req.headers.push(("content-length".into(), list_req.body.len().to_string()));
            let r = st.core.handle(&list_req);
            if r.code != 200 {
                return response(r.code, &r.kind, r.body);
            }
            let names: Vec<String> = serde_json::from_slice::<Value>(&r.body)
                .ok()
                .and_then(|v| v["projects"].as_array().map(|a| a.iter().filter_map(|p| p["name"].as_str().map(String::from)).collect()))
                .unwrap_or_default();
            let draft = st.drafts.begin(&project, |n| names.iter().any(|x| x.eq_ignore_ascii_case(n)));
            eprintln!("[desktop] saving a local draft of server project ({} chars name)", project.chars().count());
            json_response(200, &json!({"draft": draft}))
        }
        ("api/drafts/finish", true) => {
            let b = body_json(req);
            let draft = b["draft"].as_str().unwrap_or("");
            let e = crate::drafts::entry(
                draft,
                b["project"].as_str().unwrap_or(""),
                st.config().server_url,
                b["reason"].as_str().unwrap_or(""),
                b["samples"].as_u64().unwrap_or(0),
                b["samples_with_fsa"].as_u64().unwrap_or(0),
                b["samples_failed"].as_u64().unwrap_or(0),
            );
            match st.drafts.finish(draft, e) {
                Ok(()) => json_response(200, &json!({"ok": true})),
                Err(x) => json_response(400, &json!({"error": x})),
            }
        }
        ("api/drafts", false) => json_response(200, &json!({"drafts": st.drafts.list()})),
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
                "project_source": cfg.project_source,
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
                    st.reload_lab_profile();
                    json_response(200, &json!({"ok": true, "user": s.user, "roles": s.roles, "expires_ms": s.expires_ms}))
                }
                Err(e) => remote_err(e),
            }
        }
        ("api/server/logout", true) => {
            st.workspace.release_all();
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
        ("api/workspace/status", false) => {
            let mut v = st.workspace.status();
            v["source"] = json!(st.config().project_source);
            v["logged_in"] = json!(st.remote.session().is_some());
            v["server"] = json!(st.config().server_url);
            json_response(200, &v)
        }
        ("api/lab-profile", false) => json_response(200, &json!({"active": st.workspace.profile_status(), "configured": st.config().lab_profile})),
        ("api/lab-profile", true) => {
            // {name, version} activates (after verification), {} clears.
            let b = body_json(req);
            let pick = match (b["name"].as_str(), b["version"].as_i64()) {
                (Some(n), Some(v)) => Some((n.to_string(), v)),
                _ => None,
            };
            if pick.is_some() && st.remote.session().is_none() {
                return json_response(409, &json!({"error": "Сначала войдите на сервер"}));
            }
            let previous = st.config().lab_profile;
            st.update_config(|c| c.lab_profile = pick.clone());
            let status = st.reload_lab_profile();
            if status["verified"] == false {
                // Not usable: keep the previous choice, report why.
                st.update_config(|c| c.lab_profile = previous);
                st.reload_lab_profile();
                return json_response(409, &json!({"error": status["error"]}));
            }
            json_response(200, &json!({"active": status}))
        }
        ("api/workspace/source", true) => {
            let src = body_json(req)["source"].as_str().unwrap_or("").to_string();
            if src != "local" && src != "server" {
                return json_response(400, &json!({"error": "source: local или server"}));
            }
            if src == "server" && st.remote.session().is_none() {
                return json_response(409, &json!({"error": "Сначала войдите на сервер"}));
            }
            if src == "local" {
                st.workspace.release_all();
            }
            st.update_config(|c| c.project_source = src.clone());
            json_response(200, &json!({"ok": true, "source": src}))
        }
        ("standards.html", false) => response(200, "text/html; charset=utf-8", inject_scripts(STANDARDS_HTML.as_bytes().to_vec(), false)),
        ("standards.js", false) => response(200, "text/javascript; charset=utf-8", STANDARDS_JS.as_bytes().to_vec()),
        ("provenance.html", false) => response(200, "text/html; charset=utf-8", inject_scripts(PROVENANCE_HTML.as_bytes().to_vec(), false)),
        ("provenance.js", false) => response(200, "text/javascript; charset=utf-8", PROVENANCE_JS.as_bytes().to_vec()),
        ("api/standards/hash", true) => {
            use sha2::Digest;
            let items = body_json(req)["items"].as_array().cloned().unwrap_or_default();
            let out: Vec<Value> = items.iter().map(|i| json!(crate::sync::hex(&sha2::Sha256::digest(canonical(&i["content"]).as_bytes())))).collect();
            json_response(200, &json!({"sha256": out}))
        }
        (p, _) if p.starts_with("api/v1/") => {
            let method = req.method().as_str().to_string();
            let sub = &p["api/v1".len()..];
            if !proxy_allowed(sub, &method) || !matches!(method.as_str(), "GET" | "POST" | "PUT") {
                return json_response(403, &json!({"error": "not available from the desktop pages"}));
            }
            let path = match req.uri().query() {
                Some(q) => format!("/api/v1{}?{}", sub, q),
                None => format!("/api/v1{}", sub),
            };
            let body = if method == "GET" { None } else { Some(body_json(req)) };
            match st.remote.call(st.config().ca_cert_pem.as_deref(), &method, &path, body.as_ref()) {
                Ok(v) => json_response(200, &v),
                Err(e) => remote_err(e),
            }
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
        ("api/selftest/fsa", false) if st.selftest.is_some() => {
            // `?seed=N`: a 10 000-scan run file for the batch measurement; otherwise the small demo file.
            let body = if query_param(req, "ladder").is_some() {
                opentrace_core::demo::synthetic_ladder_fsa()
            } else if let Some(seed) = query_param(req, "genotype").and_then(|v| v.parse::<u64>().ok()) {
                opentrace_core::demo::synthetic_genotype_fsa(seed).0
            } else {
                match query_param(req, "seed").and_then(|v| v.parse::<u64>().ok()) {
                    Some(seed) => opentrace_core::demo::synthetic_run_fsa(seed),
                    None => opentrace_core::demo::demo_bytes(),
                }
            };
            response(200, "application/octet-stream", body)
        }
        ("api/selftest/kit", false) if st.selftest.is_some() => {
            let (kit, panel, panels, bins, stutter) = opentrace_core::demo::synthetic_kit_texts();
            json_response(200, &json!({"kit": kit, "panel": panel, "panels": panels, "bins": bins, "stutter": stutter}))
        }
        ("api/selftest/expected", false) if st.selftest.is_some() => {
            let seed = query_param(req, "seed").and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
            let exp: serde_json::Map<String, Value> = opentrace_core::demo::synthetic_genotype_fsa(seed).1.into_iter().map(|(m, l)| (m, json!(l))).collect();
            json_response(200, &Value::Object(exp))
        }
        // Native windows the app still has (a popup closed by script must not survive as an empty native window).
        ("api/selftest/windows", false) if st.selftest.is_some() => {
            let mut labels: Vec<String> = app.webview_windows().keys().cloned().collect();
            labels.sort();
            json_response(200, &json!({"windows": labels}))
        }
        ("api/selftest/drop-lease", true) if st.selftest.is_some() => {
            let name = body_json(req)["project"].as_str().unwrap_or("").to_string();
            json_response(200, &json!({"dropped": st.workspace.drop_lease_on_server(&name)}))
        }
        ("api/selftest/memory", false) if st.selftest.is_some() => json_response(200, &json!({"process_tree_rss_mib": crate::selftest::process_tree_rss_mib()})),
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
    fn scripts_are_injected_first_in_head() {
        let out = String::from_utf8(inject_scripts(b"<!doctype html>\n<html><HEAD><meta charset=utf-8><script src=/idx/ui.js></script>".to_vec(), false)).unwrap();
        assert!(out.contains("<HEAD><script src=\"/desktop/shim.js\"></script><meta"));
        assert_eq!(inject_scripts(b"no head here".to_vec(), true), b"no head here");
    }

    #[test]
    fn proxy_is_limited_to_standards() {
        assert!(proxy_allowed("/standards", "GET"));
        assert!(proxy_allowed("/standards/kit/K/1/approve", "POST"));
        assert!(proxy_allowed("/lab-profiles", "GET"));
        assert!(proxy_allowed("/me", "GET"));
        assert!(!proxy_allowed("/projects/P/idx", "POST"));
        assert!(proxy_allowed("/projects/Case%201/analyses", "GET"));
        assert!(!proxy_allowed("/projects/P/analyses", "POST"));
        assert!(!proxy_allowed("/projects/P/x/analyses", "GET"));
        assert!(!proxy_allowed("/fsa/abc", "GET"));
        assert!(!proxy_allowed("/auth/logout", "POST"));
        assert!(!proxy_allowed("/audit", "GET"));
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
