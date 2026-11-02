//! Batch analysis benchmark: N synthetic 5-dye FSA files (10 000 scans, GS500-like size standard on DATA105)
//! analysed through the same entry point the desktop uses (`server::App::post`, `/api/analyze`), with the shipped
//! MiniFiler_GS500 analysis method converted by `/api/analysis-method` exactly like the web UI does, and the same
//! concurrency as the UI (4 requests in flight).
//!
//!   cargo run --release --example bench_batch -- [N ...] [--lanes 4]
//!
//! Prints one JSON line per batch: time to first result, total wall time, per-file latency, response bytes (what the
//! webview has to parse) and peak resident memory of the process. The files are invented signals, not lab data.
use opentrace_core::server::{App, Config, Request};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const SCANS: usize = 10_000;
const GS500: [f64; 16] = [35.0, 50.0, 75.0, 100.0, 139.0, 150.0, 160.0, 200.0, 250.0, 300.0, 340.0, 350.0, 400.0, 450.0, 490.0, 500.0];

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn u01(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Mobility: bp → scan, mildly non-linear like a real run.
fn scan_of(bp: f64) -> f64 {
    2450.0 + 13.5 * bp + 0.002 * bp * bp
}

fn item(name: &str, number: i32, kind: u16, width: u16, count: usize, payload: Vec<u8>) -> (Vec<u8>, i32, u16, u16, u32, Vec<u8>) {
    (name.as_bytes().to_vec(), number, kind, width, count as u32, payload)
}

fn pstr(s: &str) -> Vec<u8> {
    let mut v = vec![s.len() as u8];
    v.extend_from_slice(s.as_bytes());
    v
}

fn synthetic_fsa(seed: u64) -> Vec<u8> {
    let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let name = format!("Synthetic bench {}", seed);
    let mut items = vec![item("SMPL", 1, 18, 1, name.len() + 1, pstr(&name)), item("Dye#", 1, 4, 2, 1, 5i16.to_be_bytes().to_vec())];
    for (i, (number, dye)) in [(1, "6-FAM"), (2, "VIC"), (3, "NED"), (4, "PET"), (105, "LIZ")].iter().enumerate() {
        items.push(item("DyeN", (i + 1) as i32, 18, 1, dye.len() + 1, pstr(dye)));
        // (bp, height, sigma in scans)
        let mut peaks: Vec<(f64, f64, f64)> = Vec::new();
        if *number == 105 {
            for bp in GS500 {
                peaks.push((bp, 700.0 + 500.0 * r.u01(), 3.5));
            }
        } else {
            for m in 0..6 {
                let lo = 70.0 + 60.0 * m as f64;
                for _ in 0..2 {
                    let bp = lo + 4.0 * (r.next() % 10) as f64 + 0.3;
                    let h = 300.0 + 2700.0 * r.u01();
                    peaks.push((bp, h, 4.5));
                    peaks.push((bp - 4.0, 0.08 * h, 4.5)); // stutter
                    peaks.push((bp + 1.0, 0.05 * h, 4.5)); // +A
                }
            }
            for _ in 0..8 {
                peaks.push((60.0 + 400.0 * r.u01(), 40.0 + 80.0 * r.u01(), 4.0)); // noise peaks
            }
            if seed % 16 == 3 && *number == 1 {
                peaks.push((180.3, 40_000.0, 5.0)); // off-scale
            }
        }
        let mut values = vec![0f64; SCANS];
        for (s, v) in values.iter_mut().enumerate() {
            *v = 30.0 + 0.001 * s as f64 + (r.u01() - 0.5) * 8.0;
        }
        for (bp, h, sig) in peaks {
            let c = scan_of(bp);
            let lo = (c - 8.0 * sig).max(0.0) as usize;
            let hi = ((c + 8.0 * sig) as usize).min(SCANS - 1);
            for (s, v) in values.iter_mut().enumerate().take(hi + 1).skip(lo) {
                let z = (s as f64 - c) / sig;
                *v += h * (-0.5 * z * z).exp();
            }
        }
        let data: Vec<u8> = values.iter().flat_map(|v| (v.round().clamp(-32768.0, 32767.0) as i16).to_be_bytes()).collect();
        items.push(item("DATA", *number, 4, 2, SCANS, data));
    }
    opentrace_core::demo::build_abif(&items)
}

fn peak_rss_mib() -> Option<f64> {
    #[cfg(target_os = "linux")]
    {
        let s = std::fs::read_to_string("/proc/self/status").ok()?;
        let l = s.lines().find(|l| l.starts_with("VmHWM:"))?;
        return l.split_whitespace().nth(1)?.parse::<f64>().ok().map(|kb| kb / 1024.0);
    }
    #[cfg(windows)]
    {
        #[repr(C)]
        struct Pmc {
            cb: u32,
            page_fault_count: u32,
            peak_working_set_size: usize,
            working_set_size: usize,
            quota_peak_paged_pool_usage: usize,
            quota_paged_pool_usage: usize,
            quota_peak_non_paged_pool_usage: usize,
            quota_non_paged_pool_usage: usize,
            pagefile_usage: usize,
            peak_pagefile_usage: usize,
        }
        extern "system" {
            fn GetCurrentProcess() -> isize;
            fn K32GetProcessMemoryInfo(p: isize, c: *mut Pmc, cb: u32) -> i32;
        }
        let mut c: Pmc = unsafe { std::mem::zeroed() };
        c.cb = std::mem::size_of::<Pmc>() as u32;
        let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
        return if ok != 0 { Some(c.peak_working_set_size as f64 / 1048576.0) } else { None };
    }
    #[allow(unreachable_code)]
    None
}

fn post(app: &App, target: &str, body: Vec<u8>) -> (u16, Vec<u8>) {
    let host = format!("127.0.0.1:{}", app.port);
    let req = Request {
        method: "POST".into(),
        target: target.into(),
        headers: vec![("host".into(), host), ("x-session-token".into(), app.token.clone()), ("content-length".into(), body.len().to_string())],
        body,
    };
    let r = app.post(&req);
    (r.code, r.body)
}

fn pct(sorted: &[f64], p: f64) -> f64 {
    sorted[((sorted.len() as f64 - 1.0) * p).round() as usize]
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut lanes = 4usize;
    let mut sizes = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--lanes" {
            lanes = args[i + 1].parse().expect("--lanes N");
            i += 2;
        } else {
            sizes.push(args[i].parse::<usize>().expect("batch size"));
            i += 1;
        }
    }
    if sizes.is_empty() {
        sizes = vec![1, 48, 96];
    }
    let tmp = std::env::temp_dir().join(format!("opentrace-bench-{}", std::process::id()));
    let cfg = Config { port: 8765, projects_dir: Some(tmp.join("p")), security_dir: Some(tmp.join("s")), ..Default::default() };
    let app = Arc::new(App::new(&cfg).expect("core app"));

    // Settings exactly as the web UI builds them (analyzeSettings in resources/web/idx/project.js).
    let shipped: Value = serde_json::from_slice(&std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources/data/shipped_methods.json")).unwrap()).unwrap();
    let method = shipped["methods"][0].clone();
    let (code, body) = post(&app, "/api/analysis-method", serde_json::to_vec(&json!({"action": "analyze_settings", "method": method, "standard_bp": GS500, "standard_dye": "orange"})).unwrap());
    assert_eq!(code, 200, "{}", String::from_utf8_lossy(&body));
    let settings: Value = serde_json::from_slice(&body).unwrap();
    let mut query: Vec<(String, String)> = Vec::new();
    for (k, v) in settings["settings"].as_object().unwrap() {
        match v {
            Value::Null => {}
            Value::String(s) => query.push((k.clone(), s.clone())),
            Value::Object(_) | Value::Array(_) => query.push((k.clone(), v.to_string())),
            other => query.push((k.clone(), other.to_string())),
        }
    }
    let enc = |s: &str| s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{:02X}", b) }).collect::<String>();
    let base_query: String = query.iter().map(|(k, v)| format!("{}={}", enc(k), enc(v))).collect::<Vec<_>>().join("&");

    for &n in &sizes {
        let files: Arc<Vec<Vec<u8>>> = Arc::new((0..n as u64).map(synthetic_fsa).collect());
        let input_bytes: usize = files.iter().map(|f| f.len()).sum();
        let next = Arc::new(AtomicUsize::new(0));
        let lat = Arc::new(Mutex::new(Vec::new()));
        let first = Arc::new(Mutex::new(None::<f64>));
        let out_bytes = Arc::new(AtomicUsize::new(0));
        let max_out = Arc::new(AtomicUsize::new(0));
        let bad = Arc::new(Mutex::new(Vec::new()));
        let peaks = Arc::new(AtomicUsize::new(0));
        let t0 = Instant::now();
        let handles: Vec<_> = (0..lanes.min(n.max(1)))
            .map(|_| {
                let (app, files, next, lat, first, out_bytes, max_out, bad, peaks, q) =
                    (app.clone(), files.clone(), next.clone(), lat.clone(), first.clone(), out_bytes.clone(), max_out.clone(), bad.clone(), peaks.clone(), base_query.clone());
                std::thread::spawn(move || loop {
                    let k = next.fetch_add(1, Ordering::SeqCst);
                    if k >= files.len() {
                        return;
                    }
                    let t = Instant::now();
                    let (code, body) = post(&app, &format!("/api/analyze?{}&filename=bench{}.fsa", q, k), files[k].clone());
                    let ms = t.elapsed().as_secs_f64() * 1e3;
                    let done = t0.elapsed().as_secs_f64() * 1e3;
                    first.lock().unwrap().get_or_insert(done);
                    lat.lock().unwrap().push(ms);
                    out_bytes.fetch_add(body.len(), Ordering::SeqCst);
                    max_out.fetch_max(body.len(), Ordering::SeqCst);
                    if code != 200 {
                        bad.lock().unwrap().push(format!("{}: {} {}", k, code, String::from_utf8_lossy(&body[..body.len().min(200)])));
                    } else if let Ok(v) = serde_json::from_slice::<Value>(&body) {
                        let c = v["channels"].as_array().map(|a| a.iter().map(|ch| ch["peaks"].as_array().map_or(0, |p| p.len())).sum()).unwrap_or(0);
                        if v["sizing"]["status"] != "accepted" {
                            bad.lock().unwrap().push(format!("{}: sizing {}", k, v["sizing"]["status"]));
                        }
                        peaks.fetch_add(c, Ordering::SeqCst);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let total = t0.elapsed().as_secs_f64() * 1e3;
        let mut l = lat.lock().unwrap().clone();
        l.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let bad = bad.lock().unwrap().clone();
        println!(
            "{}",
            json!({
                "files": n, "lanes": lanes, "math_backend": opentrace_core::cmath::BACKEND,
                "input_mib": (input_bytes as f64 / 1048576.0 * 100.0).round() / 100.0,
                "first_result_ms": first.lock().unwrap().map(|v| v.round()),
                "total_ms": total.round(), "files_per_s": ((n as f64) / (total / 1e3) * 10.0).round() / 10.0,
                "latency_ms": {"p50": pct(&l, 0.5).round(), "p95": pct(&l, 0.95).round(), "max": l.last().map(|v| v.round())},
                "response_mib": (out_bytes.load(Ordering::SeqCst) as f64 / 1048576.0 * 100.0).round() / 100.0,
                "response_max_kib": (max_out.load(Ordering::SeqCst) as f64 / 1024.0).round(),
                "peaks_total": peaks.load(Ordering::SeqCst),
                "errors": bad,
                "peak_rss_mib": peak_rss_mib().map(|v| v.round()),
                "threads_available": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
            })
        );
    }
    let _ = std::fs::remove_dir_all(&tmp);
}
