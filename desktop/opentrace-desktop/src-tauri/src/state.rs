//! Process-wide desktop state (managed by Tauri).
use crate::config::DesktopConfig;
use crate::pool::Pool;
use crate::remote::Remote;
use crate::selftest::SelfTest;
use crate::sync::SyncQueue;
use opentrace_core::server::App;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};

pub struct DesktopState {
    /// LocalAnalysisService: the analysis core and the local project library (no HTTP round trip).
    pub core: Arc<App>,
    pub pool: Pool,
    pub cfg: Mutex<DesktopConfig>,
    pub cfg_path: PathBuf,
    pub data_dir: PathBuf,
    /// RemoteOrganizationService.
    pub remote: Arc<Remote>,
    pub sync: Arc<SyncQueue>,
    /// ProjectWorkspace for server projects.
    pub workspace: Arc<crate::workspace::Workspace>,
    pub selftest: Option<Arc<SelfTest>>,
    pub close_ok: Mutex<HashSet<String>>,
    pub close_asked: Mutex<std::collections::HashMap<String, std::time::Instant>>,
    pub popup_seq: AtomicUsize,
    /// Windows whose page reached the app scheme handler (see `windows::decorate`, popup reload).
    pub scheme_seen: Mutex<HashSet<String>>,
    pub popup_reloaded: Mutex<HashSet<String>>,
}

impl DesktopState {
    pub fn config(&self) -> DesktopConfig {
        self.cfg.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn update_config(&self, f: impl FnOnce(&mut DesktopConfig)) {
        let mut g = self.cfg.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut g);
        let _ = g.save(&self.cfg_path);
    }

    pub fn server_projects(&self) -> bool {
        self.config().project_source == "server"
    }

    pub fn allow_close(&self, label: &str) {
        self.close_ok.lock().unwrap_or_else(|e| e.into_inner()).insert(label.to_string());
    }

    /// Verifies the configured laboratory profile against the server (see `labprofile.rs`) and makes it active.
    pub fn reload_lab_profile(&self) -> serde_json::Value {
        let cfg = self.config();
        let r = match &cfg.lab_profile {
            None => Ok(None),
            Some((name, version)) => crate::labprofile::resolve(&self.remote, cfg.ca_cert_pem.as_deref(), &self.core, name, *version).map(Some),
        };
        *self.workspace.profile.lock().unwrap_or_else(|e| e.into_inner()) = r;
        self.workspace.profile_status()
    }

    pub fn take_close_permission(&self, label: &str) -> bool {
        self.close_ok.lock().unwrap_or_else(|e| e.into_inner()).remove(label)
    }
}
