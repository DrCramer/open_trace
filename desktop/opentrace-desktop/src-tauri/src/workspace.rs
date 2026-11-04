//! ProjectWorkspace: server projects in the unchanged web UI.
//!
//! When the desktop is set to keep projects on the organisation server, the UI's own project requests
//! (`POST /api/idx-project` with `list`, `open`, `open_sample`, `save_meta`, `save_sample`, `export`) are answered
//! here instead of by the local project library:
//!
//! * `open` takes the project's edit lease. If somebody else holds it (or the user has no edit right) the project
//!   opens **read-only**; saving is then refused with the holder's name — never a silent overwrite.
//! * every write carries lease id, fencing token and the revision the client last saw; the server rejects stale ones.
//!   A revision conflict or a lost lease is reported and the project becomes read-only for this client.
//! * `save_sample` sends the original FSA once, binary, by SHA-256 (`HEAD`/`PUT /api/v1/fsa/{sha}`), links it to the
//!   project and stores the sample with `fsa_sha256` — no base64 FSA in the JSON to the server.
//! * a heartbeat renews held leases; leases are released when another project is opened, on logout and at exit.
//! * without a connection there is no editing of server projects (no offline last-write-wins).
use crate::remote::{Remote, RemoteError};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Held {
    pub lease_id: Option<String>,
    pub fencing: i64,
    pub revision: i64,
    /// Why this client may not write (holder name, missing right, lost lease, conflict).
    pub readonly: Option<String>,
    pub expires_ms: i64,
}

pub struct Workspace {
    remote: Arc<Remote>,
    ca: Arc<dyn Fn() -> Option<PathBuf> + Send + Sync>,
    held: Mutex<HashMap<String, Held>>,
    /// The project currently shown in the main window (the UI works on one project at a time).
    current: Mutex<Option<String>>,
    /// Serialises writes so revisions advance in order.
    write: Mutex<()>,
    /// Analyses run in this session, posted as provenance when their sample is saved to a server project.
    pub ledger: crate::provenance::Ledger,
    /// Active laboratory profile (verified on activation) or why it could not be used.
    pub profile: Mutex<Result<Option<crate::labprofile::Active>, String>>,
}

#[derive(Debug)]
pub struct WsError {
    pub status: u16,
    pub body: Value,
}

impl WsError {
    fn msg(status: u16, m: impl Into<String>) -> Self {
        WsError { status, body: json!({"error": m.into()}) }
    }
}

impl From<RemoteError> for WsError {
    fn from(e: RemoteError) -> Self {
        let v = e.to_json();
        let status = match v["status"].as_u64() {
            Some(0) | None => 502,
            Some(c) => c as u16,
        };
        WsError { status, body: v }
    }
}

pub fn enc(name: &str) -> String {
    percent_encoding::utf8_percent_encode(name, percent_encoding::NON_ALPHANUMERIC).to_string()
}

fn code(e: &RemoteError) -> Option<String> {
    match e {
        RemoteError::Status(_, v) => v.get("code").and_then(|c| c.as_str()).map(String::from),
        _ => None,
    }
}

impl Workspace {
    pub fn new(remote: Arc<Remote>, ca: Arc<dyn Fn() -> Option<PathBuf> + Send + Sync>) -> Workspace {
        Workspace { remote, ca, held: Mutex::new(HashMap::new()), current: Mutex::new(None), write: Mutex::new(()), ledger: Default::default(), profile: Mutex::new(Ok(None)) }
    }

    fn ca(&self) -> Option<PathBuf> {
        (self.ca)()
    }

    fn held(&self) -> std::sync::MutexGuard<'_, HashMap<String, Held>> {
        self.held.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn state_of(&self, name: &str) -> Option<Held> {
        self.held().get(name).cloned()
    }

    pub fn status(&self) -> Value {
        let cur = self.current.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let h = cur.as_ref().and_then(|n| self.state_of(n));
        json!({
            "project": cur,
            "editing": h.as_ref().map(|h| h.readonly.is_none() && h.lease_id.is_some()).unwrap_or(false),
            "readonly": h.as_ref().and_then(|h| h.readonly.clone()),
            "revision": h.as_ref().map(|h| h.revision),
            "lease_expires_ms": h.as_ref().map(|h| h.expires_ms),
            "provenance": self.ledger.status(),
            "lab_profile": self.profile_status(),
        })
    }

