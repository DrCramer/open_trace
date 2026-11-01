//! API v1 authentication: password login against the shared security store (same users, lockout policy and audit
//! trail as the web UI), opaque bearer tokens stored only as SHA-256 in SQLite, and laboratory roles derived from the
//! store's rights so that there is a single source of truth for who may do what.
use crate::db::{now_ms, random_token, sha256_hex};
use crate::error::ApiError;
use crate::state::Shared;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use opentrace_core::security::SecurityStore;
use rusqlite::params;
use serde_json::{json, Value};
use std::net::IpAddr;

/// User group whose members may approve laboratory standards (create it in the Security Manager).
pub const APPROVER_GROUP: &str = "OpenTrace Approvers";

pub const ROLE_ADMIN: &str = "admin";
pub const ROLE_APPROVER: &str = "approver";
pub const ROLE_METHOD_AUTHOR: &str = "method_author";
pub const ROLE_EXPERT: &str = "expert";
pub const ROLE_VIEWER: &str = "viewer";

pub fn roles(store: &SecurityStore, user: &Value) -> Vec<&'static str> {
    let can = |f: &str| store.can(Some(user), f).unwrap_or(false);
    let mut r = Vec::new();
    if can("AdminToolAcc/Users/Modify") {
        r.push(ROLE_ADMIN);
    }
    if store.groups_of(user).iter().any(|g| g == APPROVER_GROUP) {
        r.push(ROLE_APPROVER);
    }
    if can("UPDATE_ANALYSIS_METHOD") || can("UPDATE_PANEL") || can("UPDATE_SIZE_STANDARD") || can("UPDATE_MATRIX") {
        r.push(ROLE_METHOD_AUTHOR);
    }
    if can("UPDATE_PROJECT") {
        r.push(ROLE_EXPERT);
    }
    if can("READ_PROJECT") {
        r.push(ROLE_VIEWER);
    }
    r
}

/// Authenticated caller of an API v1 endpoint.
#[derive(Clone, Debug)]
pub struct Auth {
    pub user: String,
    pub session_id: String,
    pub client: Option<String>,
    pub roles: Vec<&'static str>,
    pub doc: Value,
}

impl Auth {
    pub fn has(&self, role: &str) -> bool {
        self.roles.contains(&role)
    }
    pub fn need(&self, role: &str) -> Result<(), ApiError> {
        if self.has(role) {
            Ok(())
        } else {
            Err(ApiError::forbidden(format!("role {} required", role)))
        }
    }
}

fn bearer(parts: &Parts) -> Option<String> {
    let h = parts.headers.get(axum::http::header::AUTHORIZATION)?.to_str().ok()?;
    let t = h.strip_prefix("Bearer ").or_else(|| h.strip_prefix("bearer "))?.trim();
    if t.is_empty() || t.len() > 200 {
        None
    } else {
        Some(t.to_string())
    }
}

impl FromRequestParts<Shared> for Auth {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, st: &Shared) -> Result<Self, Self::Rejection> {
        let token = bearer(parts).ok_or_else(|| ApiError::unauthorized("missing bearer token"))?;
        let st = st.clone();
        tokio::task::spawn_blocking(move || {
            let hash = sha256_hex(token.as_bytes());
            let row: Option<(String, String, Option<String>)> = {
                let db = st.db();
                crate::db::get_row3(&db, "SELECT session_id, user_name, client FROM sessions WHERE token_sha256 = ?1 AND revoked = 0 AND expires_ms > ?2", params![hash, now_ms()])
                    .map_err(ApiError::internal)?
            };
            let (session_id, user, client) = row.ok_or_else(|| ApiError::unauthorized("session expired or revoked; log in again"))?;
            let store = st.legacy.security_store();
            let doc = store.user(&json!(user)).ok_or_else(|| ApiError::unauthorized("user no longer exists"))?;
            if doc.get("disabled").and_then(|d| d.as_bool()).unwrap_or(false) {
                return Err(ApiError::unauthorized("user is disabled"));
            }
            let roles = roles(&store, &doc);
            Ok(Auth { user, session_id, client, roles, doc })
        })
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
    }
}

/// Sliding-window login limiter per client address (the account lockout policy still applies on top).
pub fn allow_login(st: &Shared, ip: IpAddr) -> bool {
    let now = now_ms();
    let mut m = st.login_attempts.lock().unwrap_or_else(|e| e.into_inner());
    m.retain(|_, v| {
        v.retain(|t| now - *t < 60_000);
        !v.is_empty()
    });
    let v = m.entry(ip).or_default();
    if v.len() as u32 >= st.cfg.login_attempts_per_minute {
        return false;
    }
    v.push(now);
    true
}

pub struct NewSession {
    pub token: String,
    pub session_id: String,
    pub expires_ms: i64,
}

pub fn create_session(st: &Shared, user: &str, client: Option<&str>) -> Result<NewSession, String> {
    let token = random_token(32);
    let session_id = random_token(16);
    let now = now_ms();
    let expires_ms = now + st.cfg.session_ttl_hours as i64 * 3_600_000;
    let db = st.db();
    db.execute(
        "INSERT INTO sessions (token_sha256, session_id, user_name, client, created_ms, expires_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![sha256_hex(token.as_bytes()), session_id, user, client, now, expires_ms],
    )
    .map_err(|e| e.to_string())?;
    db.execute("DELETE FROM sessions WHERE expires_ms < ?1 OR revoked = 1", params![now - 86_400_000]).map_err(|e| e.to_string())?;
    Ok(NewSession { token, session_id, expires_ms })
}
