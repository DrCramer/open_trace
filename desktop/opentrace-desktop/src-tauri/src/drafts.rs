//! Local drafts of server projects.
//!
//! When saving a server project fails (lease lost, revision conflict, no connection …) the user's edits exist only in
//! the window's memory. The shim then saves a full copy of the open project into the **local** project library under
//! a new name, `<project> (черновик YYYY-MM-DD HHMM)`, through the core's own `save_meta`/`save_sample` — the same
//! format as any local project, opened with "Projects: local → Open Project". Nothing is written to the server and
//! nothing is merged automatically (no last-write-wins): the user decides what to do with the draft.
//!
//! The bridge lets `/api/idx-project` writes bypass the server workspace only for a draft name registered here by
//! `begin` (header `X-OT-Draft`), so the page cannot use the bypass to write arbitrary local projects in server mode.
//! Each draft is listed in `drafts.jsonl` in the data directory (project, server, reason, samples, time).
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct Drafts {
    index: PathBuf,
    open: Mutex<HashSet<String>>,
}

/// Characters the local library refuses in project names (see core `idx_projects::check_name`).
fn clean(name: &str) -> String {
    let s: String = name.chars().map(|c| if (c as u32) < 0x20 || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
    let s = s.trim().to_string();
    // Room for the suffix within the 120-character limit.
    s.chars().take(90).collect::<String>().trim().to_string()
}

pub fn draft_name(project: &str, t: &opentrace_core::localtime::LocalTm) -> String {
    let base = clean(project);
    let base = if base.is_empty() || base == "." || base == ".." { "Проект".to_string() } else { base };
    format!("{} (черновик {:04}-{:02}-{:02} {:02}{:02})", base, t.year + 1900, t.mon + 1, t.mday, t.hour, t.min)
}

impl Drafts {
    pub fn new(data_dir: &std::path::Path) -> Self {
        Drafts { index: data_dir.join("drafts.jsonl"), open: Mutex::new(HashSet::new()) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashSet<String>> {
        self.open.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Registers a new draft name (unique among existing local project names, `exists` tells).
    pub fn begin(&self, project: &str, exists: impl Fn(&str) -> bool) -> String {
        let base = draft_name(project, &opentrace_core::localtime::now());
        let mut name = base.clone();
        let mut n = 2;
        while exists(&name) || self.lock().contains(&name) {
            name = format!("{} {}", base, n);
            n += 1;
        }
        self.lock().insert(name.clone());
        name
    }

    pub fn is_open(&self, name: &str) -> bool {
        self.lock().contains(name)
    }

    pub fn finish(&self, draft: &str, entry: Value) -> Result<(), String> {
        if !self.lock().remove(draft) {
            return Err("unknown draft".into());
        }
        let mut line = serde_json::to_vec(&entry).map_err(|e| e.to_string())?;
        line.push(b'\n');
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&self.index).map_err(|e| e.to_string())?;
        f.write_all(&line).and_then(|_| f.sync_all()).map_err(|e| e.to_string())
    }

    pub fn list(&self) -> Vec<Value> {
        std::fs::read_to_string(&self.index)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .collect()
    }
}

/// Record kept in `drafts.jsonl` (no FSA, no genotypes: names, counts, reason).
pub fn entry(draft: &str, project: &str, server: Option<String>, reason: &str, samples: u64, with_fsa: u64, failed: u64) -> Value {
    json!({
        "draft": draft,
        "server_project": project,
        "server": server,
        "reason": reason.chars().take(500).collect::<String>(),
        "samples": samples,
        "samples_with_fsa": with_fsa,
        "samples_failed": failed,
        "created": opentrace_core::localtime::now().iso(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_valid_local_project_names_and_unique() {
        let t = opentrace_core::localtime::LocalTm { year: 126, mon: 10, mday: 3, hour: 20, min: 5, sec: 0 };
        assert_eq!(draft_name("Case 12/2026: A|B", &t), "Case 12_2026_ A_B (черновик 2026-11-03 2005)");
        assert_eq!(draft_name("  ", &t), "Проект (черновик 2026-11-03 2005)");
        assert!(draft_name(&"x".repeat(300), &t).chars().count() <= 120);
        let dir = std::env::temp_dir().join(format!("ot-drafts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let d = Drafts::new(&dir);
        let a = d.begin("P", |_| false);
        let b = d.begin("P", |n| n == a);
        assert_ne!(a, b);
        assert!(d.is_open(&a) && d.is_open(&b) && !d.is_open("P"));
        d.finish(&a, entry(&a, "P", None, "lease lost", 3, 2, 0)).unwrap();
        assert!(!d.is_open(&a));
        assert!(d.finish(&a, json!({})).is_err());
        assert_eq!(d.list().len(), 1);
        assert_eq!(d.list()[0]["samples_with_fsa"], 2);
        let _ = std::fs::remove_dir_all(dir);
    }
}
