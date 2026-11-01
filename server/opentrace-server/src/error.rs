//! API error type: JSON body `{"error": "...", "code": "..."}` plus optional details.
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    pub details: Option<Value>,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        ApiError { status, code, message: message.into(), details: None }
    }
    pub fn with(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }
    pub fn bad(m: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "bad_request", m)
    }
    pub fn unauthorized(m: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized", m)
    }
    pub fn forbidden(m: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "forbidden", m)
    }
    pub fn not_found(m: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", m)
    }
    pub fn conflict(code: &'static str, m: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, code, m)
    }
    pub fn internal(m: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", m)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut body = json!({"error": self.message, "code": self.code});
        if let (Some(Value::Object(d)), Some(m)) = (self.details, body.as_object_mut()) {
            for (k, v) in d {
                m.insert(k, v);
            }
        }
        (self.status, axum::Json(body)).into_response()
    }
}

impl From<crate::leases::LeaseError> for ApiError {
    fn from(e: crate::leases::LeaseError) -> Self {
        use crate::leases::LeaseError::*;
        match e {
            Held(h) => ApiError::conflict("project_locked", format!("Проект редактирует {}{} — откройте его только для чтения. / Project is being edited by {}.",
                h.user, h.client.as_deref().map(|c| format!(" ({})", c)).unwrap_or_default(), h.user))
                .with(json!({"locked_by": h.user, "client": h.client, "lease_expires_ms": h.expires_ms, "read_only_available": true})),
            Lost(m) => ApiError::conflict("lease_lost", format!("Lease lost: {}", m)),
            Revision { expected, actual } => ApiError::conflict("revision_conflict", format!("Project changed on the server (revision {} expected, current {})", expected, actual))
                .with(json!({"expected_revision": expected, "current_revision": actual})),
            Db(m) => ApiError::internal(m),
        }
    }
}

impl From<crate::standards::StdError> for ApiError {
    fn from(e: crate::standards::StdError) -> Self {
        use crate::standards::StdError::*;
        match e {
            Invalid(m) => ApiError::bad(m),
            NotFound => ApiError::not_found("no such standard version"),
            Conflict(m) => ApiError::conflict("standard_state", m),
            Forbidden(m) => ApiError::forbidden(m),
            Db(m) => ApiError::internal(m),
        }
    }
}
