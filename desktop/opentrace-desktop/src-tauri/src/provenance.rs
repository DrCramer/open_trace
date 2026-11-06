//! Analysis provenance for server projects.
//!
//! Every successful `/api/analyze` the UI runs through the desktop is noted here: SHA-256 of the FSA bytes that were
//! analysed, the exact settings the UI sent (query parameters, as strings, without the file name), the core's
//! analysis version and the SHA-256 of the core's response. When the UI then saves that sample into a project on the
//! server (`save_sample`, matched by the `source_sha256` inside the saved analysis), the record is posted to
//! `POST /api/v1/projects/{name}/analyses` under the project lease. The server hashes the settings canonically.
//!
//! Honesty rules: a record is only posted when it is unambiguous — if the same FSA was analysed with different
//! settings in this session, nothing is guessed and the sample is counted as `ambiguous`. The active laboratory profile
//! is attached only when the sent settings are exactly the ones its approved method and size standard produce
//! (`labprofile.rs`) and every kit the sample was genotyped with (seen in `/api/str-run`) was built from the profile's
//! approved kit, bin set, stutter and method (checked at `/api/import-gm-kit`); otherwise the record goes without a
//! profile and the mismatch is counted with its reason.
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;

const KEEP: usize = 4000;

#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub fsa_sha256: String,
    pub analysis_version: String,
    pub settings: Value,
    pub result_sha256: String,
}

#[derive(Default)]
struct Inner {
    by_fsa: HashMap<String, Vec<Record>>,
    order: VecDeque<String>,
    posted: HashSet<(String, String, String)>,
    recorded: u64,
    posted_count: u64,
    ambiguous: u64,
    failed: u64,
    last_error: Option<String>,
    profile_mismatch: u64,
    last_mismatch: Option<String>,
    /// Kit builds seen while a profile was active: normalised kit hash -> verdict against the profile.
    kits: HashMap<String, Result<(), String>>,
    /// Genotyping runs per FSA: the kit hashes used.
    genotyped: HashMap<String, Vec<String>>,
}

#[derive(Default)]
pub struct Ledger {
    inner: Mutex<Inner>,
}

pub fn sha256_hex(b: &[u8]) -> String {
    crate::sync::hex(&Sha256::digest(b))
}

/// Query string of an analyze request → settings object (strings as sent; `filename` and `slim`, which only shapes the
/// response, are not settings).
pub fn settings_from_query(query: Option<&str>) -> Value {
    let mut m = Map::new();
    if let Some(q) = query {
        for (k, v) in url::form_urlencoded::parse(q.as_bytes()) {
            if k != "filename" && k != "slim" {
                m.insert(k.into_owned(), Value::String(v.into_owned()));
            }
        }
    }
    Value::Object(m)
}

impl Ledger {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn record(&self, query: Option<&str>, fsa: &[u8], result: &[u8]) -> Record {
        let r = Record {
            fsa_sha256: sha256_hex(fsa),
            analysis_version: opentrace_core::server_analyze::analysis_version().to_string(),
            settings: settings_from_query(query),
            result_sha256: sha256_hex(result),
        };
        let mut g = self.lock();
        g.recorded += 1;
        let list = g.by_fsa.entry(r.fsa_sha256.clone()).or_default();
        if !list.iter().any(|x| x.settings == r.settings && x.result_sha256 == r.result_sha256) {
            list.push(r.clone());
        }
        g.order.push_back(r.fsa_sha256.clone());
        while g.order.len() > KEEP {
            if let Some(old) = g.order.pop_front() {
                if !g.order.contains(&old) {
                    g.by_fsa.remove(&old);
                }
            }
        }
        r
    }

    /// The single record for this FSA, or `Err(n)` with the number of distinct candidates (0 = never analysed here).
    pub fn lookup(&self, fsa_sha256: &str) -> Result<Record, usize> {
        let g = self.lock();
        let list = g.by_fsa.get(fsa_sha256).map(|v| v.as_slice()).unwrap_or(&[]);
        let distinct: Vec<&Record> = list.iter().fold(Vec::new(), |mut acc, r| {
            if !acc.iter().any(|x: &&Record| x.settings == r.settings) {
                acc.push(r);
            }
            acc
        });
        match distinct.len() {
            1 => Ok(list.last().cloned().unwrap()),
            n => Err(n),
        }
    }

