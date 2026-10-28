//! Replays CLI sessions recorded from the original Python `opentrace` command (`tests/golden/cli.jsonl.gz`):
//! exit code, stdout, stderr and the files the commands leave behind must agree.
use flate2::read::GzDecoder;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

mod common;
use common::{mask, replace};

fn walk(dir: &Path, base: &Path, out: &mut BTreeMap<String, String>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        if p.is_dir() {
            walk(&p, base, out);
        } else {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let rel = p.strip_prefix(base).unwrap().to_string_lossy().into_owned();
            let opaque = (name.ends_with(".sqlite") || name.ends_with(".json"))
                && !name.contains("report")
                && !["c.json", "pq.json", "size.json", "out_str.json", "run_out.json"].contains(&name.as_str());
            let digest = if opaque { "x".to_string() } else { Sha256::digest(std::fs::read(&p).unwrap()).iter().map(|b| format!("{:02x}", b)).collect() };
            out.insert(rel, digest);
        }
    }
}

fn norm(wd: &str, data: &[u8]) -> String {
    let d = replace(data, wd.as_bytes(), b"@WD@");
    String::from_utf8_lossy(&mask(&d)).into_owned()
}

#[test]
fn cli_sessions_match_python() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("golden").join("cli.jsonl.gz");
    let mut text = String::new();
    GzDecoder::new(std::fs::File::open(path).unwrap()).read_to_string(&mut text).unwrap();
    let wd = std::fs::canonicalize(std::env::temp_dir()).unwrap().join(format!("opentrace-cli-golden-{}", std::process::id()));
    std::fs::create_dir_all(&wd).unwrap();
    let wd_text = wd.to_string_lossy().into_owned();
    let mut bad = Vec::new();
    let mut n = 0;
    for line in text.lines() {
        let r: Value = serde_json::from_str(line).unwrap();
        let args: Vec<String> = r["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_string()).collect();
        let out = Command::new(env!("CARGO_BIN_EXE_opentrace")).args(&args).current_dir(&wd).output().unwrap();
        n += 1;
        let rc = out.status.code().unwrap_or(-1) as i64;
        let stdout = norm(&wd_text, &out.stdout);
        let mut stderr = norm(&wd_text, &out.stderr);
        if rc == 2 && stderr.contains("usage:") {
            let last = stderr.trim().lines().last().unwrap_or("").to_string();
            let tail = last.split("error:").last().unwrap_or("").trim().to_string();
            stderr = match tail.find(" (choose from") {
                Some(i) => tail[..i].trim_end().to_string(),
                None => tail,
            };
        }
        let mut tree = BTreeMap::new();
        walk(&wd, &wd, &mut tree);
        let want_tree: BTreeMap<String, String> = r["tree"].as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string())).collect();
        if rc != r["rc"].as_i64().unwrap() || stdout != r["stdout"].as_str().unwrap() || stderr != r["stderr"].as_str().unwrap() || tree != want_tree {
            bad.push(format!("opentrace {} (rc {} vs {})", args.join(" "), rc, r["rc"]));
        }
    }
    let _ = std::fs::remove_dir_all(&wd);
    assert!(n >= 60, "recorded sessions missing: {}", n);
    assert!(bad.is_empty(), "CLI mismatches: {:#?}", bad);
}
