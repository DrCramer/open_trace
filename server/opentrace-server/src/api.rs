//! HTTP API v1 (`/api/v1/*`). Every endpoint except health/capabilities/login requires a bearer session; rights are
//! checked here on the server, never trusted from the client.
use crate::auth::{self, Auth, ROLE_ADMIN, ROLE_APPROVER, ROLE_EXPERT, ROLE_METHOD_AUTHOR, ROLE_VIEWER};
use crate::db::{self, now_ms};
use crate::error::ApiError;
use crate::fsa::{self, PutError, PutOutcome};
use crate::leases;
use crate::standards;
use crate::state::Shared;
use axum::body::Body;
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use rusqlite::params;
use serde::Deserialize;
use serde_json::{json, Value};
use std::net::SocketAddr;

pub const API_VERSION: u32 = 1;

type R<T> = Result<T, ApiError>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> R<T> + Send + 'static) -> R<T> {
    tokio::task::spawn_blocking(f).await.map_err(|e| ApiError::internal(e.to_string()))?
}

// ------------------------------------------------------------------------------------------------ public

pub async fn health() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "api_version": API_VERSION,
        "server_version": env!("CARGO_PKG_VERSION"),
        "analysis_version": opentrace_core::server_analyze::analysis_version(),
        "math_backend": opentrace_core::cmath::BACKEND,
        "research_only": true,
        "time_ms": now_ms(),
    }))
}

pub async fn capabilities(State(st): State<Shared>) -> Json<Value> {
    Json(json!({
        "api_versions": [API_VERSION],
        "min_client_api": 1,
        "features": ["auth.password", "projects.lease", "projects.revision", "projects.idx", "fsa.content_addressed",
                     "analyses.provenance", "standards.lifecycle", "lab_profiles", if st.cfg.legacy_web { "legacy_web" } else { "" }],
        "gzip": {"requests": true, "responses": true},
        "limits": {"max_fsa_bytes": st.cfg.max_fsa_bytes(), "lease_ttl_seconds": st.cfg.lease_ttl_seconds,
                   "session_ttl_hours": st.cfg.session_ttl_hours},
        "transport": match st.cfg.transport { crate::config::Transport::Proxy => "tls-proxy", crate::config::Transport::Http => "http" },
        "roles": [ROLE_ADMIN, ROLE_APPROVER, ROLE_METHOD_AUTHOR, ROLE_EXPERT, ROLE_VIEWER],
        "standard_kinds": standards::KINDS,
    }))
}

#[derive(Deserialize)]
pub struct LoginReq {
    username: String,
    password: String,
    client: Option<String>,
}

pub async fn login(State(st): State<Shared>, ConnectInfo(addr): ConnectInfo<SocketAddr>, Json(req): Json<LoginReq>) -> R<Json<Value>> {
    if !auth::allow_login(&st, addr.ip()) {
        return Err(ApiError::new(StatusCode::TOO_MANY_REQUESTS, "rate_limited", "Too many login attempts; wait a minute"));
    }
    let client = req.client.map(|c| c.chars().filter(|c| !c.is_control()).take(80).collect::<String>());
    blocking(move || {
        let (doc, roles) = {
            let mut store = st.legacy.security_store();
            let r = store.login(&req.username, &req.password).map_err(|e| ApiError::unauthorized(lerr(&e)))?;
            if r.get("must_change_password").and_then(|v| v.as_bool()).unwrap_or(false) {
                return Err(ApiError::forbidden("Password expired: change it in the web Security Manager first").with(json!({"must_change_password": true})));
            }
            let doc = store.user(&json!(req.username)).ok_or_else(|| ApiError::unauthorized("unknown user"))?;
            let roles = auth::roles(&store, &doc);
            (doc, roles)
        };
        let s = auth::create_session(&st, &req.username, client.as_deref()).map_err(ApiError::internal)?;
        {
            let c = st.db();
            let _ = db::audit(&c, Some(&req.username), "auth.login", "session", Some(&s.session_id), &json!({"client": client}));
        }
        Ok(Json(json!({
            "token": s.token, "session_id": s.session_id, "expires_ms": s.expires_ms,
            "user": {"name": req.username, "full_name": doc.get("full_name"), "roles": roles},
        })))
    })
    .await
}

fn lerr(e: &opentrace_core::idx_projects::LErr) -> String {
    use opentrace_core::idx_projects::LErr::*;
    match e {
        Value(m) | Denied(m) | Other(m) => m.clone(),
    }
}