    /// Entry point for a UI `/api/idx-project` request in server mode.
    pub fn handle(&self, req: &Value) -> Result<Value, WsError> {
        let action = req.get("action").and_then(|a| a.as_str()).unwrap_or("");
        let name = req.get("name").and_then(|n| n.as_str()).map(|s| s.trim().to_string());
        match (action, name) {
            ("list", _) => self.list(),
            ("open", Some(n)) => self.open(&n),
            ("open_sample", Some(n)) | ("export", Some(n)) => {
                let mut r = req.clone();
                r.as_object_mut().unwrap().remove("name");
                // The FSA comes by SHA-256 (from the local cache when present) instead of base64 inside the JSON.
                r["fsa_by_sha"] = json!(true);
                Ok(self.remote.post_json(self.ca().as_deref(), &format!("/api/v1/projects/{}/idx", enc(&n)), &[], &r)?)
            }
            ("save_meta", Some(n)) => self.save_meta(&n, req),
            ("save_sample", Some(n)) => self.save_sample(&n, req),
            ("delete" | "rename" | "save_as" | "import", _) => Err(WsError::msg(
                400,
                "В режиме «проекты на сервере» удаление, переименование, копирование и импорт выполняет администратор в веб-интерфейсе сервера (Менеджер проектов). Сохранить под новым именем можно командой «Сохранить проект как».",
            )),
            _ => Err(WsError::msg(400, "Unsupported project request")),
        }
    }

    fn list(&self) -> Result<Value, WsError> {
        let r = self.remote.get_json(self.ca().as_deref(), "/api/v1/projects")?;
        let server = self.remote.session().map(|s| s.server).unwrap_or_default();
        Ok(json!({"root": format!("сервер {}", server), "projects": r["projects"], "server": true}))
    }

    /// Takes the edit lease if possible, otherwise opens read-only.
    pub fn open(&self, name: &str) -> Result<Value, WsError> {
        let _w = self.write.lock().unwrap_or_else(|e| e.into_inner());
        let previous = self.current.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(p) = previous.filter(|p| p != name) {
            self.release(&p);
        }
        let ca = self.ca();
        let held = match self.remote.post_json(ca.as_deref(), &format!("/api/v1/projects/{}/lease", enc(name)), &[], &json!({})) {
            Ok(l) => Held {
                lease_id: l["lease_id"].as_str().map(String::from),
                fencing: l["fencing_token"].as_i64().unwrap_or(0),
                revision: l["revision"].as_i64().unwrap_or(0),
                readonly: None,
                expires_ms: l["expires_ms"].as_i64().unwrap_or(0),
            },
            Err(RemoteError::Status(409, v)) => Held {
                readonly: Some(format!(
                    "Проект редактирует {}{}",
                    v["locked_by"].as_str().unwrap_or("другой пользователь"),
                    v["client"].as_str().map(|c| format!(" ({})", c)).unwrap_or_default()
                )),
                ..Default::default()
            },
            Err(RemoteError::Status(403, _)) => Held { readonly: Some("Нет права редактирования проектов (роль expert)".into()), ..Default::default() },
            Err(e) => return Err(e.into()),
        };
        let got = match self.remote.get_json(ca.as_deref(), &format!("/api/v1/projects/{}", enc(name))) {
            Ok(v) => v,
            Err(e) => {
                if held.lease_id.is_some() {
                    self.held().insert(name.to_string(), held);
                    self.release(name);
                }
                return Err(e.into());
            }
        };
        let mut held = held;
        if held.readonly.is_some() {
            held.revision = got["revision"].as_i64().unwrap_or(0);
        }
        let readonly = held.readonly.clone();
        self.held().insert(name.to_string(), held);
        *self.current.lock().unwrap_or_else(|e| e.into_inner()) = Some(name.to_string());
        let mut project = got["project"].clone();
        if let Some(m) = project.as_object_mut() {
            m.insert("opentrace_server".into(), json!({"readonly": readonly, "revision": got["revision"]}));
        }
        Ok(project)
    }

    fn guard_headers(h: &Held) -> Vec<(&'static str, String)> {
        vec![
            ("X-Lease-Id", h.lease_id.clone().unwrap_or_default()),
            ("X-Fencing-Token", h.fencing.to_string()),
            ("X-Expected-Revision", h.revision.to_string()),
        ]
    }

