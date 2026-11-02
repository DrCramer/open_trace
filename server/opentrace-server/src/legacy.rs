//! The existing web UI (`/`, `/idx/`, `/api/*`) served unchanged through `opentrace_core::server::App::handle`,
//! with these additions for project writes from the web:
//!
//! * refused while another client holds the project's edit lease;
//! * every accepted web write advances the project revision (a desktop editor with an older revision gets a conflict
//!   instead of silently overwriting it);
//! * optimistic check between browsers: the server remembers, per browser session (`X-User-Ticket`, stored hashed)
//!   and project, the revision that session last opened or wrote (`web_views`). A write is refused when the project
//!   changed since — two web users can no longer overwrite each other's work. A project that does not exist yet, or was
//!   never changed through this server (revision 0), may be written without having been opened (new project, Save As).
use crate::leases;
use crate::state::Shared;
use axum::body::Body;
use axum::http::{HeaderValue, Request, Response, StatusCode};
use opentrace_core::idx_projects::LErr;
use opentrace_core::security::{SecurityStore, DEFAULT_SECURITY_GROUP};
use serde::Deserialize;
use serde_json::{json, Value};

/// Project library actions of `/api/idx-project` that change stored data.
pub const MUTATING: [&str; 6] = ["save_meta", "save_sample", "delete", "import", "rename", "save_as"];

#[derive(Deserialize)]
struct Peek {
    action: Option<String>,
    name: Option<Value>,
    new_name: Option<Value>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Touch {
    /// Read-only request.
    None,
    /// Mutation of these projects.
    Names(Vec<String>),
    /// Mutation whose target is only known after processing (import without `new_name`).
    Unknown,
}

/// Which projects a legacy `/api/idx-project` request would modify.
pub fn touched_projects(body: &[u8]) -> Touch {
    let Ok(p) = serde_json::from_slice::<Peek>(body) else { return Touch::None };
    let action = p.action.as_deref().unwrap_or("");
    if !MUTATING.contains(&action) {
        return Touch::None;
    }
    let names: Vec<String> =
        [p.name, p.new_name].into_iter().flatten().filter_map(|v| v.as_str().map(|s| s.trim().to_string())).filter(|s| !s.is_empty()).collect();
    if action == "import" && names.is_empty() {
        return Touch::Unknown;
    }
    Touch::Names(names)
}

fn peek(body: &[u8]) -> (String, Option<String>) {
    let p = serde_json::from_slice::<Peek>(body).ok();
    let action = p.as_ref().and_then(|p| p.action.clone()).unwrap_or_default();
    let name = p.and_then(|p| p.name).and_then(|v| v.as_str().map(|s| s.trim().to_string())).filter(|s| !s.is_empty());
    (action, name)
}

fn ticket_key(headers: &axum::http::HeaderMap) -> String {
    crate::db::sha256_hex(headers.get("x-user-ticket").and_then(|v| v.to_str().ok()).unwrap_or("").as_bytes())
}

fn view_of(db: &rusqlite::Connection, key: &str, project: &str) -> Result<Option<i64>, String> {
    crate::db::get_opt::<i64>(db, "SELECT revision FROM web_views WHERE ticket_sha256 = ?1 AND project = ?2", rusqlite::params![key, project])
}

fn set_view(db: &rusqlite::Connection, key: &str, project: &str, revision: i64) {
    let _ = db.execute(
        "INSERT INTO web_views (ticket_sha256, project, revision, updated_ms) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (ticket_sha256, project) DO UPDATE SET revision = excluded.revision, updated_ms = excluded.updated_ms",
        rusqlite::params![key, project, revision, crate::db::now_ms()],
    );
}

const VIEW_RETENTION_MS: i64 = 30 * 24 * 3600 * 1000;

fn any_active_lease(db: &rusqlite::Connection, now: i64) -> Result<Option<(String, String)>, String> {
    db.query_row("SELECT name, user_name FROM project_leases WHERE expires_ms > ?1 LIMIT 1", [now], |r| Ok((r.get(0)?, r.get(1)?)))
        .map(Some)
        .or_else(|e| if matches!(e, rusqlite::Error::QueryReturnedNoRows) { Ok(None) } else { Err(e.to_string()) })
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::String(s) => !s.is_empty(),
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::Array(a) => !a.is_empty(),
        Value::Object(m) => !m.is_empty(),
    }
}

