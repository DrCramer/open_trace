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
        let got = cmd.output().unwrap_or_else(|e| panic!("{}: {}", bin.display(), e)).stdout;
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

/// Demo fixture generators (`demo.rs`): digests of every FSA and canonical JSON of every fixture.
#[test]
fn demo_fixtures_match_python() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("golden");
    let want = gunzip(&dir.join("demo.ref.txt.gz"));
    let got = Command::new(examples_dir().join("demo_dump")).output().unwrap().stdout;
    assert!(got == want, "demo fixtures differ from the Python reference");
}
