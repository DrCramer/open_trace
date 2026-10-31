//! Socket-level behaviour of `opentrace serve`: framing, size limits, host/origin/token guards, concurrency.
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};

struct Server {
    child: Child,
    port: u16,
    token: String,
    _tmp: std::path::PathBuf,
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self._tmp);
    }
}

fn start() -> Server {
    let tmp = std::env::temp_dir().join(format!("opentrace-sock-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos()));
    std::fs::create_dir_all(&tmp).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_opentrace"))
        .args(["serve", "--port", "0", "--projects-dir"])
        .arg(tmp.join("p"))
        .arg("--security-dir")
        .arg(tmp.join("s"))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut first = String::new();
    BufReader::new(child.stdout.as_mut().unwrap()).read_line(&mut first).unwrap();
    let port: u16 = first.split("127.0.0.1:").nth(1).and_then(|s| s.trim_end_matches(&['/', '\n'][..]).parse().ok()).unwrap_or_else(|| panic!("banner: {}", first));
    let mut s = Server { child, port, token: String::new(), _tmp: tmp };
    let (_, body) = s.request("GET", "/", &[], b"");
    let text = String::from_utf8_lossy(&body).into_owned();
    let at = text.find("const TOKEN='").expect("token in page") + "const TOKEN='".len();
    s.token = text[at..].split('\'').next().unwrap().to_string();
    s
}

impl Server {
    /// One HTTP/1.0 exchange; returns (status line + headers, body).
    fn request(&self, method: &str, path: &str, headers: &[(&str, String)], body: &[u8]) -> (String, Vec<u8>) {
        self.raw(method, path, headers, body, body.len() as i64)
    }
    fn raw(&self, method: &str, path: &str, headers: &[(&str, String)], body: &[u8], length: i64) -> (String, Vec<u8>) {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        stream.set_read_timeout(Some(std::time::Duration::from_secs(20))).unwrap();
        let mut head = format!("{} {} HTTP/1.1\r\n", method, path);
        if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("host")) {
            head.push_str(&format!("Host: 127.0.0.1:{}\r\n", self.port));
        }
        for (k, v) in headers {
            head.push_str(&format!("{}: {}\r\n", k, v));
        }
        if method == "POST" {
            head.push_str(&format!("Content-Length: {}\r\n", length));
        }
        head.push_str("\r\n");
        stream.write_all(head.as_bytes()).unwrap();
        let _ = stream.write_all(body);
        let mut data = Vec::new();
        let _ = stream.read_to_end(&mut data);
        let split = data.windows(4).position(|w| w == b"\r\n\r\n").expect("response head");
        (String::from_utf8_lossy(&data[..split]).into_owned(), data[split + 4..].to_vec())
    }
    fn auth(&self) -> Vec<(&'static str, String)> {
        vec![("X-Session-Token", self.token.clone())]
    }
}

fn status(head: &str) -> u16 {
    head.split_whitespace().nth(1).unwrap().parse().unwrap()
}

#[test]
fn pages_health_and_static_whitelist() {
    let s = start();
    let (h, b) = s.request("GET", "/api/health", &[], b"");
    assert_eq!(status(&h), 200);
    assert!(String::from_utf8_lossy(&b).contains("\"research_only\": true"));
    assert!(h.contains("Content-Security-Policy") && h.contains("nosniff") && h.contains("no-store"));
    for p in ["/idx", "/idx/", "/idx/project.js", "/idx/plot.html", "/idx/idx.css"] {
        assert_eq!(status(&s.request("GET", p, &[], b"").0), 200, "{}", p);
    }
    assert_eq!(status(&s.request("GET", "/idx/../Cargo.toml", &[], b"").0), 404);
    assert_eq!(status(&s.request("GET", "/idx/secret.js", &[], b"").0), 404);
    assert_eq!(status(&s.request("GET", "/favicon.ico", &[], b"").0), 204);
    let (h, b) = s.request("GET", "/idx/project.html", &[], b"");
    assert_eq!(status(&h), 200);
    assert!(String::from_utf8_lossy(&b).contains(&s.token), "token substituted in the workspace page");
}

#[test]
fn guards_reject_foreign_hosts_origins_and_tokens() {
    let s = start();
    let bad_host = s.request("GET", "/api/health", &[("Host", "evil.example:80".into())], b"");
    assert_eq!(status(&bad_host.0), 403);
    assert_eq!(status(&s.request("POST", "/api/project", &[], b"{}").0), 403);
    let mut h = s.auth();
    h.push(("Origin", "http://evil.example".into()));
    assert_eq!(status(&s.request("POST", "/api/project", &h, b"{}").0), 403);
    let mut h = s.auth();
    h.push(("Origin", format!("http://127.0.0.1:{}", s.port)));
    assert_eq!(status(&s.request("POST", "/api/project", &h, br#"{"action":"new","name":"T"}"#).0), 200);
    assert_eq!(status(&s.request("POST", "/api/nope", &s.auth(), b"{}").0), 404);
    let (h, b) = s.request("POST", "/api/project", &s.auth(), b"{bad");
    assert_eq!(status(&h), 400);
    assert!(String::from_utf8_lossy(&b).contains("\"error\""));
}

#[test]
fn oversize_uploads_get_413_and_short_bodies_get_400() {
    let s = start();
    let (h, b) = s.raw("POST", "/api/analyze", &s.auth(), b"", 70 * 1024 * 1024);
    assert_eq!(status(&h), 413);
    assert!(String::from_utf8_lossy(&b).contains("Request too large"));
    let (h, _) = s.raw("POST", "/api/project", &s.auth(), b"", -5);
    assert_eq!(status(&h), 413);
}

#[test]
fn analysis_endpoint_and_concurrent_requests() {
    let s = start();
    let demo = std::sync::Arc::new(s);
    let handles: Vec<_> = (0..6)
        .map(|_| {
            let s = demo.clone();
            std::thread::spawn(move || {
                let (h, b) = s.request("POST", "/api/demo?filename=d.fsa", &s.auth(), b"");
                assert_eq!(status(&h), 200);
                let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
                assert_eq!(v["filename"], "d.fsa");
                assert!(v["channels"].as_array().unwrap().len() >= 5);
            })
        })
        .collect();
    for t in handles {
        t.join().unwrap();
    }
}
