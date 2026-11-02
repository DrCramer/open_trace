//! RemoteOrganizationService: the desktop's only path to the OpenTrace Server (API v1). Runs in Rust, so the bearer
//! token never reaches page JavaScript; TLS uses the OS trust store (Windows certificate store, OpenSSL on Linux) plus
//! an optional laboratory CA. There is no automatic downgrade to HTTP.
use serde_json::{json, Value};
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const KEYRING_SERVICE: &str = "org.opentrace.desktop";

#[derive(Clone, Debug, Default)]
pub struct Session {
    pub server: String,
    pub user: String,
    pub token: String,
    pub roles: Vec<String>,
    pub expires_ms: i64,
}

pub struct Remote {
    agent: Mutex<Option<(String, ureq::Agent)>>,
    pub session: Mutex<Option<Session>>,
    /// Remember session tokens in the OS credential store (off for self-tests, which must leave no trace).
    persist: bool,
}

#[derive(Debug)]
pub enum RemoteError {
    /// Server reachable, request refused (status, server JSON).
    Status(u16, Value),
    /// Network/TLS problem.
    Transport(String),
    NotLoggedIn,
}

impl RemoteError {
    pub fn to_json(&self) -> Value {
        match self {
            RemoteError::Status(code, v) => json!({"error": v.get("error").cloned().unwrap_or(json!(format!("HTTP {}", code))), "status": code, "details": v}),
            RemoteError::Transport(m) => json!({"error": format!("Сервер недоступен: {}", m), "status": 0, "offline": true}),
            RemoteError::NotLoggedIn => json!({"error": "Нет входа на сервер", "status": 401}),
        }
    }
    pub fn is_auth(&self) -> bool {
        matches!(self, RemoteError::Status(401, _) | RemoteError::NotLoggedIn)
    }
}

pub type RResult<T> = Result<T, RemoteError>;

fn map_err(e: ureq::Error) -> RemoteError {
    match e {
        ureq::Error::Status(code, resp) => RemoteError::Status(code, resp.into_json::<Value>().unwrap_or(Value::Null)),
        ureq::Error::Transport(t) => RemoteError::Transport(t.to_string()),
    }
}

fn build_agent(ca_pem: Option<&std::path::Path>) -> Result<ureq::Agent, String> {
    let mut tls = native_tls::TlsConnector::builder();
    if let Some(p) = ca_pem {
        let pem = std::fs::read(p).map_err(|e| format!("{}: {}", p.display(), e))?;
        let cert = native_tls::Certificate::from_pem(&pem).map_err(|e| format!("CA certificate: {}", e))?;
        tls.add_root_certificate(cert);
    }
    let tls = tls.build().map_err(|e| e.to_string())?;
    Ok(ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(120))
        .timeout_write(Duration::from_secs(120))
        .user_agent(&format!("OpenTraceDesktop/{}", env!("CARGO_PKG_VERSION")))
        .redirects(0)
        .tls_connector(Arc::new(tls))
        .build())
}

impl Remote {
    pub fn new(persist: bool) -> Remote {
        Remote { agent: Mutex::new(None), session: Mutex::new(None), persist }
    }

    fn agent(&self, ca_pem: Option<&std::path::Path>) -> Result<ureq::Agent, RemoteError> {
        let key = ca_pem.map(|p| p.display().to_string()).unwrap_or_default();
        let mut g = self.agent.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((k, a)) = g.as_ref() {
            if *k == key {
                return Ok(a.clone());
            }
        }
        let a = build_agent(ca_pem).map_err(RemoteError::Transport)?;
        *g = Some((key, a.clone()));
        Ok(a)
    }