pub async fn logout(State(st): State<Shared>, a: Auth) -> R<Json<Value>> {
    blocking(move || {
        let mut c = st.db();
        c.execute("UPDATE sessions SET revoked = 1 WHERE session_id = ?1", params![a.session_id]).map_err(|e| ApiError::internal(e.to_string()))?;
        let released = leases::release_session(&mut c, &a.session_id)?;
        let _ = db::audit(&c, Some(&a.user), "auth.logout", "session", Some(&a.session_id), &json!({"leases_released": released}));
        Ok(Json(json!({"ok": true, "leases_released": released})))
    })
    .await
}

pub async fn me(a: Auth) -> Json<Value> {
    Json(json!({"name": a.user, "full_name": a.doc.get("full_name"), "roles": a.roles, "session_id": a.session_id}))
}

// ------------------------------------------------------------------------------------------------ projects

fn lease_json(l: &leases::Lease) -> Value {
    json!({"lease_id": l.lease_id, "fencing_token": l.fencing_token, "expires_ms": l.expires_ms, "revision": l.revision,
           "user": l.user, "client": l.client, "acquired_ms": l.acquired_ms})
}

pub async fn projects_list(State(st): State<Shared>, a: Auth) -> R<Json<Value>> {
    a.need(ROLE_VIEWER)?;
    blocking(move || {
        let listed = {
            let store = st.legacy.security_store();
            crate::legacy::with_access(&store, &a.doc, |acc| st.library.list(Some(acc))).map_err(|e| ApiError::forbidden(lerr(&e)))?
        };
        let c = st.db();
        let now = now_ms();
        let mut out = Vec::new();
        for p in listed.as_array().cloned().unwrap_or_default() {
            let name = p.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
            let rev = leases::revision(&c, &name)?;
            let holder = leases::active(&c, &name, now)?;
            let mut m = p.as_object().cloned().unwrap_or_default();
            m.insert("revision".into(), json!(rev));
            m.insert("lease".into(), holder.map(|h| json!({"user": h.user, "client": h.client, "expires_ms": h.expires_ms})).unwrap_or(Value::Null));
            out.push(Value::Object(m));
        }
        Ok(Json(json!({"projects": out})))
    })
    .await
}

pub async fn project_get(State(st): State<Shared>, a: Auth, Path(name): Path<String>) -> R<Json<Value>> {
    a.need(ROLE_VIEWER)?;
    blocking(move || {
        // Revision read first: if a write lands in between, the client sees the older revision and its next write
        // conflicts (safe direction).
        let (rev, holder) = {
            let c = st.db();
            (leases::revision(&c, &name)?, leases::active(&c, &name, now_ms())?)
        };
        let project = {
            let store = st.legacy.security_store();
            store.require(Some(&a.doc), &["READ_PROJECT"]).map_err(|e| ApiError::forbidden(lerr(&e)))?;
            crate::legacy::with_access(&store, &a.doc, |acc| st.library.open(&json!(name), Some(acc))).map_err(|e| lib_err(&e))?
        };
        Ok(Json(json!({"name": name, "revision": rev, "lease": holder.map(|h| json!({"user": h.user, "client": h.client, "expires_ms": h.expires_ms})), "project": project})))
    })
    .await
}

fn lib_err(e: &opentrace_core::idx_projects::LErr) -> ApiError {
    use opentrace_core::idx_projects::LErr::*;
    match e {
        Value(m) if m.contains("No such project") || m.contains("not found") => ApiError::not_found(m.clone()),
        Value(m) => ApiError::bad(m.clone()),
        Denied(m) => ApiError::forbidden(m.clone()),
        Other(m) => ApiError::internal(m.clone()),
    }
}

#[derive(Deserialize, Default)]
pub struct LeaseReq {
    lease_id: Option<String>,
    reason: Option<String>,
}

pub async fn lease_acquire(State(st): State<Shared>, a: Auth, Path(name): Path<String>) -> R<Json<Value>> {
    a.need(ROLE_EXPERT)?;
    check_project_name(&name)?;
    blocking(move || {
        let ttl = st.cfg.lease_ttl_seconds as i64 * 1000;
        let mut c = st.db();
        let l = leases::acquire(&mut c, &name, &a.user, &a.session_id, a.client.as_deref(), ttl)?;
        Ok(Json(lease_json(&l)))
    })
    .await
}

