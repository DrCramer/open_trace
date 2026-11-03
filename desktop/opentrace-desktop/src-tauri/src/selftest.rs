//! Desktop self-test (`OPENTRACE_DESKTOP_SELFTEST=<report.json>`): drives the real UI in the real webview — core
//! analysis of a synthetic FSA through the scheme, the example project, a Samples Plot window opened with
//! `window.open` that must reach `window.opener.OT` and draw on its canvas — then writes a JSON report and exits.
//! Used by CI and for manual checks on Windows/Linux; inactive in normal runs.
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::AppHandle;

pub const SELFTEST_JS: &str = include_str!("../../ui/selftest.js");

/// `OPENTRACE_DESKTOP_SELFTEST_BATCH=N` (1..=500): also run the batch measurement (see `ui/selftest.js`).
pub fn batch_size() -> Option<u64> {
    std::env::var("OPENTRACE_DESKTOP_SELFTEST_BATCH").ok().and_then(|v| v.parse::<u64>().ok()).filter(|n| (1..=500).contains(n))
}

/// Resident memory of this process and all its descendants (the WebKitGTK web/network processes), Linux only.
/// WebView2 exposes the JS heap through `performance.memory` instead.
pub fn process_tree_rss_mib() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let me = std::process::id();
        let mut parent = std::collections::HashMap::new();
        let mut rss = std::collections::HashMap::new();
        for e in std::fs::read_dir("/proc").ok()?.flatten() {
            let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else { continue };
            let Ok(stat) = std::fs::read_to_string(e.path().join("stat")) else { continue };
            // pid (comm) state ppid ... ; comm may contain spaces, so split after the last ')'.
            let Some(rest) = stat.rsplit_once(')').map(|x| x.1) else { continue };
            let f: Vec<&str> = rest.split_whitespace().collect();
            if f.len() > 21 {
                parent.insert(pid, f[1].parse::<u32>().unwrap_or(0));
                rss.insert(pid, f[21].parse::<u64>().unwrap_or(0)); // pages
            }
        }
        let page = 4096u64;
        let mut total = 0u64;
        for (&pid, &r) in &rss {
            let mut p = pid;
            for _ in 0..64 {
                if p == me {
                    total += r * page;
                    break;
                }
                match parent.get(&p) {
                    Some(&q) if q != 0 => p = q,
                    _ => break,
                }
            }
        }
        return Some(total / 1_048_576);
    }
    #[allow(unreachable_code)]
    None
}

/// `OPENTRACE_DESKTOP_SELFTEST_GENOTYPE=N` (1..=96): import the synthetic kit through the Panel Manager's own store,
/// add an allelic ladder and N synthetic samples, analyse and genotype them through the UI and compare every call with
/// the alleles the files were made with.
pub fn genotype_size() -> Option<u64> {
    std::env::var("OPENTRACE_DESKTOP_SELFTEST_GENOTYPE").ok().and_then(|v| v.parse::<u64>().ok()).filter(|n| (1..=96).contains(n))
}

/// `OPENTRACE_SELFTEST_PROFILE=<name>:<version>` (with a server and a batch): activate this approved lab profile, save
/// the batch as a server project and require provenance attributed to the profile for every sample.
pub fn profile() -> Option<(String, i64)> {
    let v = std::env::var("OPENTRACE_SELFTEST_PROFILE").ok()?;
    let (n, ver) = v.rsplit_once(':')?;
    Some((n.to_string(), ver.parse().ok()?))
}

pub struct SelfTest {
    pub out: PathBuf,
    started: std::time::Instant,
    reports: Mutex<Vec<(String, Value)>>,
}

impl SelfTest {
    pub fn from_env() -> Option<SelfTest> {
        let p = std::env::var_os("OPENTRACE_DESKTOP_SELFTEST").filter(|v| !v.is_empty())?;
        Some(SelfTest { out: PathBuf::from(p), started: std::time::Instant::now(), reports: Mutex::new(Vec::new()) })
    }

