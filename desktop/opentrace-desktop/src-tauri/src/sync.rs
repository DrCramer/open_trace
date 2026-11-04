//! Local FSA cache and background upload queue.
//!
//! * Cache: `<local data>/fsa-cache/ab/cd/<sha256>.fsa`, content addressed and written once (temp file + rename after
//!   the hash is verified); the user's original files are never touched. `<sha>.uploaded` marks files the server has.
//! * Queue: pending hashes are uploaded by a small number of worker threads (independent of the analysis pool):
//!   `HEAD` first (no second copy if the server already has the bytes), then a streamed binary `PUT`. Network errors
//!   back off and retry; an expired session pauses the queue until the next login; pending items survive restarts
//!   because the cache itself is the queue.
//! * Size limit: when the cache exceeds its budget the oldest already-uploaded files are evicted first; files not yet
//!   on the server are never evicted.
use crate::remote::{Remote, RemoteError};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemState {
    Pending,
    Uploading,
    Done,
    Failed(String),
}

#[derive(Clone, Debug)]
struct Item {
    name: String,
    size: u64,
    state: ItemState,
    attempts: u32,
    not_before: Option<Instant>,
}

#[derive(Default)]
struct Inner {
    items: BTreeMap<String, Item>,
    queue: VecDeque<String>,
    paused_reason: Option<String>,
    last_error: Option<String>,
    uploaded_bytes: u64,
    deduplicated: u64,
}

pub struct SyncQueue {
    pub dir: PathBuf,
    inner: Mutex<Inner>,
    cv: Condvar,
    pub max_bytes: Mutex<u64>,
    pub enabled: Mutex<bool>,
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{:02x}", x)).collect()
}

fn blob_path(dir: &Path, sha: &str) -> PathBuf {
    dir.join(&sha[0..2]).join(&sha[2..4]).join(format!("{}.fsa", sha))
}

fn marker(dir: &Path, sha: &str) -> PathBuf {
    dir.join(&sha[0..2]).join(&sha[2..4]).join(format!("{}.uploaded", sha))
}