    pub fn reset_agent(&self) {
        *self.agent.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    pub fn session(&self) -> Option<Session> {
        self.session.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// health + capabilities, with an API version compatibility verdict.
    pub fn check(&self, server: &str, ca: Option<&std::path::Path>) -> RResult<Value> {
        let a = self.agent(ca)?;
        let t0 = std::time::Instant::now();
        let health: Value = a.get(&format!("{}/api/v1/health", server)).call().map_err(map_err)?.into_json().map_err(|e| RemoteError::Transport(e.to_string()))?;
        let caps: Value = a.get(&format!("{}/api/v1/capabilities", server)).call().map_err(map_err)?.into_json().map_err(|e| RemoteError::Transport(e.to_string()))?;
        let compatible = caps["api_versions"].as_array().map(|v| v.iter().any(|x| x == 1)).unwrap_or(false);
        let local = opentrace_core::server_analyze::analysis_version();
        Ok(json!({
            "ok": health["status"] == "ok", "compatible": compatible, "latency_ms": t0.elapsed().as_millis() as u64,
            "server": health, "capabilities": caps,
            "analysis_version_local": local,
            "analysis_version_match": health["analysis_version"] == json!(local),
            "math_backend_local": opentrace_core::cmath::BACKEND,
        }))
    }

    pub fn login(&self, server: &str, ca: Option<&std::path::Path>, user: &str, password: &str) -> RResult<Session> {
        let a = self.agent(ca)?;
        let client = format!("OpenTrace Desktop {} ({})", env!("CARGO_PKG_VERSION"), std::env::consts::OS);
        let r: Value = a
            .post(&format!("{}/api/v1/auth/login", server))
            .send_json(json!({"username": user, "password": password, "client": client}))
            .map_err(map_err)?
            .into_json()
            .map_err(|e| RemoteError::Transport(e.to_string()))?;
        let s = Session {
            server: server.to_string(),
            user: user.to_string(),
            token: r["token"].as_str().unwrap_or("").to_string(),
            roles: r["user"]["roles"].as_array().map(|v| v.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
            expires_ms: r["expires_ms"].as_i64().unwrap_or(0),
        };
        if self.persist {
            store_token(&s);
        }
        *self.session.lock().unwrap_or_else(|e| e.into_inner()) = Some(s.clone());
        Ok(s)
    }

    pub fn logout(&self, ca: Option<&std::path::Path>) {
        let s = self.session.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(s) = s {
            if let Ok(a) = self.agent(ca) {
                let _ = a.post(&format!("{}/api/v1/auth/logout", s.server)).set("Authorization", &format!("Bearer {}", s.token)).send_json(json!({}));
            }
            forget_token(&s.server, &s.user);
        }
    }

    /// Restores a session saved in the OS credential store and checks it with `/me`.
    pub fn resume(&self, server: &str, user: &str, ca: Option<&std::path::Path>) -> RResult<Session> {
        let token = if self.persist { load_token(server, user) } else { None }.ok_or(RemoteError::NotLoggedIn)?;
        let a = self.agent(ca)?;
        let me: Value = a
            .get(&format!("{}/api/v1/me", server))
            .set("Authorization", &format!("Bearer {}", token))
            .call()
            .map_err(|e| {
                let e = map_err(e);
                if e.is_auth() {
                    forget_token(server, user);
                }
                e
            })?
            .into_json()
            .map_err(|e| RemoteError::Transport(e.to_string()))?;
        let s = Session {
            server: server.into(),
            user: user.into(),
            token,
            roles: me["roles"].as_array().map(|v| v.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
            expires_ms: 0,
        };
        *self.session.lock().unwrap_or_else(|e| e.into_inner()) = Some(s.clone());
        Ok(s)
    }

    fn authed(&self) -> RResult<Session> {
        self.session().ok_or(RemoteError::NotLoggedIn)
    }

    pub fn get_json(&self, ca: Option<&std::path::Path>, path: &str) -> RResult<Value> {
        let s = self.authed()?;
        let a = self.agent(ca)?;
        a.get(&format!("{}{}", s.server, path))
            .set("Authorization", &format!("Bearer {}", s.token))
            .call()
            .map_err(|e| self.on_err(map_err(e)))?
            .into_json()
            .map_err(|e| RemoteError::Transport(e.to_string()))
    }

    pub fn post_json(&self, ca: Option<&std::path::Path>, path: &str, headers: &[(&str, String)], body: &Value) -> RResult<Value> {
        let s = self.authed()?;
        let a = self.agent(ca)?;
        let mut r = a.post(&format!("{}{}", s.server, path)).set("Authorization", &format!("Bearer {}", s.token));
        for (k, v) in headers {
            r = r.set(k, v);
        }
        r.send_json(body.clone()).map_err(|e| self.on_err(map_err(e)))?.into_json().map_err(|e| RemoteError::Transport(e.to_string()))
    }

    pub fn delete(&self, ca: Option<&std::path::Path>, path: &str, headers: &[(&str, String)]) -> RResult<Value> {
        let s = self.authed()?;
        let a = self.agent(ca)?;
        let mut r = a.delete(&format!("{}{}", s.server, path)).set("Authorization", &format!("Bearer {}", s.token));
        for (k, v) in headers {
            r = r.set(k, v);
        }
        r.call().map_err(|e| self.on_err(map_err(e)))?.into_json().map_err(|e| RemoteError::Transport(e.to_string()))
    }

    /// Generic JSON call for the allow-listed proxy of the desktop pages (`GET`/`POST`/`PUT`).
    pub fn call(&self, ca: Option<&std::path::Path>, method: &str, path: &str, body: Option<&Value>) -> RResult<Value> {
        let s = self.authed()?;
        let a = self.agent(ca)?;
        let r = a.request(method, &format!("{}{}", s.server, path)).set("Authorization", &format!("Bearer {}", s.token));
        let res = match body {
            Some(b) => r.send_json(b.clone()),
            None => r.call(),
        };
        res.map_err(|e| self.on_err(map_err(e)))?.into_json().map_err(|e| RemoteError::Transport(e.to_string()))
    }

    /// `HEAD /api/v1/fsa/{sha}`: true when the server already has the file.
    pub fn fsa_exists(&self, ca: Option<&std::path::Path>, sha: &str) -> RResult<bool> {
        let s = self.authed()?;
        let a = self.agent(ca)?;
        match a.head(&format!("{}/api/v1/fsa/{}", s.server, sha)).set("Authorization", &format!("Bearer {}", s.token)).call() {
            Ok(_) => Ok(true),
            Err(ureq::Error::Status(404, _)) => Ok(false),
            Err(e) => Err(self.on_err(map_err(e))),
        }
    }

    /// Binary upload (streamed from disk, never base64). Idempotent: 201 created or 200 already present.
    pub fn fsa_put(&self, ca: Option<&std::path::Path>, sha: &str, len: u64, reader: impl Read) -> RResult<Value> {
        let s = self.authed()?;
        let a = self.agent(ca)?;
        a.put(&format!("{}/api/v1/fsa/{}", s.server, sha))
            .set("Authorization", &format!("Bearer {}", s.token))
            .set("Content-Type", "application/octet-stream")
            .set("Content-Length", &len.to_string())
            .send(reader)
            .map_err(|e| self.on_err(map_err(e)))?
            .into_json()
            .map_err(|e| RemoteError::Transport(e.to_string()))
    }

    fn on_err(&self, e: RemoteError) -> RemoteError {
        if matches!(e, RemoteError::Status(401, _)) {
            if let Some(s) = self.session.lock().unwrap_or_else(|e| e.into_inner()).take() {
                forget_token(&s.server, &s.user);
            }
        }
        e
    }
}

fn entry(server: &str, user: &str) -> Option<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, &format!("{}|{}", server, user)).ok()
}

/// Session token into the OS credential store (Windows Credential Manager / Linux kernel keyring). If the store is not
/// available the session simply is not remembered across restarts; the token is never written to a plain file.
fn store_token(s: &Session) {
    if let Some(e) = entry(&s.server, &s.user) {
        let _ = e.set_password(&s.token);
    }
}

fn load_token(server: &str, user: &str) -> Option<String> {
    entry(server, user)?.get_password().ok()
}

fn forget_token(server: &str, user: &str) {
    if let Some(e) = entry(server, user) {
        let _ = e.delete_credential();
    }
}