    /// Lease for a write: the held one, or a fresh one for a project saved under a new name.
    fn writable(&self, name: &str) -> Result<Held, WsError> {
        if let Some(h) = self.state_of(name) {
            if let Some(why) = &h.readonly {
                return Err(WsError::msg(409, format!("Проект «{}» открыт только для чтения: {}. Изменения на сервер не сохранены; сохраните их под другим именем или повторите, когда проект освободится.", name, why)));
            }
            if h.lease_id.is_some() {
                return Ok(h);
            }
        }
        let l = self
            .remote
            .post_json(self.ca().as_deref(), &format!("/api/v1/projects/{}/lease", enc(name)), &[], &json!({}))
            .map_err(|e| match &e {
                RemoteError::Status(409, v) => WsError::msg(409, format!("Проект «{}» на сервере редактирует {}. Сохранение отклонено.", name, v["locked_by"].as_str().unwrap_or("другой пользователь"))),
                _ => e.into(),
            })?;
        let h = Held {
            lease_id: l["lease_id"].as_str().map(String::from),
            fencing: l["fencing_token"].as_i64().unwrap_or(0),
            revision: l["revision"].as_i64().unwrap_or(0),
            readonly: None,
            expires_ms: l["expires_ms"].as_i64().unwrap_or(0),
        };
        self.held().insert(name.to_string(), h.clone());
        Ok(h)
    }

    /// Runs one guarded write and advances the local revision; conflicts make the project read-only here.
    fn guarded(&self, name: &str, path: &str, body: &Value) -> Result<Value, WsError> {
        let h = self.writable(name)?;
        match self.remote.post_json(self.ca().as_deref(), path, &Self::guard_headers(&h), body) {
            Ok(v) => {
                if let Some(r) = v["revision"].as_i64() {
                    if let Some(x) = self.held().get_mut(name) {
                        x.revision = r;
                    }
                }
                Ok(v)
            }
            Err(e) => {
                let why = match code(&e).as_deref() {
                    Some("revision_conflict") => Some("проект изменён на сервере другим пользователем после открытия".to_string()),
                    Some("lease_lost") | Some("project_locked") => Some("блокировка редактирования потеряна (истекла или сброшена администратором)".to_string()),
                    _ => None,
                };
                if let Some(why) = why {
                    if let Some(x) = self.held().get_mut(name) {
                        x.readonly = Some(why.clone());
                        x.lease_id = None;
                    }
                    return Err(WsError::msg(409, format!("Сохранение на сервер отклонено: {}. Ваши изменения не записаны; откройте проект заново или сохраните под другим именем.", why)));
                }
                Err(e.into())
            }
        }
    }

    fn save_meta(&self, name: &str, req: &Value) -> Result<Value, WsError> {
        let _w = self.write.lock().unwrap_or_else(|e| e.into_inner());
        let mut body = req.clone();
        body.as_object_mut().unwrap().remove("name");
        let r = self.guarded(name, &format!("/api/v1/projects/{}/idx", enc(name)), &body)?;
        // Saved under another name ("Save As"): this becomes the open project; release the previous one.
        let previous = self.current.lock().unwrap_or_else(|e| e.into_inner()).replace(name.to_string());
        if let Some(p) = previous.filter(|p| p != name) {
            self.release(&p);
        }
        Ok(r["result"].clone())
    }