pub async fn lease_renew(State(st): State<Shared>, a: Auth, Path(name): Path<String>, Json(req): Json<LeaseReq>) -> R<Json<Value>> {
    let lease_id = req.lease_id.ok_or_else(|| ApiError::bad("lease_id required"))?;
    blocking(move || {
        let ttl = st.cfg.lease_ttl_seconds as i64 * 1000;
        let mut c = st.db();
        let l = leases::renew(&mut c, &name, &lease_id, &a.session_id, ttl)?;
        Ok(Json(lease_json(&l)))
    })
    .await
}

pub async fn lease_release(State(st): State<Shared>, a: Auth, Path(name): Path<String>, headers: HeaderMap) -> R<Json<Value>> {
    let lease_id = header(&headers, "x-lease-id").ok_or_else(|| ApiError::bad("X-Lease-Id header required"))?;
    blocking(move || {
        let mut c = st.db();
        leases::release(&mut c, &name, &lease_id, &a.session_id)?;
        Ok(Json(json!({"ok": true})))
    })
    .await
}

pub async fn lease_force_release(State(st): State<Shared>, a: Auth, Path(name): Path<String>, Json(req): Json<LeaseReq>) -> R<Json<Value>> {
    a.need(ROLE_ADMIN)?;
    let reason = req.reason.filter(|r| !r.trim().is_empty()).ok_or_else(|| ApiError::bad("reason required (recorded in the audit trail)"))?;
    blocking(move || {
        let mut c = st.db();
        let h = leases::force_release(&mut c, &name, &a.user, &reason)?;
        Ok(Json(json!({"released": h.map(|h| json!({"user": h.user, "client": h.client}))})))
    })
    .await
}

