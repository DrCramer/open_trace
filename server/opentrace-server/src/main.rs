//! `opentrace-server` command line.
use opentrace_server::config::ServerConfig;
use opentrace_server::{backup, db};
use serde_json::json;
use std::io::BufRead;
use std::path::PathBuf;

const USAGE: &str = "opentrace-server <command> [--config FILE]

Commands:
  serve                      run the server (API v1 at /api/v1, web UI at /idx/)
  check-config               validate configuration and print the effective values
  migrate                    apply pending database migrations and exit
  set-password USER          set a user's password (read from stdin or OPENTRACE_NEW_PASSWORD)
  grant-approver USER        add USER to the \"OpenTrace Approvers\" group (creates the group)
  backup [--out DIR]         consistent backup of database, projects, security store and FSA files
  verify-backup DIR          check a backup against its manifest
  restore DIR                restore a backup into an empty data directory
  version                    print versions

Configuration: --config FILE (TOML), then OPENTRACE_SERVER_* environment variables (see docs/desktop/DEPLOY_*.md).";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(args) {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

fn take_opt(args: &mut Vec<String>, name: &str) -> Option<String> {
    let i = args.iter().position(|a| a == name)?;
    if i + 1 < args.len() {
        let v = args.remove(i + 1);
        args.remove(i);
        Some(v)
    } else {
        None
    }
}

fn run(mut args: Vec<String>) -> Result<(), String> {
    let config = take_opt(&mut args, "--config").map(PathBuf::from);
    let out = take_opt(&mut args, "--out").map(PathBuf::from);
    let cmd = if args.is_empty() { "help".to_string() } else { args.remove(0) };
    let load = || ServerConfig::load(config.as_deref());
    match cmd.as_str() {
        "serve" => {
            let cfg = load()?;
            let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().map_err(|e| e.to_string())?;
            rt.block_on(opentrace_server::serve(cfg))
        }
        "check-config" => {
            let cfg = load()?;
            println!("{:#?}", cfg);
            Ok(())
        }
        "migrate" => {
            let cfg = load()?;
            let mut c = db::open(&cfg.db_path())?;
            let applied = db::migrate(&mut c)?;
            println!("schema version {} (applied now: {:?})", db::schema_version(&c)?, applied);
            Ok(())
        }
        "set-password" => {
            let user = args.first().ok_or("set-password USER")?.clone();
            let pw = match std::env::var("OPENTRACE_NEW_PASSWORD") {
                Ok(p) if !p.is_empty() => p,
                _ => {
                    eprintln!("New password for {} (one line on stdin):", user);
                    let mut line = String::new();
                    std::io::stdin().lock().read_line(&mut line).map_err(|e| e.to_string())?;
                    line.trim_end_matches(['\r', '\n']).to_string()
                }
            };
            if pw.is_empty() {
                return Err("empty password".into());
            }
            let cfg = load()?;
            std::fs::create_dir_all(cfg.security_dir()).map_err(|e| e.to_string())?;
            let _lock = opentrace_server::take_instance_lock(&cfg).map_err(|e| format!("{} — stop the server first", e))?;
            let mut store = opentrace_core::security::SecurityStore::open(&cfg.security_dir().to_string_lossy()).map_err(|e| format!("{:?}", e))?;
            let mut u = store.user(&json!(user)).ok_or_else(|| format!("unknown user {}", user))?;
            u["password"] = json!(pw);
            store.save_user(&u, &json!(user)).map_err(|e| format!("{:?}", e))?;
            println!("password set for {}", user);
            Ok(())
        }
        "grant-approver" => {
            let user = args.first().ok_or("grant-approver USER")?.clone();
            let cfg = load()?;
            let _lock = opentrace_server::take_instance_lock(&cfg).map_err(|e| format!("{} — stop the server first", e))?;
            let mut store = opentrace_core::security::SecurityStore::open(&cfg.security_dir().to_string_lossy()).map_err(|e| format!("{:?}", e))?;
            let u = store.user(&json!(user)).ok_or_else(|| format!("unknown user {}", user))?;
            let group = opentrace_server::auth::APPROVER_GROUP;
            let state = store.admin_state().map_err(|e| format!("{:?}", e))?;
            let exists = state["user_groups"].as_array().map(|g| g.iter().any(|x| x["name"] == group)).unwrap_or(false);
            let mut members: Vec<serde_json::Value> = state["users"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|x| x["groups"].as_array().map(|g| g.iter().any(|n| n == group)).unwrap_or(false))
                .map(|x| x["name"].clone())
                .collect();
            if !members.contains(&u["name"]) {
                members.push(u["name"].clone());
            }
            let data = json!({"name": group, "description": "May approve OpenTrace laboratory standards (API v1)", "members": members});
            store.save_group(&data, &if exists { json!(group) } else { json!("") }).map_err(|e| format!("{:?}", e))?;
            println!("{} is now in \"{}\"", user, group);
            Ok(())
        }
        "backup" => {
            let cfg = load()?;
            let dir = out.or(cfg.backup_dir.clone()).unwrap_or_else(|| cfg.data_dir.join("backups"));
            let p = backup::backup(&cfg, &dir)?;
            println!("{}", p.display());
            Ok(())
        }
        "verify-backup" => {
            let dir = PathBuf::from(args.first().ok_or("verify-backup DIR")?);
            println!("ok: {} files", backup::verify(&dir)?);
            Ok(())
        }
        "restore" => {
            let dir = PathBuf::from(args.first().ok_or("restore DIR")?);
            let cfg = load()?;
            std::fs::create_dir_all(&cfg.data_dir).map_err(|e| e.to_string())?;
            let _lock = opentrace_server::take_instance_lock(&cfg).map_err(|e| format!("{} — stop the server first", e))?;
            println!("restored {} files into {}", backup::restore(&dir, &cfg)?, cfg.data_dir.display());
            Ok(())
        }
        "version" | "--version" => {
            println!(
                "opentrace-server {} (API v{}, {}, math {})",
                env!("CARGO_PKG_VERSION"),
                opentrace_server::api::API_VERSION,
                opentrace_core::server_analyze::analysis_version(),
                opentrace_core::cmath::BACKEND
            );
            Ok(())
        }
        _ => {
            println!("{}", USAGE);
            if cmd == "help" || cmd == "--help" || cmd == "-h" {
                Ok(())
            } else {
                Err(format!("unknown command {}", cmd))
            }
        }
    }
}