    fn save_sample(&self, name: &str, req: &Value) -> Result<Value, WsError> {
        let _w = self.write.lock().unwrap_or_else(|e| e.into_inner());
        let mut body = req.clone();
        let obj = body.as_object_mut().unwrap();
        obj.remove("name");
        if let Some(b64) = obj.remove("fsa_b64").filter(|v| !v.is_null()) {
            let bytes = opentrace_core::matrix::b64decode_strict(b64.as_str().unwrap_or("")).map_err(|e| WsError::msg(400, e))?;
            let sha = crate::sync::hex(&<sha2::Sha256 as sha2::Digest>::digest(&bytes));
            let ca = self.ca();
            if !self.remote.fsa_exists(ca.as_deref(), &sha)? {
                self.remote.fsa_put(ca.as_deref(), &sha, bytes.len() as u64, std::io::Cursor::new(&bytes))?;
            }
            let id = obj.get("id").map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string())).unwrap_or_default();
            self.guarded(name, &format!("/api/v1/projects/{}/fsa", enc(name)), &json!({"sha256": sha, "filename": format!("{}.fsa", id)}))?;
            obj.insert("fsa_sha256".into(), json!(sha));
        }
        let r = self.guarded(name, &format!("/api/v1/projects/{}/idx", enc(name)), &body)?;
        if let Some(fsa_sha) = req["data"]["analysis"]["source_sha256"].as_str() {
            let id = req.get("id").map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string())).unwrap_or_default();
            self.post_provenance(name, &id, fsa_sha);
        }
        Ok(r["result"].clone())
    }

    /// Provenance of the saved sample's analysis (see `provenance.rs`). Never fails the save itself.
    fn post_provenance(&self, name: &str, id: &str, fsa_sha: &str) {
        let rec = match self.ledger.lookup(fsa_sha) {
            Ok(r) => r,
            Err(0) => return, // not analysed in this session (e.g. a project opened from the server)
            Err(_) => {
                if self.ledger.mark_posted(name, id, &format!("ambiguous:{}", fsa_sha)) {
                    self.ledger.note(Ok(()), true);
                }
                return;
            }
        };
        if !self.ledger.mark_posted(name, id, &rec.result_sha256) {
            return;
        }
        let mut body = json!({"sample_id": id, "fsa_sha256": rec.fsa_sha256, "analysis_version": rec.analysis_version,
                              "settings": rec.settings, "result_sha256": rec.result_sha256});
        if let Ok(Some(p)) = &*self.profile.lock().unwrap_or_else(|e| e.into_inner()) {
            match crate::labprofile::check(p, &rec.settings).and_then(|_| self.ledger.genotyping_verdict(&rec.fsa_sha256)) {
                Ok(()) => body["lab_profile"] = json!({"name": p.name, "version": p.version}),
                Err(why) => self.ledger.note_profile_mismatch(&format!("образец {}: {}", id, why)),
            }
        }
        match self.guarded(name, &format!("/api/v1/projects/{}/analyses", enc(name)), &body) {
            Ok(_) => self.ledger.note(Ok(()), false),
            Err(e) => {
                self.ledger.unmark_posted(name, id, &rec.result_sha256);
                self.ledger.note(Err(e.body["error"].as_str().map(String::from).unwrap_or_else(|| e.body.to_string())), false);
            }
        }
    }

    pub fn profile_status(&self) -> Value {
        match &*self.profile.lock().unwrap_or_else(|e| e.into_inner()) {
            Ok(Some(p)) => json!({"name": p.name, "version": p.version, "content_sha256": p.content_sha256, "method": p.method,
                                  "size_standard": p.size_standard, "verified": true}),
            Ok(None) => Value::Null,
            Err(e) => json!({"verified": false, "error": e}),
        }
    }

    pub fn release(&self, name: &str) {
        let h = self.held().remove(name);
        if let Some(Held { lease_id: Some(id), .. }) = h {
            let _ = self.remote.delete(self.ca().as_deref(), &format!("/api/v1/projects/{}/lease", enc(name)), &[("X-Lease-Id", id)]);
        }
        let mut cur = self.current.lock().unwrap_or_else(|e| e.into_inner());
        if cur.as_deref() == Some(name) {
            *cur = None;
        }
    }

    /// Self-test only: the server forgets this client's lease (as after expiry or an administrator's reset) while the
    /// client still believes it holds it — the next save is refused with `lease_lost`.
    pub fn drop_lease_on_server(&self, name: &str) -> bool {
        let id = self.state_of(name).and_then(|h| h.lease_id);
        match id {
            Some(id) => self.remote.delete(self.ca().as_deref(), &format!("/api/v1/projects/{}/lease", enc(name)), &[("X-Lease-Id", id)]).is_ok(),
            None => false,
        }
    }

    pub fn release_all(&self) {
        let names: Vec<String> = self.held().keys().cloned().collect();
        for n in names {
            self.release(&n);
        }
    }

    /// Heartbeat: renews every held lease; a failed renewal turns the project read-only for this client.
    pub fn renew_all(&self) {
        let items: Vec<(String, String)> = self.held().iter().filter_map(|(n, h)| h.lease_id.clone().map(|id| (n.clone(), id))).collect();
        for (n, id) in items {
            match self.remote.post_json(self.ca().as_deref(), &format!("/api/v1/projects/{}/lease/renew", enc(&n)), &[], &json!({"lease_id": id})) {
                Ok(l) => {
                    if let Some(h) = self.held().get_mut(&n) {
                        h.expires_ms = l["expires_ms"].as_i64().unwrap_or(h.expires_ms);
                    }
                }
                Err(RemoteError::Transport(_)) => {} // offline for a moment: retry on the next beat; the server lets it expire otherwise
                Err(_) => {
                    if let Some(h) = self.held().get_mut(&n) {
                        h.readonly = Some("блокировка редактирования потеряна (истекла или сброшена администратором)".into());
                        h.lease_id = None;
                    }
                }
            }
        }
    }

    pub fn start_heartbeat(self: &Arc<Self>, every: std::time::Duration) {
        let me = Arc::downgrade(self);
        std::thread::Builder::new()
            .name("lease-heartbeat".into())
            .spawn(move || loop {
                std::thread::sleep(every);
                match me.upgrade() {
                    Some(w) => w.renew_all(),
                    None => return,
                }
            })
            .expect("spawn heartbeat");
    }
}