fn header(h: &HeaderMap, name: &str) -> Option<String> {
    h.get(name).and_then(|v| v.to_str().ok()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Lease id, fencing token and expected revision of a mutating request.
struct WriteGuard {
    lease_id: String,
    fencing: i64,
    expected_revision: i64,
}

fn write_guard(h: &HeaderMap) -> R<WriteGuard> {
    let lease_id = header(h, "x-lease-id").ok_or_else(|| ApiError::new(StatusCode::PRECONDITION_REQUIRED, "lease_required", "X-Lease-Id header required: acquire the project lease first"))?;
    let fencing = header(h, "x-fencing-token").and_then(|v| v.parse().ok()).ok_or_else(|| ApiError::new(StatusCode::PRECONDITION_REQUIRED, "lease_required", "X-Fencing-Token header required"))?;
    let rev = header(h, "x-expected-revision")
        .or_else(|| header(h, "if-match").map(|v| v.trim_matches('"').to_string()))
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| ApiError::new(StatusCode::PRECONDITION_REQUIRED, "revision_required", "X-Expected-Revision (or If-Match) header required"))?;
    Ok(WriteGuard { lease_id, fencing, expected_revision: rev })
}

fn check_project_name(name: &str) -> R<()> {
    let ok = !name.trim().is_empty() && name.chars().count() <= 120 && !name.chars().any(|c| (c as u32) < 0x20 || "<>:\"/\\|?*".contains(c));
    if ok {
        Ok(())
    } else {
        Err(ApiError::bad("Project name: 1..120 characters without control characters or < > : \" / \\ | ? *"))
    }
}

/// Project library actions through API v1 (same document format as the web UI). Reads need `viewer`; `save_meta`
/// and `save_sample` need `expert`, the lease, the fencing token and the expected revision. `save_sample` may pass
/// `fsa_sha256` of a file already uploaded to `/api/v1/fsa/{sha256}` instead of inline base64.
pub async fn project_idx(State(st): State<Shared>, a: Auth, Path(name): Path<String>, headers: HeaderMap, Json(req): Json<Value>) -> R<Response> {
    if req.get("action").and_then(|v| v.as_str()) == Some("open_sample") {
        return project_open_sample(st, a, name, req).await;
    }
    project_idx_json(st, a, name, headers, req).await.map(|j| j.into_response())
}

/// `open_sample`, composed as text: `{"id":…,"fsa_sha256":…,"fsa_b64":…,"data":<stored JSON>}` — the stored sample
/// (several MB for a real run) is not parsed and re-serialised. With `fsa_by_sha` and the original in the FSA store,
/// only its SHA-256 is sent (the desktop fills it from its cache); otherwise base64 as before.
async fn project_open_sample(st: Shared, a: Auth, name: String, req: Value) -> R<Response> {
    check_project_name(&name)?;
    a.need(ROLE_VIEWER)?;
    let by_sha = req.get("fsa_by_sha") == Some(&json!(true));
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    blocking(move || {
        let store = st.legacy.security_store();
        store.require(Some(&a.doc), &["READ_PROJECT"]).map_err(|e| ApiError::forbidden(lerr(&e)))?;
        let (fsa_bytes, data) = crate::legacy::with_access(&store, &a.doc, |acc| st.library.open_sample_raw(&json!(name), &id, Some(acc))).map_err(|e| lib_err(&e))?;
        drop(store);
        let mut sha = Value::Null;
        let mut b64 = Value::Null;
        if let Some(bytes) = fsa_bytes {
            let h = db::sha256_hex(&bytes);
            if by_sha && fsa::blob_path(&st.cfg.fsa_dir(), &h).map(|p| p.is_file()).unwrap_or(false) {
                sha = json!(h);
            } else {
                b64 = json!(opentrace_core::matrix::b64encode(&bytes));
            }
        }
        let data = data.unwrap_or_else(|| b"null".to_vec());
        let mut out = Vec::with_capacity(data.len() + 256);
        out.extend_from_slice(format!("{{\"id\":{},\"fsa_sha256\":{},\"fsa_b64\":{},\"data\":", id, sha, b64).as_bytes());
        out.extend_from_slice(&data);
        out.push(b'}');
        Ok(([(axum::http::header::CONTENT_TYPE, "application/json; charset=utf-8")], out).into_response())
    })
    .await
}

async fn project_idx_json(st: Shared, a: Auth, name: String, headers: HeaderMap, mut req: Value) -> R<Json<Value>> {
    check_project_name(&name)?;
    let obj = req.as_object_mut().ok_or_else(|| ApiError::bad("request must be an object"))?;
    let action = obj.get("action").and_then(|a| a.as_str()).unwrap_or("").to_string();
    obj.insert("name".into(), json!(name));
    obj.remove("new_name");
    match action.as_str() {
        "open" | "open_sample" | "export" => {
            a.need(ROLE_VIEWER)?;
            // `fsa_by_sha`: the client (desktop) fetches the original by SHA-256 — usually from its own cache — instead
            // of receiving it again as base64 inside the JSON. Only for files the FSA store has.
            let by_sha = obj.remove("fsa_by_sha").map(|v| v == json!(true)).unwrap_or(false) && action == "open_sample";
            blocking(move || {
                let store = st.legacy.security_store();
                store.require(Some(&a.doc), &["READ_PROJECT"]).map_err(|e| ApiError::forbidden(lerr(&e)))?;
                let user = a.user.clone();
                let mut r = crate::legacy::with_access(&store, &a.doc, |acc| st.library.request(&req, Some(acc), Some(&user))).map_err(|e| lib_err(&e))?;
                drop(store);
                if by_sha {
                    if let Some(b64) = r.get("fsa_b64").and_then(|v| v.as_str()) {
                        if let Ok(bytes) = opentrace_core::matrix::b64decode_strict(b64) {
                            let sha = db::sha256_hex(&bytes);
                            if fsa::blob_path(&st.cfg.fsa_dir(), &sha).map(|p| p.is_file()).unwrap_or(false) {
                                r["fsa_b64"] = Value::Null;
                                r["fsa_sha256"] = json!(sha);
                            }
                        }
                    }
                }
                Ok(Json(r))
            })
            .await
        }
        "save_meta" | "save_sample" => {
            a.need(ROLE_EXPERT)?;
            let g = write_guard(&headers)?;
            let mut link_fsa: Option<(std::path::PathBuf, Value)> = None;
            if action == "save_sample" {
                if let Some(sha) = obj.get("fsa_sha256").and_then(|s| s.as_str()).map(|s| s.to_string()) {
                    let path = fsa::blob_path(&st.cfg.fsa_dir(), &sha).ok_or_else(|| ApiError::bad("fsa_sha256 must be 64 lowercase hex digits"))?;
                    let bytes = tokio::fs::read(&path).await.map_err(|_| ApiError::conflict("fsa_missing", "FSA not on the server: upload it first (PUT /api/v1/fsa/{sha256})"))?;
                    obj.insert("fsa_b64".into(), json!(opentrace_core::matrix::b64encode(&bytes)));
                    obj.remove("fsa_sha256");
                    link_fsa = Some((path, obj.get("id").cloned().unwrap_or(Value::Null)));
                }
            }
            blocking(move || {
                let mut c = st.db();
                let user = a.user.clone();
                let (r, rev) = leases::guarded_write(&mut c, &name, &g.lease_id, &a.session_id, g.fencing, g.expected_revision, &a.user, |_tx| {
                    let store = st.legacy.security_store();
                    store.require(Some(&a.doc), &["UPDATE_PROJECT"]).map_err(|e| lerr(&e))?;
                    crate::legacy::with_access(&store, &a.doc, |acc| st.library.request(&req, Some(acc), Some(&user))).map_err(|e| lerr(&e))
                })?;
                // The project's copy of the FSA becomes a hard link to the stored original (same bytes, stored once).
                if let Some((blob, id)) = link_fsa {
                    if let Ok(p) = st.library.sample_fsa_path(&json!(name), &id) {
                        let _ = crate::fsa::link_copy(&blob, &p);
                    }
                }
                Ok(Json(json!({"result": r, "revision": rev})))
            })
            .await
        }
        _ => Err(ApiError::bad("action must be open, open_sample, export, save_meta or save_sample (delete/rename/import stay in the web Project Manager)")),
    }
}

#[derive(Deserialize)]
pub struct LinkFsa {
    sha256: String,
    filename: String,
}

pub async fn project_fsa_link(State(st): State<Shared>, a: Auth, Path(name): Path<String>, headers: HeaderMap, Json(req): Json<LinkFsa>) -> R<Json<Value>> {
    a.need(ROLE_EXPERT)?;
    let g = write_guard(&headers)?;
    if !db::is_sha256_hex(&req.sha256) {
        return Err(ApiError::bad("sha256 must be 64 lowercase hex digits"));
    }
    let filename: String = req.filename.chars().filter(|c| !c.is_control()).take(255).collect();
    blocking(move || {
        let mut c = st.db();
        let exists: Option<i64> = db::get_opt(&c, "SELECT size FROM fsa_blobs WHERE sha256 = ?1", params![req.sha256]).map_err(ApiError::internal)?;
        if exists.is_none() {
            return Err(ApiError::conflict("fsa_missing", "FSA not on the server: upload it first"));
        }
        let user = a.user.clone();
        let (_, rev) = leases::guarded_write(&mut c, &name, &g.lease_id, &a.session_id, g.fencing, g.expected_revision, &a.user, |tx| {
            tx.execute(
                "INSERT OR IGNORE INTO project_fsa (project, sha256, filename, added_ms, added_by) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![name, req.sha256, filename, now_ms(), user],
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        })?;
        Ok(Json(json!({"ok": true, "revision": rev})))
    })
    .await
}

pub async fn project_fsa_list(State(st): State<Shared>, a: Auth, Path(name): Path<String>) -> R<Json<Value>> {
    a.need(ROLE_VIEWER)?;
    blocking(move || {
        let c = st.db();
        let mut s = c
            .prepare("SELECT p.sha256, p.filename, p.added_ms, p.added_by, b.size FROM project_fsa p JOIN fsa_blobs b ON b.sha256 = p.sha256 WHERE p.project = ?1 ORDER BY p.added_ms, p.filename")
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let rows = s
            .query_map(params![name], |r| Ok(json!({"sha256": r.get::<_, String>(0)?, "filename": r.get::<_, String>(1)?, "added_ms": r.get::<_, i64>(2)?, "added_by": r.get::<_, String>(3)?, "size": r.get::<_, i64>(4)?})))
            .map_err(|e| ApiError::internal(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| ApiError::internal(e.to_string()))?;
        Ok(Json(json!({"files": rows})))
    })
    .await
}

#[derive(Deserialize)]
pub struct AnalysisReq {
    sample_id: Option<String>,
    fsa_sha256: String,
    analysis_version: String,
    settings: Value,
    result_sha256: Option<String>,
    lab_profile: Option<ProfileRef>,
}
#[derive(Deserialize)]
pub struct ProfileRef {
    name: String,
    version: i64,
}

/// Provenance of an analysis run: exact settings (hashed), algorithm version, source FSA hash and, when used, the
/// approved laboratory profile version with its content hash.
pub async fn analyses_add(State(st): State<Shared>, a: Auth, Path(name): Path<String>, headers: HeaderMap, Json(req): Json<AnalysisReq>) -> R<Json<Value>> {
    a.need(ROLE_EXPERT)?;
    let g = write_guard(&headers)?;
    if !db::is_sha256_hex(&req.fsa_sha256) || req.result_sha256.as_deref().map(|s| !db::is_sha256_hex(s)).unwrap_or(false) {
        return Err(ApiError::bad("hashes must be 64 lowercase hex digits"));
    }
    if !req.settings.is_object() {
        return Err(ApiError::bad("settings must be an object"));
    }
    blocking(move || {
        let mut c = st.db();
        if db::get_opt::<i64>(&c, "SELECT size FROM fsa_blobs WHERE sha256 = ?1", params![req.fsa_sha256]).map_err(ApiError::internal)?.is_none() {
            return Err(ApiError::conflict("fsa_missing", "source FSA is not on the server"));
        }
        let profile = match &req.lab_profile {
            Some(p) => {
                let v = standards::get(&c, "lab_profile", &p.name, p.version)?;
                if v["status"] != "approved" {
                    return Err(ApiError::conflict("profile_not_approved", format!("lab profile {} v{} is {}", p.name, p.version, v["status"].as_str().unwrap_or(""))));
                }
                Some((p.name.clone(), p.version, v["content_sha256"].as_str().unwrap_or("").to_string()))
            }
            None => None,
        };
        let settings_json = standards::canonical(&req.settings);
        let settings_sha = db::sha256_hex(settings_json.as_bytes());
        let user = a.user.clone();
        let (id, rev) = leases::guarded_write(&mut c, &name, &g.lease_id, &a.session_id, g.fencing, g.expected_revision, &a.user, |tx| {
            let base_rev = leases::revision(tx, &name).map_err(|e| format!("{:?}", e))?;
            tx.execute(
                "INSERT INTO analyses (project, sample_id, fsa_sha256, analysis_version, settings_json, settings_sha256, lab_profile, lab_profile_version,
                                       lab_profile_sha256, result_sha256, project_revision, created_ms, created_by)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![name, req.sample_id, req.fsa_sha256, req.analysis_version, settings_json, settings_sha,
                        profile.as_ref().map(|p| p.0.clone()), profile.as_ref().map(|p| p.1), profile.as_ref().map(|p| p.2.clone()),
                        req.result_sha256, base_rev + 1, now_ms(), user],
            )
            .map_err(|e| e.to_string())?;
            Ok(tx.last_insert_rowid())
        })?;
        Ok(Json(json!({"id": id, "settings_sha256": settings_sha, "revision": rev})))
    })
    .await
}

pub async fn analyses_list(State(st): State<Shared>, a: Auth, Path(name): Path<String>) -> R<Json<Value>> {
    a.need(ROLE_VIEWER)?;
    blocking(move || {
        let c = st.db();
        let mut s = c
            .prepare("SELECT id, sample_id, fsa_sha256, analysis_version, settings_sha256, lab_profile, lab_profile_version, lab_profile_sha256, result_sha256, project_revision, created_ms, created_by, settings_json FROM analyses WHERE project = ?1 ORDER BY id")
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let rows = s
            .query_map(params![name], |r| {
                Ok(json!({"id": r.get::<_, i64>(0)?, "sample_id": r.get::<_, Option<String>>(1)?, "fsa_sha256": r.get::<_, String>(2)?,
                    "analysis_version": r.get::<_, String>(3)?, "settings_sha256": r.get::<_, String>(4)?, "lab_profile": r.get::<_, Option<String>>(5)?,
                    "lab_profile_version": r.get::<_, Option<i64>>(6)?, "lab_profile_sha256": r.get::<_, Option<String>>(7)?,
                    "result_sha256": r.get::<_, Option<String>>(8)?, "project_revision": r.get::<_, i64>(9)?, "created_ms": r.get::<_, i64>(10)?,
                    "created_by": r.get::<_, String>(11)?, "settings": serde_json::from_str::<Value>(&r.get::<_, String>(12)?).unwrap_or(Value::Null)}))
            })
            .map_err(|e| ApiError::internal(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| ApiError::internal(e.to_string()))?;
        Ok(Json(json!({"analyses": rows})))
    })
    .await
}

// ------------------------------------------------------------------------------------------------ FSA store

pub async fn fsa_head(State(st): State<Shared>, a: Auth, Path(sha): Path<String>) -> R<Response> {
    a.need(ROLE_VIEWER)?;
    let p = fsa::blob_path(&st.cfg.fsa_dir(), &sha).ok_or_else(|| ApiError::bad("bad sha256"))?;
    match tokio::fs::metadata(&p).await {
        Ok(m) => Ok((StatusCode::OK, [(axum::http::header::CONTENT_LENGTH, m.len().to_string())]).into_response()),
        Err(_) => Ok(StatusCode::NOT_FOUND.into_response()),
    }
}

pub async fn fsa_get(State(st): State<Shared>, a: Auth, Path(sha): Path<String>) -> R<Response> {
    a.need(ROLE_VIEWER)?;
    let p = fsa::blob_path(&st.cfg.fsa_dir(), &sha).ok_or_else(|| ApiError::bad("bad sha256"))?;
    let f = tokio::fs::File::open(&p).await.map_err(|_| ApiError::not_found("no such FSA"))?;
    let len = f.metadata().await.map(|m| m.len()).unwrap_or(0);
    let stream = tokio_util_stream(f);
    Ok((
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/octet-stream".to_string()), (axum::http::header::CONTENT_LENGTH, len.to_string())],
        Body::from_stream(stream),
    )
        .into_response())
}

/// File -> byte stream in 64 KiB chunks (memory bounded download).
fn tokio_util_stream(f: tokio::fs::File) -> impl futures_util::Stream<Item = Result<bytes::Bytes, std::io::Error>> {
    futures_util::stream::unfold(f, |mut f| async move {
        use tokio::io::AsyncReadExt;
        let mut buf = vec![0u8; 64 * 1024];
        match f.read(&mut buf).await {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                Some((Ok(bytes::Bytes::from(buf)), f))
            }
            Err(e) => Some((Err(e), f)),
        }
    })
}

pub async fn fsa_put(State(st): State<Shared>, a: Auth, Path(sha): Path<String>, body: Body) -> R<Response> {
    a.need(ROLE_EXPERT)?;
    let out = fsa::put_stream(&st.cfg.fsa_dir(), &st.cfg.tmp_dir(), &sha, st.cfg.max_fsa_bytes(), body.into_data_stream()).await;
    let (status, size, created) = match out {
        Ok(PutOutcome::Created(n)) => (StatusCode::CREATED, n, true),
        Ok(PutOutcome::Exists(n)) => (StatusCode::OK, n, false),
        Err(PutError::BadName) => return Err(ApiError::bad("sha256 must be 64 lowercase hex digits")),
        Err(PutError::TooLarge(m)) => return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "too_large", format!("FSA larger than {} bytes", m))),
        Err(PutError::HashMismatch { expected, actual }) => {
            return Err(ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "hash_mismatch", "received bytes do not match the SHA-256 in the URL").with(json!({"expected": expected, "actual": actual})))
        }
        Err(PutError::Empty) => return Err(ApiError::bad("empty body")),
        Err(PutError::NotAbif) => return Err(ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "not_abif", "not an ABIF (.fsa/.ab1) file")),
        Err(PutError::Client(m)) => return Err(ApiError::bad(format!("upload interrupted: {}", m))),
        Err(PutError::Io(m)) => return Err(ApiError::internal(m)),
    };
    let user = a.user.clone();
    let sha2 = sha.clone();
    blocking(move || {
        let c = st.db();
        c.execute("INSERT OR IGNORE INTO fsa_blobs (sha256, size, stored_ms, stored_by) VALUES (?1, ?2, ?3, ?4)", params![sha2, size as i64, now_ms(), user])
            .map_err(|e| ApiError::internal(e.to_string()))?;
        if created {
            let _ = db::audit(&c, Some(&user), "fsa.store", "fsa", Some(&sha2), &json!({"size": size}));
        }
        Ok(())
    })
    .await?;
    Ok((status, Json(json!({"sha256": sha, "size": size, "status": if created { "created" } else { "exists" }}))).into_response())
}

