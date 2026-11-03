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
            // The reference was recorded on a case-sensitive POSIX file system. Sessions whose reference file tree
            // holds two paths differing only by letter case (W7: saving such a sample is refused here on purpose,
            // see docs/desktop/BASELINE_AUDIT.md) or, on Windows, a name NTFS cannot store (control characters,
            // `<>:"|?*`, trailing dot/space, device names) cannot reproduce that layout. Every other session is still
            // compared byte for byte.
            let skip = unreproducible_sessions(&want, cfg!(windows));
            let total = gunzip(&dir.join(format!("{}.req.jsonl.gz", name))).split(|b| *b == b'\n').filter(|l| !l.is_empty()).count();
            assert!(skip.len() * 4 < total, "too many sessions skipped: {:?}", skip);
            eprintln!("idx_projects: sessions {:?} of {} need a case-sensitive POSIX file system; the other {} are compared", skip, total, total - skip.len());
            got = drop_sessions(&got, &skip);
            want = drop_sessions(&want, &skip);
        }
        checked += 1;
        if got != want {
            let (g, w) = (String::from_utf8_lossy(&got), String::from_utf8_lossy(&want));
            let first = g.lines().zip(w.lines()).position(|(a, b)| a != b).unwrap_or(g.lines().count().min(w.lines().count()));
            let cut = |l: Option<&str>| l.unwrap_or("<missing>").chars().take(400).collect::<String>();
            eprintln!("{} line {}:\n  got:  {}\n  want: {}", name, first + 1, cut(g.lines().nth(first)), cut(w.lines().nth(first)));
            failures.push(format!("{}: first differing line {}", name, first + 1));
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    assert!(failures.is_empty(), "golden mismatches: {:?}", failures);
    assert!(checked >= 20, "golden manifest unexpectedly small: {}", checked);
}

/// Sessions of an `ip_dump` reference whose file tree (`IP <s> <step> TREE [...]`) cannot exist on a
/// case-insensitive file system, or (with `windows`) holds a name Windows cannot store.
fn unreproducible_sessions(reference: &[u8], windows: bool) -> std::collections::BTreeSet<usize> {
    use std::collections::{BTreeMap, BTreeSet};
    let mut out = BTreeSet::new();
    for line in String::from_utf8_lossy(reference).lines() {
        let mut f = line.splitn(4, ' ');
        let (ip, si, _step, rest) = (f.next(), f.next(), f.next(), f.next().unwrap_or(""));
        let (Some("IP"), Some(Ok(si))) = (ip, si.map(|s| s.parse::<usize>())) else { continue };
        let Some(tree) = rest.strip_prefix("TREE ") else { continue };
        let tree: serde_json::Value = serde_json::from_str(tree).unwrap_or(serde_json::Value::Null);
        let mut seen: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut bad = false;
        for e in tree.as_array().into_iter().flatten() {
            if let Some(p) = e[1].as_str() {
                seen.entry(p.to_lowercase()).or_default().insert(p.to_string());
                bad |= windows && p.split('/').any(|c| !windows_name_ok(c));
            }
        }
        if bad || seen.values().any(|v| v.len() > 1) {
            out.insert(si);
        }
    }
    out
}

fn windows_name_ok(c: &str) -> bool {
    let stem = c.split('.').next().unwrap_or("").to_ascii_uppercase();
    let device = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || ((stem.starts_with("COM") || stem.starts_with("LPT")) && stem.len() == 4 && stem.as_bytes()[3].is_ascii_digit());
    !c.is_empty() && !device && !c.ends_with(['.', ' ']) && !c.chars().any(|ch| ch < ' ' || "<>:\"|?*\\".contains(ch))
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

#[test]
fn unreproducible_sessions_of_the_project_corpus() {
    // Pins what a case-insensitive / Windows run leaves out, so the skip list cannot grow unnoticed.
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("golden");
    let reference = gunzip(&dir.join("idx_projects.ref.txt.gz"));
    let case_only: Vec<usize> = unreproducible_sessions(&reference, false).into_iter().collect();
    let windows: Vec<usize> = unreproducible_sessions(&reference, true).into_iter().collect();
    assert_eq!(case_only, [7, 16, 20, 26, 42]);
    assert_eq!(windows, [7, 16, 20, 26, 42, 44]); // 44: sample id with a newline
    assert!(windows_name_ok("S1.json.gz") && !windows_name_ok("new\n.fsa") && !windows_name_ok("nul.txt") && !windows_name_ok("a."));
}
