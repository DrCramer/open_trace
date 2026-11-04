//! Diagnostics: a small log file and a watchdog for the UI thread.
//!
//! * `dlog!` writes to stderr (as before) and to `<data dir>/logs/desktop.log` (rotated at 5 MiB, one previous
//!   file kept). Only technical events go there — window, request and error messages the app already printed —
//!   never passwords, tokens, FSA or genotypes.
//! * The watchdog posts a no-op to the UI thread every 2 s. If it has not run for `OPENTRACE_DESKTOP_HANG_SECONDS`
//!   (default 15), the app is hung ("not responding", white windows): this is logged, and on Windows a minidump of the
//!   process (thread stacks and thread information, no full memory) is written to `logs/hang-<time>.dmp` once per
//!   hang, at most three kept. The duration is logged when the UI thread recovers. These dumps are what was needed to
//!   find the popup deadlock fixed in `geometry.rs`; they stay on this computer unless the user sends them.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

static LOG: OnceLock<Mutex<LogFile>> = OnceLock::new();
const MAX_LOG: u64 = 5 * 1024 * 1024;
#[cfg_attr(not(windows), allow(dead_code))]
const KEEP_DUMPS: usize = 3;

struct LogFile {
    path: PathBuf,
    file: Option<std::fs::File>,
}

#[macro_export]
macro_rules! dlog {
    ($($t:tt)*) => {{
        let line = format!($($t)*);
        eprintln!("{}", line);
        $crate::diag::write_line(&line);
    }};
}

pub fn logs_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("logs")
}

pub fn init(data_dir: &Path) {
    let dir = logs_dir(data_dir);
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("desktop.log");
    let _ = LOG.set(Mutex::new(LogFile { file: open_append(&path), path }));
    write_line(&format!("[desktop] start {} pid {} ({} {})", env!("CARGO_PKG_VERSION"), std::process::id(), std::env::consts::OS, std::env::consts::ARCH));
}

fn open_append(p: &Path) -> Option<std::fs::File> {
    std::fs::OpenOptions::new().create(true).append(true).open(p).ok()
}

pub fn write_line(line: &str) {
    let Some(m) = LOG.get() else { return };
    let mut g = m.lock().unwrap_or_else(|e| e.into_inner());
    let size = g.file.as_ref().and_then(|f| f.metadata().ok()).map(|m| m.len()).unwrap_or(0);
    if size > MAX_LOG {
        let old = g.path.with_extension("1.log");
        g.file = None;
        let _ = std::fs::rename(&g.path, &old);
        g.file = open_append(&g.path);
    }
    let ts = opentrace_core::localtime::now().iso();
    if let Some(f) = g.file.as_mut() {
        let _ = writeln!(f, "{} {}", ts, line);
    }
}

fn hang_threshold() -> Duration {
    let s = std::env::var("OPENTRACE_DESKTOP_HANG_SECONDS").ok().and_then(|v| v.parse::<u64>().ok()).filter(|s| *s >= 2).unwrap_or(15);
    Duration::from_secs(s)
}

/// Result of the watchdog so far (for the self-test and the connection page).
#[derive(Default)]
pub struct WatchdogState {
    pub hangs: AtomicU64,
    pub last_dump: Mutex<Option<PathBuf>>,
}