// ------------------------------------------------------------------------------------------------ standards

#[derive(Deserialize)]
pub struct StdQuery {
    kind: Option<String>,
    status: Option<String>,
}

pub async fn standards_list(State(st): State<Shared>, a: Auth, Query(q): Query<StdQuery>) -> R<Json<Value>> {
    a.need(ROLE_VIEWER)?;
    blocking(move || {
        let c = st.db();
        Ok(Json(json!({"standards": standards::list(&c, q.kind.as_deref(), q.status.as_deref())?})))
    })
    .await
}

pub async fn lab_profiles(State(st): State<Shared>, a: Auth) -> R<Json<Value>> {
    a.need(ROLE_VIEWER)?;
    blocking(move || {
        let c = st.db();
        Ok(Json(json!({"lab_profiles": standards::list(&c, Some("lab_profile"), Some("approved"))?})))
    })
    .await
}

pub async fn standard_get(State(st): State<Shared>, a: Auth, Path((kind, name, version)): Path<(String, String, i64)>) -> R<Json<Value>> {
    a.need(ROLE_VIEWER)?;
    blocking(move || {
        let c = st.db();
        Ok(Json(standards::get(&c, &kind, &name, version)?))
    })
    .await
}

#[derive(Deserialize)]
pub struct DraftReq {
    content: Value,
    note: Option<String>,
}

