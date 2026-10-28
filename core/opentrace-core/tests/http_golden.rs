//! Replays HTTP exchanges recorded from the original Python server (`tests/golden/http.jsonl.gz`) against
//! `server::App`. Tokens, ports, temp paths, UUIDs and timestamps are normalised on both sides.
use flate2::read::GzDecoder;
use opentrace_core::server::{App, Config, Request};
use serde_json::Value;
use std::io::Read;

mod common;
use common::{b64, mask, replace};

#[test]
fn http_exchanges_match_python_server() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("golden");
    let mut text = String::new();
    GzDecoder::new(std::fs::File::open(dir.join("http.jsonl.gz")).unwrap()).read_to_string(&mut text).unwrap();
    let tmp = std::env::temp_dir().join(format!("opentrace-http-golden-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let port = 18101u16;
    let cfg = Config {
        port,
        projects_dir: Some(tmp.join("p")),
        security_dir: Some(tmp.join("s")),
        ..Config::default()
    };
    let app = App::new(&cfg).expect("app");
    let token = app.token.clone();
    let tmp_text = tmp.to_string_lossy().into_owned();
    let norm = |data: &[u8]| -> Vec<u8> {
        let d = replace(data, token.as_bytes(), b"@TOKEN@");
        let d = replace(&d, port.to_string().as_bytes(), b"@PORT@");
        let d = replace(&d, tmp_text.as_bytes(), b"@TMP@");
        mask(&d)
    };
    let (mut bad, mut n) = (Vec::new(), 0);
    for line in text.lines() {
        let r: Value = serde_json::from_str(line).unwrap();
        let fill = |s: &str| s.replace("@TOKEN@", &token).replace("@PORT@", &port.to_string());
        let headers = r["headers"].as_object().unwrap().iter().map(|(k, v)| (k.to_lowercase(), fill(v.as_str().unwrap()))).collect();
        let req = Request { method: r["method"].as_str().unwrap().into(), target: r["target"].as_str().unwrap().into(), headers, body: b64(r["body"].as_str().unwrap()) };
        let resp = app.handle(&req);
        n += 1;
        let want = b64(r["resp"].as_str().unwrap());
        let got = norm(&resp.body);
        let ctype_ok = r["ctype"].as_str().map_or(true, |c| c == resp.kind);
        if resp.code as u64 != r["status"].as_u64().unwrap() || got != want || !ctype_ok {
            bad.push(format!("{} ({} -> {} vs {})", r["label"], r["target"], resp.code, r["status"]));
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    assert!(n > 100, "recorded exchanges missing: {}", n);
    assert!(bad.is_empty(), "HTTP mismatches: {:#?}", bad);
}
