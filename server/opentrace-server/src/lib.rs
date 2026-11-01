//! OpenTrace Server: one native binary for Linux and Windows. Serves API v1 (organisation features) and, unchanged,
//! the existing web UI and its `/api/*` endpoints, all on top of `opentrace-core`.
pub mod api;
pub mod auth;
pub mod backup;
pub mod config;
pub mod db;
pub mod error;
pub mod fsa;
pub mod leases;
pub mod legacy;
pub mod standards;
pub mod state;

use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use axum::Router;
use config::ServerConfig;
use state::{AppState, Shared};
use std::sync::{Arc, Mutex};

/// Opens (and migrates) everything under `cfg.data_dir`. Takes the single-instance lock unless `lock` is false
/// (tests that build several states on one directory to simulate restarts drop the previous one first).
pub fn build_state(cfg: ServerConfig, port: u16, lock: bool) -> Result<Shared, String> {
    for d in [cfg.data_dir.clone(), cfg.fsa_dir(), cfg.projects_dir(), cfg.security_dir(), cfg.tmp_dir()] {
        std::fs::create_dir_all(&d).map_err(|e| format!("{}: {}", d.display(), e))?;
    }
    let instance_lock = if lock { Some(take_instance_lock(&cfg)?) } else { None };
    // Leftovers of interrupted uploads.
    if let Ok(rd) = std::fs::read_dir(cfg.tmp_dir()) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().ends_with(".part") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let mut conn = db::open(&cfg.db_path())?;
    db::migrate(&mut conn)?;
    let core_cfg = opentrace_core::server::Config {
        port,
        host: "127.0.0.1".into(),
        open_browser: false,
        projects_dir: Some(cfg.projects_dir()),
        security_dir: Some(cfg.security_dir()),
        allowed_hosts: cfg.allowed_hosts.clone(),
        resources: cfg.resources.clone(),
    };
    let legacy = Arc::new(opentrace_core::server::App::new(&core_cfg)?);
    let library = opentrace_core::idx_projects::ProjectLibrary::new(Some(&cfg.projects_dir()));
    Ok(Arc::new(AppState {
        cfg,
        db_conn: Mutex::new(conn),
        legacy,
        library,
        login_attempts: Mutex::new(Default::default()),
        _instance_lock: instance_lock,
    }))
}

pub fn take_instance_lock(cfg: &ServerConfig) -> Result<std::fs::File, String> {
    let p = cfg.data_dir.join("server.lock");
    let f = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&p).map_err(|e| format!("{}: {}", p.display(), e))?;
    match f.try_lock() {
        Ok(()) => Ok(f),
        Err(_) => Err(format!("{} is in use by another opentrace-server process (one server per data directory)", cfg.data_dir.display())),
    }
}

pub fn router(st: Shared) -> Router {
    let idx_limit = (st.cfg.legacy_max_body_mib * 1024 * 1024) as usize;
    let v1 = Router::new()
        .route("/health", get(api::health))
        .route("/capabilities", get(api::capabilities))
        .route("/auth/login", post(api::login))
        .route("/auth/logout", post(api::logout))
        .route("/me", get(api::me))
        .route("/projects", get(api::projects_list))
        .route("/projects/{name}", get(api::project_get))
        .route("/projects/{name}/lease", post(api::lease_acquire).delete(api::lease_release))
        .route("/projects/{name}/lease/renew", post(api::lease_renew))
        .route("/projects/{name}/lease/force-release", post(api::lease_force_release))
        .route("/projects/{name}/idx", post(api::project_idx).layer(DefaultBodyLimit::max(idx_limit)))
        .route("/projects/{name}/fsa", get(api::project_fsa_list).post(api::project_fsa_link))
        .route("/projects/{name}/analyses", get(api::analyses_list).post(api::analyses_add))
        .route("/fsa/{sha}", get(api::fsa_get).head(api::fsa_head).put(api::fsa_put))
        .route("/standards", get(api::standards_list))
        .route("/standards/{kind}/{name}", post(api::standard_create))
        .route("/standards/{kind}/{name}/{version}", get(api::standard_get).put(api::standard_update))
        .route("/standards/{kind}/{name}/{version}/{action}", post(api::standard_action))
        .route("/lab-profiles", get(api::lab_profiles))
        .route("/audit", get(api::audit_list))
        .fallback(api::not_found);
    let mut app = Router::new().nest("/api/v1", v1);
    if st.cfg.legacy_web {
        let s2 = st.clone();
        app = app.fallback(move |req: axum::extract::Request| legacy::handle(s2.clone(), req));
    }
    app.layer(axum::middleware::from_fn(access_log)).with_state(st)
}

/// One line per request: method, path (no query string, no headers, no bodies), status, duration.
async fn access_log(req: axum::extract::Request, next: axum::middleware::Next) -> axum::response::Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let t = std::time::Instant::now();
    let resp = next.run(req).await;
    if !(path.starts_with("/idx/") && resp.status().is_success()) {
        log(&format!("{} {} {} {}ms", method, path, resp.status().as_u16(), t.elapsed().as_millis()));
    }
    resp
}

pub fn log(msg: &str) {
    let t = opentrace_core::localtime::now().iso();
    eprintln!("{} {}", t, msg);
}

pub async fn serve(cfg: ServerConfig) -> Result<(), String> {
    let addr = cfg.listen_addr()?;
    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| format!("bind {}: {}", addr, e))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let st = build_state(cfg, port, true)?;
    if st.cfg.transport == config::Transport::Http && !addr.ip().is_loopback() {
        log("WARNING: plain HTTP on a network address. Passwords, session tokens, genetic data and FSA files are not encrypted.");
    }
    log(&format!(
        "OpenTrace Server {} on http://{}  data: {}  API v1: /api/v1  web UI: {}",
        env!("CARGO_PKG_VERSION"),
        addr,
        st.cfg.data_dir.display(),
        if st.cfg.legacy_web { "/idx/" } else { "off" }
    ));
    let app = router(st).into_make_service_with_connect_info::<std::net::SocketAddr>();
    axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await.map_err(|e| e.to_string())?;
    log("stopped");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = term => {},
    }
    log("shutdown requested");
}