pub async fn standard_create(State(st): State<Shared>, a: Auth, Path((kind, name)): Path<(String, String)>, Json(req): Json<DraftReq>) -> R<Json<Value>> {
    a.need(ROLE_METHOD_AUTHOR)?;
    blocking(move || {
        let mut c = st.db();
        Ok(Json(standards::create_draft(&mut c, &kind, &name, &req.content, &a.user, req.note.as_deref())?))
    })
    .await
}

pub async fn standard_update(State(st): State<Shared>, a: Auth, Path((kind, name, version)): Path<(String, String, i64)>, Json(req): Json<DraftReq>) -> R<Json<Value>> {
    a.need(ROLE_METHOD_AUTHOR)?;
    blocking(move || {
        let mut c = st.db();
        Ok(Json(standards::update_draft(&mut c, &kind, &name, version, &req.content, &a.user)?))
    })
    .await
}

#[derive(Deserialize, Default)]
pub struct ActionReq {
    reason: Option<String>,
}

pub async fn standard_action(
    State(st): State<Shared>,
    a: Auth,
    Path((kind, name, version, action)): Path<(String, String, i64, String)>,
    body: Option<Json<ActionReq>>,
) -> R<Json<Value>> {
    let reason = body.and_then(|b| b.0.reason).unwrap_or_default();
    match action.as_str() {
        "submit" => a.need(ROLE_METHOD_AUTHOR)?,
        "approve" | "reject" => a.need(ROLE_APPROVER)?,
        "deprecate" | "archive" => {
            if !(a.has(ROLE_APPROVER) || a.has(ROLE_ADMIN)) {
                return Err(ApiError::forbidden("approver or admin role required"));
            }
        }
        _ => return Err(ApiError::not_found("action must be submit, approve, reject, deprecate or archive")),
    }
    blocking(move || {
        let mut c = st.db();
        let v = match action.as_str() {
            "submit" => standards::submit(&mut c, &kind, &name, version, &a.user)?,
            "approve" => standards::approve(&mut c, &kind, &name, version, &a.user)?,
            "reject" => {
                if reason.trim().is_empty() {
                    return Err(ApiError::bad("reason required"));
                }
                standards::reject(&mut c, &kind, &name, version, &a.user, &reason)?
            }
            "deprecate" => standards::set_retired(&mut c, &kind, &name, version, &a.user, "deprecated")?,
            _ => standards::set_retired(&mut c, &kind, &name, version, &a.user, "archived")?,
        };
        Ok(Json(v))
    })
    .await
}

