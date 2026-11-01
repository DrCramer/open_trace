//! OpenTrace Desktop (Tauri 2): the existing OpenTrace web UI in native windows, analysis in-process through
//! `opentrace-core`, optional organisation features through OpenTrace Server API v1.
mod bridge;
mod config;
mod pool;
mod remote;
mod selftest;
mod state;
mod sync;
mod windows;

use state::DesktopState;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{Manager, WindowEvent};

const SYNC_WORKERS: usize = 2;

fn data_dir(app: &tauri::App, selftest: bool) -> Result<PathBuf, String> {
    if let Some(d) = std::env::var_os("OPENTRACE_DESKTOP_DATA_DIR").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(d));
    }
    if selftest {
        return Ok(std::env::temp_dir().join(format!("opentrace-desktop-selftest-{}", std::process::id())));
    }
    app.path().app_local_data_dir().map_err(|e| e.to_string())
}

fn setup(app: &mut tauri::App, selftest: Option<Arc<selftest::SelfTest>>) -> Result<(), Box<dyn std::error::Error>> {
    let data = data_dir(app, selftest.is_some())?;
    std::fs::create_dir_all(&data)?;
    let cfg_path = if selftest.is_some() { data.join("desktop.json") } else { app.path().app_config_dir()?.join("desktop.json") };
    let resources = app.path().resource_dir().ok().map(|d| d.join("resources")).filter(|d| d.join("web").join("index.html").is_file());
    let core_cfg = opentrace_core::server::Config {
        port: 0,
        host: "127.0.0.1".into(),
        open_browser: false,
        projects_dir: Some(data.join("projects")),
        security_dir: Some(data.join("security")),
        allowed_hosts: vec![],
        resources,
    };
    let core = Arc::new(opentrace_core::server::App::new(&core_cfg)?);
    let cfg = config::DesktopConfig::load(&cfg_path);
    let remote = Arc::new(remote::Remote::new());
    let sync = sync::SyncQueue::open(data.join("fsa-cache"), cfg.fsa_cache_max_mib.saturating_mul(1024 * 1024));
    *sync.enabled.lock().unwrap_or_else(|e| e.into_inner()) = cfg.sync_fsa;
    sync.pause(Some("Нет входа на сервер".into()));
    let st = Arc::new(DesktopState {
        core,
        pool: pool::Pool::new(pool::default_size(), "analysis"),
        cfg: Mutex::new(cfg.clone()),
        cfg_path,
        data_dir: data,
        remote: remote.clone(),
        sync: sync.clone(),
        selftest: selftest.clone(),
        close_ok: Mutex::new(Default::default()),
        close_asked: Mutex::new(Default::default()),
        popup_seq: AtomicUsize::new(0),
    });
    app.manage(st.clone());
    let st_ca = st.clone();
    sync::start_workers(sync.clone(), remote.clone(), Arc::new(move || st_ca.config().ca_cert_pem), SYNC_WORKERS);
    // Resume a remembered server session in the background (never blocks start-up; offline is fine).
    if let (Some(url), Some(user)) = (cfg.server_url.clone(), cfg.username.clone()) {
        let (remote, sync, ca) = (remote.clone(), sync.clone(), cfg.ca_cert_pem.clone());
        std::thread::spawn(move || match remote.resume(&url, &user, ca.as_deref()) {
            Ok(_) => sync.resume(),
            Err(e) => eprintln!("[desktop] session not resumed: {}", e.to_json()["error"]),
        });
    }
    windows::create_main(app.handle())?;
    if let Some(t) = selftest {
        t.watchdog(app.handle().clone(), 120);
    }
    Ok(())
}

fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    let app = window.app_handle();
    let label = window.label().to_string();
    match event {
        WindowEvent::CloseRequested { api, .. } => {
            let Some(st) = app.try_state::<Arc<DesktopState>>() else { return };
            if st.take_close_permission(&label) {
                return;
            }
            // Ask the page (unsaved edits -> confirmation). If the page does not answer within 3 s (hung script),
            // the next close request is let through so a window can always be closed.
            let mut asked = st.close_asked.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(t) = asked.get(&label) {
                if t.elapsed() > Duration::from_secs(3) && t.elapsed() < Duration::from_secs(60) {
                    asked.remove(&label);
                    return;
                }
            }
            asked.insert(label.clone(), Instant::now());
            drop(asked);
            api.prevent_close();
            if let Some(w) = app.get_webview_window(&label) {
                let _ = w.eval("window.__otDesktopRequestClose ? window.__otDesktopRequestClose() : fetch('/desktop/api/window/close-confirmed', {method: 'POST'})");
            }
        }
        WindowEvent::Destroyed => {
            if label == "main" {
                app.exit(0);
            }
        }
        _ => {}
    }
}

pub fn run() {
    let selftest = selftest::SelfTest::from_env().map(Arc::new);
    let st_setup = selftest.clone();
    let mut state_plugin = tauri_plugin_window_state::Builder::new().map_label(windows::state_key);
    if selftest.is_some() {
        // Deterministic geometry for the self-test: do not restore or persist window state.
        state_plugin = state_plugin.with_denylist(&["main"]).with_filter(|_| false);
    }
    tauri::Builder::default()
        .plugin(state_plugin.build())
        .register_asynchronous_uri_scheme_protocol("opentrace", |ctx, req, responder| {
            let app = ctx.app_handle().clone();
            let label = ctx.webview_label().to_string();
            let Some(st) = app.try_state::<Arc<DesktopState>>().map(|s| s.inner().clone()) else {
                responder.respond(bridge::json_response(503, &serde_json::json!({"error": "starting"})));
                return;
            };
            let st2 = st.clone();
            st.pool.execute(move || {
                let r = bridge::handle(&app, &st2, &label, req);
                responder.respond(r);
            });
        })
        .setup(move |app| setup(app, st_setup.clone()))
        .on_window_event(on_window_event)
        .run(tauri::generate_context!())
        .expect("OpenTrace Desktop failed to start");
}
