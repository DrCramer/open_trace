//! Golden regression: every corpus in `tests/golden` is a request file plus the output of the original
//! pure-Python reference implementation. The matching `examples/*_dump` program must reproduce it byte for byte.
use flate2::read::GzDecoder;
use std::io::Read;
use std::path::PathBuf;
use std::process::Command;

fn gunzip(path: &std::path::Path) -> Vec<u8> {
    let mut out = Vec::new();
    GzDecoder::new(std::fs::File::open(path).unwrap_or_else(|e| panic!("{}: {}", path.display(), e))).read_to_end(&mut out).unwrap();
    out
}

fn examples_dir() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    exe.parent().unwrap().parent().unwrap().join("examples")
}

#[test]
fn golden_corpora_match_python_reference() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("golden");
    let manifest = std::fs::read_to_string(dir.join("manifest.txt")).unwrap();
    let tmp = std::env::temp_dir().join(format!("opentrace-golden-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let mut failures = Vec::new();
    let mut checked = 0;
    for line in manifest.lines().filter(|l| !l.trim().is_empty()) {
        let mut parts = line.split_whitespace();
        let (name, example) = (parts.next().unwrap(), parts.next().unwrap());
        let needs_dir = parts.next() == Some("dir");
        let req = tmp.join(format!("{}.jsonl", name));
        std::fs::write(&req, gunzip(&dir.join(format!("{}.req.jsonl.gz", name)))).unwrap();
        let want = gunzip(&dir.join(format!("{}.ref.txt.gz", name)));
        let bin = examples_dir().join(example);
        let mut cmd = Command::new(&bin);
        cmd.arg(&req);
        if needs_dir {
            let d = tmp.join(format!("{}_dir", name));
            std::fs::create_dir_all(&d).unwrap();
            cmd.arg(&d);
        }
        let mut got = cmd.output().unwrap_or_else(|e| panic!("{}: {}", bin.display(), e)).stdout;
        let mut want = want;
        if name == "idx_projects" && cfg!(any(windows, target_os = "macos")) {
            // Case-insensitive file system: sessions whose project or sample names differ only by letter case cannot
            // reproduce the Linux layout (and saving such a sample is refused there on purpose, see W7 in
            // docs/desktop/BASELINE_AUDIT.md). Every other session is still compared byte for byte.
            let skip = case_colliding_sessions(&gunzip(&dir.join(format!("{}.req.jsonl.gz", name))));
            let total = gunzip(&dir.join(format!("{}.req.jsonl.gz", name))).split(|b| *b == b'\n').filter(|l| !l.is_empty()).count();
            assert!(skip.len() * 2 < total, "too many case-colliding sessions skipped: {:?}", skip);
            eprintln!("idx_projects: {} of {} sessions use case-only name differences; the other {} are compared", skip.len(), total, total - skip.len());
            got = drop_sessions(&got, &skip);
            want = drop_sessions(&want, &skip);
        }
        checked += 1;
        if got != want {
            let (g, w) = (String::from_utf8_lossy(&got), String::from_utf8_lossy(&want));
            let first = g.lines().zip(w.lines()).position(|(a, b)| a != b).unwrap_or(g.lines().count().min(w.lines().count()));
            failures.push(format!("{}: first differing line {}", name, first + 1));
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    assert!(failures.is_empty(), "golden mismatches: {:?}", failures);
    assert!(checked >= 20, "golden manifest unexpectedly small: {}", checked);
}

/// `slug` of idx_projects.rs (the project folder name).
fn folder_slug(name: &str) -> String {
    let mut out = String::new();
    let mut bad = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            out.push(c);
            bad = false;
        } else if !bad {
            out.push('_');
            bad = true;
        }
    }
    out.trim_matches('_').chars().take(40).collect()
}

/// Sessions (lines of the request corpus) that use two project folders, or two sample ids of one project, that differ
/// only by letter case: distinct files on Linux, one file on a case-insensitive file system.
fn case_colliding_sessions(req: &[u8]) -> std::collections::BTreeSet<usize> {
    use std::collections::{BTreeMap, BTreeSet};
    let mut out = BTreeSet::new();
    for (si, line) in String::from_utf8_lossy(req).lines().filter(|l| !l.trim().is_empty()).enumerate() {
        let ops: serde_json::Value = serde_json::from_str(line).unwrap_or(serde_json::Value::Null);
        let mut folders: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut samples: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for op in ops.as_array().into_iter().flatten() {
            let r = &op["req"];
            for key in ["name", "new_name"] {
                if let Some(n) = r[key].as_str() {
                    let slug = folder_slug(n);
                    folders.entry(slug.to_ascii_lowercase()).or_default().insert(slug);
                }
            }
            // Samples move between projects with save_as/rename/import, so any two ids of a session count.
            if let Some(id) = r.get("id").filter(|_| r["name"].is_string()) {
                let id = id.as_str().map(|s| s.to_string()).unwrap_or_else(|| id.to_string());
                samples.entry(id.to_ascii_lowercase()).or_default().insert(id);
            }
        }
        if folders.values().chain(samples.values()).any(|v| v.len() > 1) {
            out.insert(si);
        }
    }
    out
}

/// Removes the `IP <session> ...` lines of the given sessions from an ip_dump output.
fn drop_sessions(out: &[u8], skip: &std::collections::BTreeSet<usize>) -> Vec<u8> {
    let text = String::from_utf8_lossy(out);
    let mut kept = String::new();
    for l in text.split_inclusive('\n') {
        let si = l.split(' ').nth(1).and_then(|v| v.parse::<usize>().ok());
        if !(l.starts_with("IP ") && si.map_or(false, |s| skip.contains(&s))) {
            kept.push_str(l);
        }
    }
    kept.into_bytes()
}

/// Demo fixture generators (`demo.rs`): digests of every FSA and canonical JSON of every fixture.
#[test]
fn demo_fixtures_match_python() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("golden");
    let want = gunzip(&dir.join("demo.ref.txt.gz"));
    let got = Command::new(examples_dir().join("demo_dump")).output().unwrap().stdout;
    assert!(got == want, "demo fixtures differ from the Python reference");
}
