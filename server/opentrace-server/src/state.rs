//! Shared server state.
use crate::config::ServerConfig;
use opentrace_core::idx_projects::ProjectLibrary;
use opentrace_core::server::App;
use rusqlite::Connection;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, MutexGuard};

pub struct AppState {
    pub cfg: ServerConfig,
    /// Single connection; every use happens on a blocking thread. Lock order: `db` before the security store.
    pub db_conn: Mutex<Connection>,
    /// The legacy web application (also owner of the shared security store).
    pub legacy: Arc<App>,
    /// Same directory as the legacy application's project library.
    pub library: ProjectLibrary,
    pub login_attempts: Mutex<HashMap<IpAddr, Vec<i64>>>,
    /// Keeps the single-instance lock of the data directory for the lifetime of the server.
    pub _instance_lock: Option<std::fs::File>,
}

pub type Shared = Arc<AppState>;

impl AppState {
    pub fn db(&self) -> MutexGuard<'_, Connection> {
        self.db_conn.lock().unwrap_or_else(|e| e.into_inner())
    }
}