    pub fn record(&self, app: &AppHandle, label: &str, v: Value) {
        let mut g = self.reports.lock().unwrap_or_else(|e| e.into_inner());
        g.push((label.to_string(), v));
        let have_main = g.iter().any(|(_, v)| v["window"] == "main");
        let have_plot = g.iter().any(|(_, v)| v["window"] == "plot");
        if have_main && have_plot {
            self.finish(app, &g);
        }
    }

    pub fn finish(&self, app: &AppHandle, reports: &[(String, Value)]) {
        let main = reports.iter().find(|(_, v)| v["window"] == "main").map(|r| r.1["checks"].clone()).unwrap_or(Value::Null);
        let plot = reports.iter().find(|(_, v)| v["window"] == "plot").map(|r| r.1["checks"].clone()).unwrap_or(Value::Null);
        let pass = main["ot_loaded"] == true
            && main["health"] == true
            && main["analyze_status"] == 200
            && main["analyze_channels"].as_u64().unwrap_or(0) > 0
            && main["plot_opened"] == true
            && plot["opener_ot"] == true
            && plot["canvas"] == true
            && plot["colored_pixels"].as_u64().unwrap_or(0) > 100
            && plot["samples_in_payload"].as_u64().unwrap_or(0) >= 1
            && (std::env::var_os("OPENTRACE_SELFTEST_SERVER_URL").is_none()
                || (main["sync"]["done"].as_u64().unwrap_or(0) >= 1
                    && main["ws_saved"] == true
                    && main["ws_loaded_samples"].as_u64().unwrap_or(0) >= 1
                    && main["ws_after_open"]["editing"] == true
                    && (main["local_methods"].as_u64().unwrap_or(0) == 0 || main["std_hash_match"] == true)))
            && (batch_size().is_none()
                || (main["batch"]["analyzed"].as_u64() == batch_size()
                    && main["batch"]["sizing_accepted"].as_u64() == batch_size()
                    && main["batch"]["plot_ready"] == true
                    && (profile().is_none()
                        || (main["batch"]["provenance"]["posted"].as_u64() == batch_size()
                            && main["batch"]["provenance"]["profile_mismatch"] == 0
                            && main["batch"]["lab_profile"]["verified"] == true))))
            && (genotype_size().is_none()
                || (main["genotype"]["samples_genotyped"].as_u64() == genotype_size()
                    && main["genotype"]["markers_ok"].as_u64() == genotype_size().map(|n| n * 24)
                    && main["genotype"]["mismatches"].as_u64() == Some(0)
                    // With a server and a profile: the ladder and every sample saved with provenance attributed to it.
                    && (profile().is_none()
                        || std::env::var_os("OPENTRACE_SELFTEST_SERVER_URL").is_none()
                        || (main["genotype"]["provenance"]["posted"].as_u64() == genotype_size().map(|n| n + 1)
                            && main["genotype"]["provenance"]["profile_mismatch"] == 0))));
        let report = json!({
            "pass": pass,
            "platform": std::env::consts::OS,
            "app_version": env!("CARGO_PKG_VERSION"),
            "analysis_version": opentrace_core::server_analyze::analysis_version(),
            "math_backend": opentrace_core::cmath::BACKEND,
            "elapsed_ms": self.started.elapsed().as_millis() as u64,
            "main": main,
            "plot": plot,
            "windows": reports.iter().map(|(l, _)| l.clone()).collect::<Vec<_>>(),
        });
        let _ = std::fs::write(&self.out, serde_json::to_vec_pretty(&report).unwrap());
        eprintln!("[selftest] {} -> {}", if pass { "PASS" } else { "FAIL" }, self.out.display());
        app.exit(if pass { 0 } else { 1 });
    }

    /// Writes a failure report if the UI never reported back.
    pub fn watchdog(self: std::sync::Arc<Self>, app: AppHandle, secs: u64) {
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(secs));
            let g = self.reports.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let mut v = g.clone();
            v.push(("watchdog".into(), json!({"window": "watchdog", "timeout_s": secs})));
            self.finish(&app, &v);
        });
    }
}