/// Starts the UI-thread watchdog. `post` must schedule the closure on the UI thread (Tauri `run_on_main_thread`).
pub fn start_watchdog(logs: PathBuf, state: Arc<WatchdogState>, post: impl Fn(Box<dyn FnOnce() + Send>) + Send + 'static) {
    let threshold = hang_threshold();
    let started = Instant::now();
    let beat = Arc::new(AtomicU64::new(0)); // ms since `started` of the last UI-thread run
    std::thread::Builder::new()
        .name("ui-watchdog".into())
        .spawn(move || {
            let mut hung_since: Option<u64> = None;
            loop {
                let b = beat.clone();
                post(Box::new(move || b.store(started.elapsed().as_millis() as u64, Ordering::SeqCst)));
                std::thread::sleep(Duration::from_secs(2));
                let now = started.elapsed().as_millis() as u64;
                let last = beat.load(Ordering::SeqCst);
                let silent = now.saturating_sub(last.max(1));
                if last != 0 && silent >= threshold.as_millis() as u64 {
                    if hung_since.is_none() {
                        hung_since = Some(last);
                        crate::dlog!("[desktop] UI thread not responding for {} s", silent / 1000);
                        match write_dump(&logs) {
                            Ok(Some(p)) => {
                                crate::dlog!("[desktop] hang dump written: {}", p.display());
                                *state.last_dump.lock().unwrap_or_else(|e| e.into_inner()) = Some(p);
                            }
                            Ok(None) => {}
                            Err(e) => crate::dlog!("[desktop] hang dump failed: {}", e),
                        }
                        // Counted after the dump, so whoever sees the count also finds the dump.
                        state.hangs.fetch_add(1, Ordering::SeqCst);
                    }
                } else if let Some(since) = hung_since.take() {
                    crate::dlog!("[desktop] UI thread responding again after {:.1} s", (last.saturating_sub(since)) as f64 / 1000.0);
                }
            }
        })
        .ok();
}

/// Minidump of this process (Windows): thread stacks + thread info. `Ok(None)` elsewhere.
pub fn write_dump(logs: &Path) -> Result<Option<PathBuf>, String> {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        #[link(name = "dbghelp")]
        extern "system" {
            fn MiniDumpWriteDump(process: isize, pid: u32, file: isize, kind: u32, e: *const u8, u: *const u8, c: *const u8) -> i32;
        }
        extern "system" {
            fn GetCurrentProcess() -> isize;
        }
        std::fs::create_dir_all(logs).map_err(|e| e.to_string())?;
        prune_dumps(logs, KEEP_DUMPS - 1);
        let t = opentrace_core::localtime::now();
        let name = format!("hang-{:04}{:02}{:02}-{:02}{:02}{:02}.dmp", t.year + 1900, t.mon + 1, t.mday, t.hour, t.min, t.sec);
        let p = logs.join(name);
        let f = std::fs::File::create(&p).map_err(|e| e.to_string())?;
        // MiniDumpNormal | MiniDumpWithThreadInfo | MiniDumpWithHandleData
        let ok = unsafe { MiniDumpWriteDump(GetCurrentProcess(), std::process::id(), f.as_raw_handle() as isize, 0x1004, std::ptr::null(), std::ptr::null(), std::ptr::null()) };
        drop(f);
        if ok == 0 {
            let _ = std::fs::remove_file(&p);
            return Err(format!("MiniDumpWriteDump: {}", std::io::Error::last_os_error()));
        }
        Ok(Some(p))
    }
    #[cfg(not(windows))]
    {
        let _ = logs;
        Ok(None)
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn prune_dumps(logs: &Path, keep: usize) {
    let mut dumps: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(logs)
        .map(|r| {
            r.flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("hang-") && e.file_name().to_string_lossy().ends_with(".dmp"))
                .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
                .collect()
        })
        .unwrap_or_default();
    dumps.sort();
    while dumps.len() > keep {
        let (_, p) = dumps.remove(0);
        let _ = std::fs::remove_file(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_keeps_the_newest() {
        let dir = std::env::temp_dir().join(format!("ot-diag-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..5 {
            std::fs::write(dir.join(format!("hang-{}.dmp", i)), b"x").unwrap();
            std::thread::sleep(Duration::from_millis(20));
        }
        std::fs::write(dir.join("desktop.log"), b"x").unwrap();
        prune_dumps(&dir, 2);
        let mut left: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left, vec!["desktop.log", "hang-3.dmp", "hang-4.dmp"]);
        let _ = std::fs::remove_dir_all(dir);
    }
}