impl SyncQueue {
    pub fn open(dir: PathBuf, max_bytes: u64) -> Arc<SyncQueue> {
        let _ = std::fs::create_dir_all(&dir);
        let q = Arc::new(SyncQueue { dir, inner: Mutex::new(Inner::default()), cv: Condvar::new(), max_bytes: Mutex::new(max_bytes), enabled: Mutex::new(true) });
        q.rescan();
        q
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Rebuilds the queue from the cache directory (restart recovery).
    pub fn rescan(&self) {
        let mut found: Vec<(String, u64, bool)> = Vec::new();
        if let Ok(a) = std::fs::read_dir(&self.dir) {
            for a in a.flatten() {
                if let Ok(b) = std::fs::read_dir(a.path()) {
                    for b in b.flatten() {
                        if let Ok(c) = std::fs::read_dir(b.path()) {
                            for f in c.flatten() {
                                let n = f.file_name().to_string_lossy().into_owned();
                                if let Some(sha) = n.strip_suffix(".fsa") {
                                    if sha.len() == 64 {
                                        let size = f.metadata().map(|m| m.len()).unwrap_or(0);
                                        found.push((sha.to_string(), size, marker(&self.dir, sha).exists()));
                                    }
                                } else if n.ends_with(".part") {
                                    let _ = std::fs::remove_file(f.path());
                                }
                            }
                        }
                    }
                }
            }
        }
        let mut g = self.lock();
        for (sha, size, done) in found {
            if g.items.contains_key(&sha) {
                continue;
            }
            g.items.insert(sha.clone(), Item { name: format!("{}.fsa", &sha[..12]), size, state: if done { ItemState::Done } else { ItemState::Pending }, attempts: 0, not_before: None });
            if !done {
                g.queue.push_back(sha);
            }
        }
        drop(g);
        self.cv.notify_all();
    }

    /// The cached original with this SHA-256, verified (None when not cached or damaged).
    pub fn cached(&self, sha: &str) -> Option<Vec<u8>> {
        if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let b = std::fs::read(blob_path(&self.dir, sha)).ok()?;
        (hex(&Sha256::digest(&b)) == sha).then_some(b)
    }

    /// Keeps an FSA downloaded from the server in the cache, already marked as on the server (no upload queued), so
    /// opening the project again needs no transfer.
    pub fn cache_downloaded(&self, name: &str, data: &[u8]) -> Result<String, String> {
        let sha = hex(&Sha256::digest(data));
        let m = marker(&self.dir, &sha);
        if let Some(p) = m.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        std::fs::write(&m, b"").map_err(|e| e.to_string())?;
        self.add(name, data)
    }

    /// Stores the bytes of an imported FSA in the cache (if new) and queues the upload. Returns the SHA-256.
    pub fn add(&self, name: &str, data: &[u8]) -> Result<String, String> {
        if data.len() < 4 || &data[0..4] != b"ABIF" {
            return Err("not an ABIF file".into());
        }
        let sha = hex(&Sha256::digest(data));
        let dest = blob_path(&self.dir, &sha);
        if !dest.exists() {
            let parent = dest.parent().unwrap();
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            let tmp = parent.join(format!("{}.{}.part", sha, std::process::id()));
            std::fs::write(&tmp, data).map_err(|e| format!("cache write: {}", e))?;
            // Read back and verify before publishing the file under its hash.
            let back = std::fs::read(&tmp).map_err(|e| e.to_string())?;
            if hex(&Sha256::digest(&back)) != sha {
                let _ = std::fs::remove_file(&tmp);
                return Err("cache write verification failed".into());
            }
            if let Err(e) = std::fs::rename(&tmp, &dest) {
                let _ = std::fs::remove_file(&tmp);
                if !dest.exists() {
                    return Err(e.to_string());
                }
            }
        }
        let mut g = self.lock();
        let done = marker(&self.dir, &sha).exists();
        let entry = g.items.entry(sha.clone()).or_insert(Item { name: name.to_string(), size: data.len() as u64, state: ItemState::Pending, attempts: 0, not_before: None });
        entry.name = name.to_string();
        let requeue = !done && matches!(entry.state, ItemState::Failed(_) | ItemState::Pending);
        if done {
            entry.state = ItemState::Done;
        } else if matches!(entry.state, ItemState::Failed(_)) {
            entry.state = ItemState::Pending;
            entry.attempts = 0;
        }
        if requeue && !g.queue.contains(&sha) {
            g.queue.push_back(sha.clone());
        }
        drop(g);
        self.cv.notify_all();
        self.evict();
        Ok(sha)
    }

    pub fn status(&self) -> Value {
        let g = self.lock();
        let mut counts = BTreeMap::new();
        let mut failed = Vec::new();
        for (sha, it) in &g.items {
            let k = match &it.state {
                ItemState::Pending => "pending",
                ItemState::Uploading => "uploading",
                ItemState::Done => "done",
                ItemState::Failed(m) => {
                    failed.push(json!({"sha256": sha, "name": it.name, "error": m}));
                    "failed"
                }
            };
            *counts.entry(k).or_insert(0u64) += 1;
        }
        json!({
            "pending": counts.get("pending").copied().unwrap_or(0), "uploading": counts.get("uploading").copied().unwrap_or(0),
            "done": counts.get("done").copied().unwrap_or(0), "failed": counts.get("failed").copied().unwrap_or(0),
            "failed_items": failed, "paused": g.paused_reason, "last_error": g.last_error,
            "uploaded_bytes": g.uploaded_bytes, "deduplicated": g.deduplicated,
            "enabled": *self.enabled.lock().unwrap_or_else(|e| e.into_inner()),
        })
    }

    pub fn pause(&self, reason: Option<String>) {
        self.lock().paused_reason = reason;
        self.cv.notify_all();
    }

    pub fn resume(&self) {
        let mut g = self.lock();
        g.paused_reason = None;
        // Retry failed transport items on resume.
        let shas: Vec<String> = g.items.iter().filter(|(_, i)| matches!(i.state, ItemState::Failed(_))).map(|(s, _)| s.clone()).collect();
        for sha in shas {
            if let Some(i) = g.items.get_mut(&sha) {
                i.state = ItemState::Pending;
                i.attempts = 0;
            }
            if !g.queue.contains(&sha) {
                g.queue.push_back(sha);
            }
        }
        drop(g);
        self.cv.notify_all();
    }

    fn next_job(&self) -> String {
        let mut g = self.lock();
        loop {
            let enabled = *self.enabled.lock().unwrap_or_else(|e| e.into_inner());
            if enabled && g.paused_reason.is_none() {
                let now = Instant::now();
                if let Some(pos) = g.queue.iter().position(|s| g.items.get(s).map(|i| i.not_before.map(|t| t <= now).unwrap_or(true)).unwrap_or(true)) {
                    let sha = g.queue.remove(pos).unwrap();
                    if let Some(i) = g.items.get_mut(&sha) {
                        if i.state == ItemState::Pending {
                            i.state = ItemState::Uploading;
                            return sha;
                        }
                    }
                    continue;
                }
            }
            g = self.cv.wait_timeout(g, Duration::from_secs(2)).unwrap_or_else(|e| e.into_inner()).0;
        }
    }

    fn finish(&self, sha: &str, result: Result<bool, RemoteError>) {
        let mut g = self.lock();
        let size = g.items.get(sha).map(|i| i.size).unwrap_or(0);
        match result {
            Ok(created) => {
                let _ = std::fs::write(marker(&self.dir, sha), b"");
                if created {
                    g.uploaded_bytes += size;
                } else {
                    g.deduplicated += 1;
                }
                if let Some(i) = g.items.get_mut(sha) {
                    i.state = ItemState::Done;
                }
                g.last_error = None;
            }
            Err(e) => {
                let msg = e.to_json()["error"].as_str().unwrap_or("error").to_string();
                g.last_error = Some(msg.clone());
                let permanent = matches!(&e, RemoteError::Status(422, _) | RemoteError::Status(413, _) | RemoteError::Status(403, _));
                if e.is_auth() {
                    g.paused_reason = Some("Требуется вход на сервер".into());
                }
                let mut requeue = false;
                if let Some(i) = g.items.get_mut(sha) {
                    i.attempts += 1;
                    if permanent {
                        i.state = ItemState::Failed(msg);
                    } else {
                        i.state = ItemState::Pending;
                        let backoff = Duration::from_secs((5u64 << i.attempts.min(6)).min(300));
                        i.not_before = Some(Instant::now() + backoff);
                        requeue = true;
                    }
                }
                if requeue {
                    g.queue.push_back(sha.to_string());
                }
                if matches!(e, RemoteError::Status(422, _)) {
                    // The server computed a different hash: our cached copy is corrupt. Drop it; re-import recreates it.
                    let _ = std::fs::remove_file(blob_path(&self.dir, sha));
                }
            }
        }
        drop(g);
        self.cv.notify_all();
    }

    /// Oldest uploaded files go first; pending ones are kept whatever the budget.
    pub fn evict(&self) {
        let max = *self.max_bytes.lock().unwrap_or_else(|e| e.into_inner());
        let g = self.lock();
        let total: u64 = g.items.values().map(|i| i.size).sum();
        if total <= max {
            return;
        }
        let mut done: Vec<(std::time::SystemTime, String, u64)> = g
            .items
            .iter()
            .filter(|(_, i)| i.state == ItemState::Done)
            .map(|(s, i)| (std::fs::metadata(blob_path(&self.dir, s)).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH), s.clone(), i.size))
            .collect();
        drop(g);
        done.sort();
        let mut excess = total - max;
        let mut g = self.lock();
        for (_, sha, size) in done {
            if excess == 0 {
                break;
            }
            let _ = std::fs::remove_file(blob_path(&self.dir, &sha));
            let _ = std::fs::remove_file(marker(&self.dir, &sha));
            g.items.remove(&sha);
            excess = excess.saturating_sub(size);
        }
    }
}

