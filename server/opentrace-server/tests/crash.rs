//! Crash safety: a real `opentrace-server serve` process is killed with SIGKILL while clients upload FSA files and
//! save a leased project. After a restart the database passes SQLite's integrity check, every stored FSA file has the
//! content its name (SHA-256) promises, no upload leftovers remain, the project is readable at a consistent revision,
//! and the server accepts new work.
#![cfg(unix)]
use serde_json::{json, Value};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_opentrace-server");

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn start(dir: &std::path::Path, port: u16) -> Child {
    let child = Command::new(BIN)
        .arg("serve")
        .env("OPENTRACE_SERVER_DATA_DIR", dir)
        .env("OPENTRACE_SERVER_TRANSPORT", "http")
        .env("OPENTRACE_SERVER_LISTEN", format!("127.0.0.1:{}", port))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let t0 = Instant::now();
    while ureq::get(&format!("http://127.0.0.1:{}/api/v1/health", port)).call().is_err() {
        assert!(t0.elapsed() < Duration::from_secs(20), "server did not start");
        std::thread::sleep(Duration::from_millis(100));
    }
    child
}

fn login(base: &str) -> String {
    let v: Value = ureq::post(&format!("{}/api/v1/auth/login", base)).send_json(json!({"username": "Casework Analyst", "password": "crash-pw-1"})).unwrap().into_json().unwrap();
    v["token"].as_str().unwrap().to_string()
}

fn sha(b: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(b).iter().map(|x| format!("{:02x}", x)).collect()
}

#[test]
fn sigkill_during_uploads_and_project_writes_leaves_a_consistent_store() {
    let dir = tempfile::tempdir().unwrap();
    let out = Command::new(BIN)
        .args(["set-password", "Casework Analyst"])
        .env("OPENTRACE_SERVER_DATA_DIR", dir.path())
        .env("OPENTRACE_NEW_PASSWORD", "crash-pw-1")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let port = free_port();
    let base = format!("http://127.0.0.1:{}", port);
    let mut server = start(dir.path(), port);
    let token = login(&base);
    let auth = format!("Bearer {}", token);
    let lease: Value = ureq::post(&format!("{}/api/v1/projects/Crash/lease", base)).set("authorization", &auth).send_json(json!({})).unwrap().into_json().unwrap();

    let stop = Arc::new(AtomicBool::new(false));
    let (uploaded, saved) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let mut workers = Vec::new();
    for w in 0..3u64 {
        let (stop, uploaded, base, auth) = (stop.clone(), uploaded.clone(), base.clone(), auth.clone());
        workers.push(std::thread::spawn(move || {
            let mut seed = w * 1_000_000;
            while !stop.load(Ordering::SeqCst) {
                let fsa = opentrace_core::demo::synthetic_run_fsa(seed);
                seed += 1;
                let r = ureq::put(&format!("{}/api/v1/fsa/{}", base, sha(&fsa))).set("authorization", &auth).send_bytes(&fsa);
                if r.is_ok() {
                    uploaded.fetch_add(1, Ordering::SeqCst);
                }
            }
        }));
    }
    {
        let (stop, saved, base, auth, lease) = (stop.clone(), saved.clone(), base.clone(), auth.clone(), lease.clone());
        workers.push(std::thread::spawn(move || {
            let mut rev = lease["revision"].as_i64().unwrap_or(0);
            let mut n = 0;
            while !stop.load(Ordering::SeqCst) {
                n += 1;
                let samples: Vec<Value> = (0..20).map(|i| json!({"id": i.to_string(), "file": format!("s{}.fsa", i), "status": "analyzed"})).collect();
                let body = json!({"action": "save_meta", "project": {"samples": samples, "comment": format!("save {}", n)}, "create": rev == 0, "overwrite": rev != 0});
                let r = ureq::post(&format!("{}/api/v1/projects/Crash/idx", base))
                    .set("authorization", &auth)
                    .set("x-lease-id", lease["lease_id"].as_str().unwrap())
                    .set("x-fencing-token", &lease["fencing_token"].to_string())
                    .set("x-expected-revision", &rev.to_string())
                    .send_json(body);
                match r {
                    Ok(resp) => {
                        let v: Value = resp.into_json().unwrap_or(Value::Null);
                        rev = v["revision"].as_i64().unwrap_or(rev + 1);
                        saved.fetch_add(1, Ordering::SeqCst);
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        }));
    }
    // Let real work happen, then pull the plug.
    let t0 = Instant::now();
    while (uploaded.load(Ordering::SeqCst) < 20 || saved.load(Ordering::SeqCst) < 10) && t0.elapsed() < Duration::from_secs(60) {
        std::thread::sleep(Duration::from_millis(20));
    }
    server.kill().unwrap(); // SIGKILL
    server.wait().unwrap();
    stop.store(true, Ordering::SeqCst);
    for w in workers {
        w.join().unwrap();
    }
    assert!(uploaded.load(Ordering::SeqCst) >= 20 && saved.load(Ordering::SeqCst) >= 10, "not enough work before the kill");

    // Restart on the same directory.
    let mut server = start(dir.path(), port);
    let db = rusqlite::Connection::open(dir.path().join("opentrace.sqlite3")).unwrap();
    let ok: String = db.query_row("PRAGMA integrity_check", [], |r| r.get(0)).unwrap();
    assert_eq!(ok, "ok");
    // Every stored file is complete and named by its content; nothing half-written left anywhere.
    let mut files = 0;
    for e in walk(&dir.path().join("fsa")) {
        let name = e.file_name().unwrap().to_string_lossy().to_string();
        let stem = name.trim_end_matches(".fsa");
        assert_eq!(sha(&std::fs::read(&e).unwrap()), stem, "{}", e.display());
        files += 1;
    }
    assert!(files >= 20, "{} files", files);
    assert!(walk(&dir.path().join("tmp")).iter().all(|p| !p.to_string_lossy().ends_with(".part")));
    // The server answers, the project reads back at its stored revision, and work continues.
    let token = login(&base);
    let auth = format!("Bearer {}", token);
    let p: Value = ureq::get(&format!("{}/api/v1/projects/Crash", base)).set("authorization", &auth).call().unwrap().into_json().unwrap();
    assert!(p["revision"].as_i64().unwrap() >= 10, "{}", p);
    assert!(p["project"]["comment"].as_str().unwrap().starts_with("save "), "{}", p);
    let fresh = opentrace_core::demo::synthetic_run_fsa(999_999_999);
    assert!(ureq::put(&format!("{}/api/v1/fsa/{}", base, sha(&fresh))).set("authorization", &auth).send_bytes(&fresh).is_ok());
    server.kill().unwrap();
    server.wait().unwrap();
}

fn walk(d: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(d) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else {
                out.push(p);
            }
        }
    }
    out
}
