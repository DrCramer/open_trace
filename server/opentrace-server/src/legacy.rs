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
//! * single editor for the web as well: `open` by a user with the edit right takes the project's edit lease for that
//!   browser tab (`X-User-Ticket` + `X-OT-Tab`, client "web"); if another client (desktop or another tab) holds it,
//!   the project opens **read-only** and the reason is returned in `opentrace_server.readonly` — reading is never
//!   blocked. The page renews the lease (`lease_renew`, every 25 s) and releases it (`lease_release`, another project,
//!   closing the tab); an abandoned lease expires after `lease_ttl_seconds`. Writes are accepted from the holding tab.
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

/// Lease session of a browser tab: hashed ticket + the tab id the page sends (`X-OT-Tab`, letters/digits/`-`).
fn web_session(headers: &axum::http::HeaderMap) -> String {
    let tab: String = headers
        .get("x-ot-tab")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(64)
        .collect();
    format!("web:{}:{}", &ticket_key(headers)[..32], tab)
}

/// The web user behind the ticket and whether they may change projects.
fn web_user(st: &Shared, headers: &axum::http::HeaderMap) -> (String, bool) {
    let store = st.legacy.security_store();
    let ticket = headers.get("x-user-ticket").and_then(|v| v.to_str().ok());
    match store.session_user(ticket) {
        Ok(Some(u)) => {
            let can = store.require(Some(&u), &["UPDATE_PROJECT"]).is_ok();
            (u["name"].as_str().unwrap_or("web").to_string(), can)
        }
        _ => ("web".to_string(), false),
    }
}

fn held_text(h: &leases::Holder) -> String {
    format!("проект редактирует {}{}", h.user, h.client.as_deref().map(|c| format!(" ({})", c)).unwrap_or_default())
}

/// Takes (or re-confirms) the tab's edit lease: `Ok(None)` held, `Ok(Some(reason))` read-only.
fn web_take_lease(st: &Shared, db: &mut rusqlite::Connection, headers: &axum::http::HeaderMap, user: &str, can_edit: bool, name: &str) -> Result<Option<String>, String> {
    if !can_edit {
        return Ok(Some("нет права изменения проектов".into()));
    }
    let ttl = st.cfg.lease_ttl_seconds as i64 * 1000;
    match leases::acquire(db, name, user, &web_session(headers), Some("web"), ttl) {
        Ok(_) => Ok(None),
        Err(leases::LeaseError::Held(h)) => Ok(Some(held_text(&h))),
        Err(e) => Err(format!("{:?}", e)),
    }
}

fn web_lease_action(st: &Shared, headers: &axum::http::HeaderMap, action: &str, name: Option<String>) -> Response<Body> {
    let Some(name) = name else { return json_err(400, json!({"error": "name required"})) };
    let (user, can_edit) = web_user(st, headers);
    let mut db = st.db();
    if action == "lease_release" {
        return match leases::release_by_session(&mut db, &name, &web_session(headers)) {
            Ok(r) => json_err(200, json!({"released": r})),
            Err(e) => json_err(500, json!({"error": format!("{:?}", e)})),
        };
    }
    match web_take_lease(st, &mut db, headers, &user, can_edit, &name) {
        Ok(None) => json_err(200, json!({"held": true})),
        Ok(Some(why)) => json_err(200, json!({"held": false, "readonly": why})),
        Err(e) => json_err(500, json!({"error": e})),
    }
}

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
            let (action, pname) = peek(&core_req.body);
            if action == "lease_renew" || action == "lease_release" {
                return web_lease_action(&st2, &headers, &action, pname);
            }
        }
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
                let me = web_session(&headers);
                for n in &names {
                    // The tab holding the lease writes; anybody else is refused while it is held.
                    if matches!(leases::holder_session(&db, n, now), Ok(Some(ref s)) if *s == me) {
                        continue;
                    }
                    match leases::active(&db, n, now) {
                        Ok(Some(h)) => {
                            return json_err(409, json!({
                                "error": format!("Проект «{}» сейчас редактирует {}{}. Сохранение отклонено: у вас проект открыт только для чтения; копию можно сохранить командой «Сохранить проект как». / Project is being edited by {}.",
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
                    // The tab that writes a project is its editor: it takes the free lease (new project, Save As,
                    // a project it saved without opening), keeps one it holds, and gives up the others.
                    let edited = match action.as_str() {
                        "save_meta" | "save_sample" => names.first().cloned(),
                        "save_as" | "rename" => names.get(1).cloned(),
                        _ => None,
                    };
                    let me = web_session(&headers);
                    if action == "delete" || action == "rename" {
                        if let Some(n) = names.first() {
                            let _ = leases::release_by_session(&mut db, n, &me);
                        }
                    }
                    if let Some(n) = edited {
                        let (user, can_edit) = web_user(&st2, &headers);
                        if can_edit && matches!(leases::active(&db, &n, crate::db::now_ms()), Ok(None)) || matches!(leases::holder_session(&db, &n, crate::db::now_ms()), Ok(Some(ref s)) if *s == me) {
                            if let Ok(None) = web_take_lease(&st2, &mut db, &headers, &user, can_edit, &n) {
                                for other in leases::held_by_session(&db, &me).unwrap_or_default() {
                                    if other != n {
                                        let _ = leases::release_by_session(&mut db, &other, &me);
                                    }
                                }
                                // Tell the page it edits this project now (it starts the heartbeat and releases it).
                                if action != "save_sample" {
                                    if let Ok(mut v) = serde_json::from_slice::<Value>(&r.body) {
                                        if let Some(m) = v.as_object_mut() {
                                            let renew_ms = (st2.cfg.lease_ttl_seconds as i64 * 1000 / 3).clamp(3000, 25000);
                                            m.insert("opentrace_server".into(), json!({"web_lease": true, "readonly": null, "project": n, "renew_ms": renew_ms}));
                                            return respond(r.code, &r.kind, v.to_string().into_bytes());
                                        }
                                    }
                                }
                            }
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
                    let (user, can_edit) = web_user(&st2, &headers);
                    let mut db = st2.db();
                    let now = crate::db::now_ms();
                    let _ = db.execute("DELETE FROM web_views WHERE updated_ms < ?1", [now - VIEW_RETENTION_MS]);
                    // One project per tab: leases of other projects this tab held are released.
                    let me = web_session(&headers);
                    for other in leases::held_by_session(&db, &me).unwrap_or_default() {
                        if other != name {
                            let _ = leases::release_by_session(&mut db, &other, &me);
                        }
                    }
                    let readonly = match web_take_lease(&st2, &mut db, &headers, &user, can_edit, &name) {
                        Ok(r) => r,
                        Err(e) => Some(format!("блокировка недоступна: {}", e)),
                    };
                    let rev = leases::revision(&db, &name).unwrap_or(0);
                    set_view(&db, &ticket_key(&headers), &name, rev);
                    if let Ok(mut v) = serde_json::from_slice::<Value>(&r.body) {
                        if let Some(m) = v.as_object_mut() {
                            // Heartbeat well within the lease time (25 s at the default 90 s).
                            let renew_ms = (st2.cfg.lease_ttl_seconds as i64 * 1000 / 3).clamp(3000, 25000);
                            m.insert("opentrace_server".into(), json!({"web_lease": true, "readonly": readonly, "revision": rev, "renew_ms": renew_ms}));
                            return respond(r.code, &r.kind, v.to_string().into_bytes());
                        }
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