/// Starts `n` upload workers. `ca` returns the configured extra CA path at call time.
pub fn start_workers(q: Arc<SyncQueue>, remote: Arc<Remote>, ca: Arc<dyn Fn() -> Option<PathBuf> + Send + Sync>, n: usize) {
    for i in 0..n {
        let (q, remote, ca) = (q.clone(), remote.clone(), ca.clone());
        std::thread::Builder::new()
            .name(format!("fsa-sync-{}", i))
            .spawn(move || loop {
                let sha = q.next_job();
                if remote.session().is_none() {
                    q.finish(&sha, Err(RemoteError::NotLoggedIn));
                    continue;
                }
                let ca_path = ca();
                let r = (|| -> Result<bool, RemoteError> {
                    if remote.fsa_exists(ca_path.as_deref(), &sha)? {
                        return Ok(false);
                    }
                    let p = blob_path(&q.dir, &sha);
                    let f = std::fs::File::open(&p).map_err(|e| RemoteError::Status(422, json!({"error": format!("cache: {}", e)})))?;
                    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
                    let v = remote.fsa_put(ca_path.as_deref(), &sha, len, std::io::BufReader::new(f))?;
                    Ok(v["status"] == "created")
                })();
                q.finish(&sha, r);
            })
            .expect("spawn sync worker");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("otd-sync-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn cache_is_content_addressed_and_survives_restart() {
        let d = tmp("cache");
        let q = SyncQueue::open(d.clone(), 1 << 30);
        let a = q.add("a.fsa", b"ABIF-one").unwrap();
        let b = q.add("copy-of-a.fsa", b"ABIF-one").unwrap();
        assert_eq!(a, b, "same bytes, same entry");
        q.add("b.fsa", b"ABIF-two").unwrap();
        assert!(q.add("x.txt", b"hello").is_err());
        assert_eq!(q.status()["pending"], 2);
        // Restart: queue rebuilt from disk.
        drop(q);
        let q = SyncQueue::open(d.clone(), 1 << 30);
        assert_eq!(q.status()["pending"], 2);
        q.finish(&a, Ok(true));
        drop(q);
        let q = SyncQueue::open(d.clone(), 1 << 30);
        assert_eq!(q.status()["done"], 1);
        assert_eq!(q.status()["pending"], 1);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn eviction_never_drops_unsent_files() {
        let d = tmp("evict");
        let q = SyncQueue::open(d.clone(), 10);
        let a = q.add("a.fsa", b"ABIF-aaaaaaaa").unwrap();
        q.finish(&a, Ok(true));
        let b = q.add("b.fsa", b"ABIF-bbbbbbbb").unwrap();
        q.evict();
        assert!(!blob_path(&d, &a).exists(), "uploaded file evicted");
        assert!(blob_path(&d, &b).exists(), "pending file kept over budget");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn auth_errors_pause_and_transport_errors_back_off() {
        let d = tmp("errors");
        let q = SyncQueue::open(d.clone(), 1 << 30);
        let a = q.add("a.fsa", b"ABIF-1").unwrap();
        q.finish(&a, Err(RemoteError::Transport("connection refused".into())));
        let s = q.status();
        assert_eq!(s["pending"], 1);
        assert!(s["last_error"].as_str().unwrap().contains("недоступен"));
        q.finish(&a, Err(RemoteError::NotLoggedIn));
        assert!(q.status()["paused"].is_string());
        q.finish(&a, Err(RemoteError::Status(422, json!({"error": "hash_mismatch"}))));
        assert_eq!(q.status()["failed"], 1);
        assert!(!blob_path(&d, &a).exists(), "corrupt cache copy removed");
        let _ = std::fs::remove_dir_all(d);
    }
}