    /// True the first time (project, sample, result) is seen; later saves of the same result are not re-posted.
    pub fn mark_posted(&self, project: &str, sample_id: &str, result_sha256: &str) -> bool {
        self.lock().posted.insert((project.to_string(), sample_id.to_string(), result_sha256.to_string()))
    }

    pub fn unmark_posted(&self, project: &str, sample_id: &str, result_sha256: &str) {
        self.lock().posted.remove(&(project.to_string(), sample_id.to_string(), result_sha256.to_string()));
    }

    pub fn note(&self, outcome: Result<(), String>, ambiguous: bool) {
        let mut g = self.lock();
        match outcome {
            Ok(()) if ambiguous => g.ambiguous += 1,
            Ok(()) => g.posted_count += 1,
            Err(e) => {
                g.failed += 1;
                g.last_error = Some(e);
            }
        }
    }

    pub fn note_kit_build(&self, kit_hash: String, verdict: Result<(), String>) {
        self.lock().kits.insert(kit_hash, verdict);
    }

    pub fn note_genotyping(&self, fsa_sha256: &str, kit_hash: String) {
        let mut g = self.lock();
        let v = g.genotyped.entry(fsa_sha256.to_string()).or_default();
        if !v.contains(&kit_hash) {
            v.push(kit_hash);
        }
    }

    /// `Ok` when every kit this FSA was genotyped with was checked against the active profile and matched (or it was
    /// not genotyped in this session); otherwise why not.
    pub fn genotyping_verdict(&self, fsa_sha256: &str) -> Result<(), String> {
        let g = self.lock();
        for k in g.genotyped.get(fsa_sha256).map(|v| v.as_slice()).unwrap_or(&[]) {
            match g.kits.get(k) {
                Some(Ok(())) => {}
                Some(Err(why)) => return Err(format!("генотипирование: {}", why)),
                None => return Err("генотипирование набором, собранным до выбора профиля (перезапустите OpenTrace Desktop и повторите анализ)".into()),
            }
        }
        Ok(())
    }

    pub fn note_profile_mismatch(&self, why: &str) {
        let mut g = self.lock();
        g.profile_mismatch += 1;
        g.last_mismatch = Some(why.to_string());
    }

    pub fn status(&self) -> Value {
        let g = self.lock();
        json!({"recorded": g.recorded, "posted": g.posted_count, "ambiguous": g.ambiguous, "failed": g.failed, "last_error": g.last_error,
               "profile_mismatch": g.profile_mismatch, "last_mismatch": g.last_mismatch})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_exclude_slim() {
        let s = settings_from_query(Some("threshold=50&slim=1&filename=a.fsa"));
        assert_eq!(s, json!({"threshold": "50"}));
    }

    #[test]
    fn settings_exclude_file_name_and_keep_strings() {
        let s = settings_from_query(Some("threshold=50&filename=a%20b.fsa&channel_thresholds=%7B%22DATA1%22%3A50%7D"));
        assert_eq!(s, json!({"threshold": "50", "channel_thresholds": "{\"DATA1\":50}"}));
    }

    #[test]
    fn ambiguous_when_one_file_was_analysed_with_different_settings() {
        let l = Ledger::default();
        let r1 = l.record(Some("threshold=50"), b"fsa", b"r1");
        assert_eq!(l.lookup(&r1.fsa_sha256).unwrap().settings, json!({"threshold": "50"}));
        l.record(Some("threshold=50"), b"fsa", b"r1"); // same again: still one
        assert!(l.lookup(&r1.fsa_sha256).is_ok());
        l.record(Some("threshold=80"), b"fsa", b"r2");
        assert_eq!(l.lookup(&r1.fsa_sha256), Err(2));
        assert_eq!(l.lookup(&sha256_hex(b"other")), Err(0));
        assert!(l.mark_posted("P", "1", "x") && !l.mark_posted("P", "1", "x"));
    }
}