// ------------------------------------------------------------------------------------------------ audit

#[derive(Deserialize)]
pub struct AuditQuery {
    limit: Option<i64>,
}

pub async fn audit_list(State(st): State<Shared>, a: Auth, Query(q): Query<AuditQuery>) -> R<Json<Value>> {
    a.need(ROLE_ADMIN)?;
    blocking(move || {
        let c = st.db();
        let limit = q.limit.unwrap_or(200).clamp(1, 5000);
        let mut s = c.prepare("SELECT id, at_ms, user_name, action, object_type, object_id, detail FROM audit ORDER BY id DESC LIMIT ?1").map_err(|e| ApiError::internal(e.to_string()))?;
        let rows = s
            .query_map(params![limit], |r| {
                Ok(json!({"id": r.get::<_, i64>(0)?, "at_ms": r.get::<_, i64>(1)?, "user": r.get::<_, Option<String>>(2)?, "action": r.get::<_, String>(3)?,
                    "object_type": r.get::<_, String>(4)?, "object_id": r.get::<_, Option<String>>(5)?,
                    "detail": serde_json::from_str::<Value>(&r.get::<_, Option<String>>(6)?.unwrap_or_default()).unwrap_or(Value::Null)}))
            })
            .map_err(|e| ApiError::internal(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| ApiError::internal(e.to_string()))?;
        Ok(Json(json!({"audit": rows})))
    })
    .await
}

pub async fn not_found() -> ApiError {
    ApiError::not_found("no such API v1 endpoint")
}
