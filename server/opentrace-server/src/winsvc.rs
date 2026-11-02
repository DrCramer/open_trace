//! Native Windows service (Service Control Manager).
//!
//! * `service-install --config FILE [--name NAME]` (elevated): registers `NAME` (default `OpenTraceServer`), automatic
//!   start, restart on failure, running as the virtual account `NT SERVICE\NAME`, and grants that account modify
//!   rights on the configured data directory only.
//! * `service-run` is what the SCM launches; it reports Start/Running/Stopped and stops gracefully on Stop/Shutdown.
//! * `service-uninstall [--name NAME]` stops and removes the service; the data directory is left untouched.
//!
//! Logs: `<data_dir>\logs\server.log`; problems before the configuration is read go to
//! `%TEMP%\opentrace-server-service.log` of the service account.
use crate::config::ServerConfig;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use windows_service::service::{
    ServiceAccess, ServiceAction, ServiceActionType, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode,
    ServiceFailureActions, ServiceFailureResetPeriod, ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
use windows_service::{define_windows_service, service_dispatcher};

pub const DEFAULT_NAME: &str = "OpenTraceServer";
static RUN: OnceLock<(String, Option<PathBuf>)> = OnceLock::new();

define_windows_service!(ffi_service_main, service_main);

fn e(x: windows_service::Error) -> String {
    match &x {
        windows_service::Error::Winapi(io) if io.raw_os_error() == Some(5) => "access denied: run from an elevated (administrator) prompt".into(),
        windows_service::Error::Winapi(io) if io.raw_os_error() == Some(1073) => "the service already exists (service-uninstall first)".into(),
        windows_service::Error::Winapi(io) if io.raw_os_error() == Some(1060) => "no such service".into(),
        _ => format!("{} ({:?})", x, std::error::Error::source(&x)),
    }
}

/// Entry point used by the SCM (`service-run`). Fails quickly when started from a console.
pub fn run(name: &str, config: Option<PathBuf>) -> Result<(), String> {
    let _ = RUN.set((name.to_string(), config));
    service_dispatcher::start(name, ffi_service_main)
        .map_err(|x| format!("not started by the Service Control Manager: {} — use `serve` in a console", e(x)))
}

fn service_main(_args: Vec<OsString>) {
    let _ = crate::set_log_file(&std::env::temp_dir().join("opentrace-server-service.log"));
    if let Err(msg) = run_service() {
        crate::log(&format!("service failed: {}", msg));
    }
}

fn run_service() -> Result<(), String> {
    let (name, config) = RUN.get().cloned().ok_or("service state missing")?;
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let tx = Mutex::new(Some(tx));
    let handler = move |c: ServiceControl| match c {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            if let Some(t) = tx.lock().unwrap_or_else(|p| p.into_inner()).take() {
                let _ = t.send(());
            }
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    };
    let status = service_control_handler::register(&name, handler).map_err(e)?;
    let report = |state: ServiceState, accept: ServiceControlAccept, exit: ServiceExitCode, wait: u64| {
        let _ = status.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: accept,
            exit_code: exit,
            checkpoint: 0,
            wait_hint: Duration::from_secs(wait),
            process_id: None,
        });
    };
    report(ServiceState::StartPending, ServiceControlAccept::empty(), ServiceExitCode::Win32(0), 30);
    let result = (|| -> Result<(), String> {
        let cfg = ServerConfig::load(config.as_deref())?;
        crate::set_log_file(&cfg.data_dir.join("logs").join("server.log"))?;
        crate::log(&format!("service {} starting (pid {})", name, std::process::id()));
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().map_err(|x| x.to_string())?;
        report(ServiceState::Running, ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN, ServiceExitCode::Win32(0), 0);
        rt.block_on(crate::serve_until(cfg, async move {
            let _ = rx.await;
            crate::log("stop requested by the Service Control Manager");
        }))
    })();
    if let Err(msg) = &result {
        crate::log(&format!("service error: {}", msg));
    }
    // A non-zero service-specific code makes the SCM apply the configured restart actions.
    let code = if result.is_ok() { ServiceExitCode::Win32(0) } else { ServiceExitCode::ServiceSpecific(1) };
    report(ServiceState::Stopped, ServiceControlAccept::empty(), code, 0);
    result
}

pub fn install(name: &str, config: &Path) -> Result<(), String> {
    let config = std::fs::canonicalize(config).map_err(|x| format!("{}: {}", config.display(), x))?;
    let cfg = ServerConfig::load(Some(&config))?;
    std::fs::create_dir_all(&cfg.data_dir).map_err(|x| format!("{}: {}", cfg.data_dir.display(), x))?;
    let exe = std::env::current_exe().map_err(|x| x.to_string())?;
    let account = format!("NT SERVICE\\{}", name);
    let mgr = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE).map_err(e)?;
    let info = ServiceInfo {
        name: OsString::from(name),
        display_name: OsString::from("OpenTrace Server"),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: exe,
        launch_arguments: vec!["service-run".into(), "--name".into(), name.into(), "--config".into(), config.clone().into_os_string()],
        dependencies: vec![],
        account_name: Some(account.clone().into()),
        account_password: None,
    };
    let svc = mgr.create_service(&info, ServiceAccess::CHANGE_CONFIG | ServiceAccess::QUERY_STATUS).map_err(e)?;
    svc.set_description("OpenTrace Server: projects, FSA store and laboratory standards (research software, not validated for diagnostic or forensic use)")
        .map_err(e)?;
    let restart = |s| ServiceAction { action_type: ServiceActionType::Restart, delay: Duration::from_secs(s) };
    svc.update_failure_actions(ServiceFailureActions {
        reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(24 * 3600)),
        reboot_msg: None,
        command: None,
        actions: Some(vec![restart(10), restart(60), restart(300)]),
    })
    .map_err(e)?;
    svc.set_failure_actions_on_non_crash_failures(true).map_err(e)?;
    // Least privilege: the virtual account may modify the data directory and nothing else we create.
    let grant = |p: &Path, spec: &str| -> Result<(), String> {
        let out = std::process::Command::new("icacls").arg(p).arg("/grant").arg(format!("{}:{}", account, spec)).output().map_err(|x| x.to_string())?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!("icacls {}: {}", p.display(), String::from_utf8_lossy(&out.stdout).trim()))
        }
    };
    grant(&cfg.data_dir, "(OI)(CI)M")?;
    if !config.starts_with(&cfg.data_dir) {
        grant(&config, "R")?;
    }
    println!("installed service {} (account {}, automatic start, restart on failure)", name, account);
    println!("data directory {} (modify rights granted to {})", cfg.data_dir.display(), account);
    println!("start: sc.exe start {}   logs: {}", name, cfg.data_dir.join("logs").join("server.log").display());
    Ok(())
}

pub fn uninstall(name: &str) -> Result<(), String> {
    let mgr = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT).map_err(e)?;
    let svc = mgr.open_service(name, ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE).map_err(e)?;
    if svc.query_status().map_err(e)?.current_state != ServiceState::Stopped {
        let _ = svc.stop();
        for _ in 0..60 {
            if svc.query_status().map_err(e)?.current_state == ServiceState::Stopped {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }
    svc.delete().map_err(e)?;
    println!("removed service {} (data directory untouched)", name);
    Ok(())
}