/// Runs `f` with the same object-level access check the web endpoint uses (security groups, owner rights).
pub fn with_access<T>(store: &SecurityStore, user: &Value, f: impl FnOnce(&dyn Fn(&str, &Value, &Value, bool) -> Result<bool, LErr>) -> T) -> T {
    let access = |op: &str, group: &Value, owner: &Value, check: bool| -> Result<bool, LErr> {
        let g = if truthy(group) { group.clone() } else { json!(DEFAULT_SECURITY_GROUP) };
        let allowed = store.object_allows(Some(user), &g, op, owner);
        if check {
            return Ok(allowed);
        }
        if !allowed {
            store.require_object(Some(user), &g, op, owner)?;
        }
        Ok(true)
    };
    f(&access)
}

fn to_core(method: &str, target: String, headers: &axum::http::HeaderMap, body: Vec<u8>) -> opentrace_core::server::Request {
    let mut hs: Vec<(String, String)> = Vec::new();
    for (k, v) in headers.iter() {
        if let Ok(v) = v.to_str() {
            hs.push((k.as_str().to_ascii_lowercase(), v.to_string()));
        }
    }
    opentrace_core::server::Request { method: method.to_string(), target, headers: hs, body }
}

fn respond(code: u16, kind: &str, body: Vec<u8>) -> Response<Body> {
    let mut r = Response::new(Body::from(body));
    *r.status_mut() = StatusCode::from_u16(code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    if let Ok(v) = HeaderValue::from_str(kind) {
        r.headers_mut().insert(axum::http::header::CONTENT_TYPE, v);
    }
    r.headers_mut().insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    r
}

fn json_err(code: u16, v: Value) -> Response<Body> {
    respond(code, "application/json; charset=utf-8", v.to_string().into_bytes())
}

pub async fn handle(st: Shared, req: Request<Body>) -> Response<Body> {
    let method = req.method().as_str().to_string();
    let target = req.uri().path_and_query().map(|p| p.as_str().to_string()).unwrap_or_else(|| "/".into());
    let path = req.uri().path().to_string();
    let headers = req.headers().clone();
    let limit = if path == "/api/analyze" || path == "/api/sample-info" {
        opentrace_core::abif::MAX_FILE_BYTES
    } else {
        (st.cfg.legacy_max_body_mib * 1024 * 1024) as usize
    };
    let body = match axum::body::to_bytes(req.into_body(), limit).await {
        Ok(b) => b.to_vec(),
        Err(_) => return json_err(413, json!({"error": format!("Request too large (limit {} MiB)", limit / (1024 * 1024))})),
    };
    let st2 = st.clone();
    tokio::task::spawn_blocking(move || {
        let core_req = to_core(&method, target, &headers, body);
        if method == "POST" && path == "/api/idx-project" {
            let touch = touched_projects(&core_req.body);
            if touch != Touch::None {
                // Lease check, write and revision bump under the database lock: a lease cannot be granted in between.
                let mut db = st2.db();
                let now = crate::db::now_ms();
                let names = match &touch {
                    Touch::Names(n) => n.clone(),
                    _ => vec![],
                };
                if touch == Touch::Unknown {
                    match any_active_lease(&db, now) {
                        Ok(Some((p, u))) => {
                            return json_err(409, json!({"error": format!("Импорт без явного имени отклонён: проект «{}» сейчас редактирует {}. Укажите новое имя. / Import without an explicit name is refused while projects are leased.", p, u), "code": "project_locked"}));
                        }
                        Ok(None) => {}
                        Err(e) => return json_err(500, json!({"error": e})),
                    }
                }
                for n in &names {
                    match leases::active(&db, n, now) {
                        Ok(Some(h)) => {
                            return json_err(409, json!({
                                "error": format!("Проект «{}» сейчас редактирует {}{}. Сохранение из веб-интерфейса отклонено; откройте проект только для чтения. / Project is being edited by {}.",
                                    n, h.user, h.client.as_deref().map(|c| format!(" ({})", c)).unwrap_or_default(), h.user),
                                "code": "project_locked", "locked_by": h.user, "lease_expires_ms": h.expires_ms}));
                        }
                        Ok(None) => {}
                        Err(e) => return json_err(500, json!({"error": format!("{:?}", e)})),
                    }
                }
                let key = ticket_key(&headers);
                let (action, _) = peek(&core_req.body);
                for (i, n) in names.iter().enumerate() {
                    // The target of rename / save_as / import (`new_name`) is a new project: nothing to compare with.
                    if i > 0 || action == "import" {
                        continue;
                    }
                    let current = match leases::revision(&db, n) {
                        Ok(r) => r,
                        Err(e) => return json_err(500, json!({"error": format!("{:?}", e)})),
                    };
                    let seen = match view_of(&db, &key, n) {
                        Ok(v) => v,
                        Err(e) => return json_err(500, json!({"error": e})),
                    };
                    let stale = match seen {
                        Some(r) => r != current,
                        // Not opened by this browser: fine for a new project (or a name reused after deletion).
                        None => current != 0 && opentrace_core::idx_projects::ProjectLibrary::new(Some(st2.legacy.projects_root())).exists(n),
                    };
                    if stale {
                        return json_err(409, json!({
                            "error": format!("Проект «{}» изменён на сервере после того, как вы его открыли (другим пользователем или в другом окне). Сохранение отклонено, чтобы не затереть чужие изменения: сохраните проект под другим именем или откройте его заново. / Project changed on the server since you opened it.", n),
                            "code": "revision_conflict", "revision": current}));
                    }
                }
                let r = st2.legacy.handle(&core_req);
                if r.code == 200 {
                    let mut bumped = names.clone();
                    if touch == Touch::Unknown {
                        if let Some(n) = serde_json::from_slice::<Value>(&r.body).ok().and_then(|v| v.get("name").and_then(|n| n.as_str()).map(|s| s.to_string())) {
                            bumped.push(n);
                        }
                    }
                    for n in &bumped {
                        if let Ok(r) = leases::bump_unleased(&mut db, n, None) {
                            set_view(&db, &key, n, r);
                        }
                    }
                }
                return respond(r.code, &r.kind, r.body);
            }
        }
        let r = st2.legacy.handle(&core_req);
        if r.code == 200 && method == "POST" && path == "/api/idx-project" {
            // Remember what this browser session has seen of the project it opened.
            if let (action, Some(name)) = peek(&core_req.body) {
                if action == "open" {
                    let db = st2.db();
                    let now = crate::db::now_ms();
                    let _ = db.execute("DELETE FROM web_views WHERE updated_ms < ?1", [now - VIEW_RETENTION_MS]);
                    if let Ok(rev) = leases::revision(&db, &name) {
                        set_view(&db, &ticket_key(&headers), &name, rev);
                    }
                }
            }
        }
        respond(r.code, &r.kind, r.body)
    })
    .await
    .unwrap_or_else(|e| json_err(500, json!({"error": e.to_string()})))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_mutations_and_their_targets() {
        assert_eq!(touched_projects(br#"{"action":"save_meta","name":"P1","project":{}}"#), Touch::Names(vec!["P1".into()]));
        assert_eq!(touched_projects(br#"{"action":"rename","name":"A","new_name":"B"}"#), Touch::Names(vec!["A".into(), "B".into()]));
        assert_eq!(touched_projects(br#"{"action":"import","package_b64":"x"}"#), Touch::Unknown);
        assert_eq!(touched_projects(br#"{"action":"open","name":"P1"}"#), Touch::None);
        assert_eq!(touched_projects(br#"{"action":"list"}"#), Touch::None);
        assert_eq!(touched_projects(b"not json"), Touch::None);
    }
}
