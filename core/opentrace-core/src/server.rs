//! Port of `genemapper_open/server.py::serve` and its request handler: loopback research UI over plain HTTP/1.0.
//! `App::handle` is socket-free (tests and the CLI call it directly); `serve` wraps it in a threaded TCP loop.
use crate::abif::MAX_FILE_BYTES;
use crate::idx_projects::{LErr, ProjectLibrary};
use crate::pydump::{dumps, UTF8};
use crate::pyjson;
use crate::pullup_quality::truthy;
use crate::security::{security_request, SecurityStore};
use crate::server_analyze::{analysis_version, analyze, parse_analysis_range, VERSION};
use crate::{demo, project_store::SErr};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashSet};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

pub const MAX_JSON_BYTES: usize = 1024 * 1024 * 1024;
const DEFAULT_GROUP: &str = "GeneMapper ID-X Security Group";
const IDX_FILES: [&str; 26] = [
    "project.html", "plot.html", "panels.html", "idx.css", "i18n.js", "ui.js", "kits.js", "project.js", "plot.js",
    "panels.js", "methods.js", "tables.js", "sizestd.js", "sizematch.js", "options.js", "reports.js", "report.js",
    "report.html", "print.js", "print.html", "profiles.js", "mixture.js", "security.js", "sampleviews.js",
    "plotsettings.js", "matrices.js",
];
const POST_PATHS: [&str; 21] = [
    "/api/analyze", "/api/demo", "/api/demo-sizing", "/api/demo-pipeline", "/api/str", "/api/demo-str", "/api/str-run",
    "/api/demo-str-run", "/api/project", "/api/controls", "/api/marker-quality", "/api/pullup-quality",
    "/api/import-gm-kit", "/api/analysis-method", "/api/idx-project", "/api/codis", "/api/profiles", "/api/mixture",
    "/api/security", "/api/sample-info", "/api/matrix",
];
const DEMO_VARIANTS: [&str; 22] = [
    "ordinary", "negative", "amel", "ladder", "ladder-rejected", "spike", "spike-unrequested", "spike-low-initial",
    "spike-low-adjusted", "sq-pass", "sq-check", "sq-fail", "sq-ladder-fail", "ladder-calls", "ladder-sample-filters",
    "morphology", "morphology-unrequested", "true-peak", "true-peak-excluded", "pullup", "pullup-flags-only",
    "pullup-unrequested",
];
const MARKER_QUALITY_VARIANTS: [&str; 15] = [
    "negative", "amel", "spike", "spike-unrequested", "spike-low-initial", "spike-low-adjusted", "ladder-calls",
    "ladder-sample-filters", "morphology", "morphology-unrequested", "true-peak", "true-peak-excluded", "pullup",
    "pullup-flags-only", "pullup-unrequested",
];

/// HTTP-level failure: 400 ValueError family, 403 AccessDenied, 500 anything else.
#[derive(Debug)]
pub enum HErr {
    Bad(String),
    Denied(String),
    Other(String),
}
impl From<String> for HErr {
    fn from(s: String) -> Self {
        HErr::Bad(s)
    }
}
impl From<&str> for HErr {
    fn from(s: &str) -> Self {
        HErr::Bad(s.to_string())
    }
}
impl From<LErr> for HErr {
    fn from(e: LErr) -> Self {
        match e {
            LErr::Value(m) => HErr::Bad(m),
            LErr::Denied(m) => HErr::Denied(m),
            LErr::Other(m) => HErr::Other(m),
        }
    }
}
impl From<SErr> for HErr {
    fn from(e: SErr) -> Self {
        HErr::Bad(e.msg)
    }
}
type HR<T> = Result<T, HErr>;

pub struct Request {
    pub method: String,
    pub target: String,
    /// Lower-cased names; first occurrence wins on lookup (like `email.message.Message.get`).
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}
impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

pub struct Response {
    pub code: u16,
    pub body: Vec<u8>,
    pub kind: String,
}

pub struct Config {
    pub port: u16,
    pub host: String,
    pub open_browser: bool,
    pub projects_dir: Option<PathBuf>,
    pub security_dir: Option<PathBuf>,
    pub allowed_hosts: Vec<String>,
    pub resources: Option<PathBuf>,
}
impl Default for Config {
    fn default() -> Self {
        Config { port: 8765, host: "127.0.0.1".into(), open_browser: false, projects_dir: None, security_dir: None, allowed_hosts: vec![], resources: None }
    }
}

pub struct App {
    pub token: String,
    allowed: HashSet<String>,
    library: ProjectLibrary,
    security: Mutex<SecurityStore>,
    page: Vec<u8>,
    idx_files: BTreeMap<String, Vec<u8>>,
    shipped_table_settings: Value,
    shipped_methods: PathBuf,
    pub port: u16,
}

/// Directory holding `web/` and `data/`: explicit, `OPENTRACE_RESOURCES`, next to the executable, or the source tree.
pub fn resource_root(explicit: Option<&Path>) -> Result<PathBuf, String> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(p) = explicit {
        candidates.push(p.to_path_buf());
    }
    if let Ok(p) = std::env::var("OPENTRACE_RESOURCES") {
        if !p.is_empty() {
            candidates.push(PathBuf::from(p));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("resources"));
            candidates.push(dir.to_path_buf());
        }
    }
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    candidates.push(manifest.join("../../resources"));
    candidates
        .into_iter()
        .find(|c| c.join("web").join("index.html").is_file())
        .ok_or_else(|| "OpenTrace resources (web/, data/) not found; set OPENTRACE_RESOURCES".to_string())
}

pub fn default_security_dir() -> PathBuf {
    match std::env::var("OPENTRACE_SECURITY") {
        Ok(p) if !p.is_empty() => PathBuf::from(p),
        _ => PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join("OpenTrace").join("security"),
    }
}

fn token_urlsafe(n: usize) -> String {
    let mut buf = vec![0u8; n];
    getrandom::getrandom(&mut buf).expect("system randomness");
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for c in buf.chunks(3) {
        let v = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        let chars = c.len() + 1;
        for i in 0..chars {
            out.push(A[((v >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

/// Python `Path.read_text` newline translation: `\r\n` and lone `\r` become `\n`.
fn universal_newlines(data: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if data[i] == b'\r' {
            out.push(b'\n');
            if data.get(i + 1) == Some(&b'\n') {
                i += 1;
            }
        } else {
            out.push(data[i]);
        }
        i += 1;
    }
    out
}

fn replace_bytes(data: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if data[i..].starts_with(from) {
            out.extend_from_slice(to);
            i += from.len();
        } else {
            out.push(data[i]);
            i += 1;
        }
    }
    out
}

// ---------------------------------------------------------------- query strings and Python number parsing

fn pct_decode(s: &str) -> String {
    let b = s.replace('+', " ");
    let b = b.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = ((b[i + 1] as char).to_digit(16), (b[i + 2] as char).to_digit(16)) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `urllib.parse.parse_qs` (blank values dropped).
pub fn parse_qs(query: &str) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let Some((k, v)) = pair.split_once('=') else { continue };
        if v.is_empty() {
            continue;
        }
        out.entry(pct_decode(k)).or_default().push(pct_decode(v));
    }
    out
}

fn first<'a>(p: &'a BTreeMap<String, Vec<String>>, k: &str) -> Option<&'a str> {
    p.get(k).and_then(|v| v.first()).map(|s| s.as_str())
}

pub(crate) fn py_float(s: &str) -> HR<Value> {
    match crate::profiles::py_float_parse(s) {
        Some(x) => Ok(serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)),
        None => Err(HErr::Bad(format!("could not convert string to float: {}", crate::pydump::str_repr(s)))),
    }
}

pub(crate) fn py_int(s: &str) -> HR<Value> {
    let bad = || HErr::Bad(format!("invalid literal for int() with base 10: {}", crate::pydump::str_repr(s)));
    let t = crate::profiles::strip(s);
    let (neg, digits) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    if digits.is_empty() || digits.starts_with('_') || digits.ends_with('_') || digits.contains("__") {
        return Err(bad());
    }
    let clean: String = digits.chars().filter(|c| *c != '_').collect();
    if !clean.chars().all(|c| c.is_ascii_digit()) {
        return Err(bad());
    }
    let n: i128 = clean.parse().map_err(|_| bad())?;
    let n = if neg { -n } else { n };
    i64::try_from(n).map(|v| json!(v)).map_err(|_| bad())
}

fn utf8_error_text(raw: &[u8], e: &std::str::Utf8Error) -> String {
    let at = e.valid_up_to();
    match e.error_len() {
        None => {
            let end = raw.len() - 1;
            if end == at {
                format!("'utf-8' codec can't decode byte 0x{:02x} in position {}: unexpected end of data", raw[at], at)
            } else {
                format!("'utf-8' codec can't decode bytes in position {}-{}: unexpected end of data", at, end)
            }
        }
        Some(1) => {
            let b = raw[at];
            let why = if (0x80..=0xc1).contains(&b) || b >= 0xf5 { "invalid start byte" } else { "invalid continuation byte" };
            format!("'utf-8' codec can't decode byte 0x{:02x} in position {}: {}", b, at, why)
        }
        Some(n) => format!("'utf-8' codec can't decode bytes in position {}-{}: invalid continuation byte", at, at + n - 1),
    }
}

/// Python `json.loads(bytes)`: encoding detection (BOMs, UTF-16/32 by NUL pattern), then decode.
fn loads_bytes(raw: &[u8]) -> HR<Value> {
    let text: String = if raw.starts_with(&[0xef, 0xbb, 0xbf]) {
        std::str::from_utf8(&raw[3..]).map_err(|e| HErr::Bad(utf8_error_text(&raw[3..], &e)))?.to_string()
    } else if raw.starts_with(&[0xff, 0xfe, 0, 0]) || raw.starts_with(&[0, 0, 0xfe, 0xff]) || (raw.len() >= 4 && ((raw[0] == 0 && raw[1] == 0 && raw[2] == 0 && raw[3] != 0) || (raw[0] != 0 && raw[1] == 0 && raw[2] == 0 && raw[3] == 0))) {
        let big = raw.starts_with(&[0, 0, 0xfe, 0xff]) || (raw[0] == 0 && !raw.starts_with(&[0xff, 0xfe, 0, 0]));
        let body = if raw.starts_with(&[0xff, 0xfe, 0, 0]) || raw.starts_with(&[0, 0, 0xfe, 0xff]) { &raw[4..] } else { raw };
        if body.len() % 4 != 0 {
            return Err(HErr::Bad(format!("'utf-32-{}' codec can't decode bytes in position {}-{}: truncated data", if big { "be" } else { "le" }, body.len() / 4 * 4, body.len() - 1)));
        }
        let mut s = String::new();
        for c in body.chunks(4) {
            let v = if big { u32::from_be_bytes([c[0], c[1], c[2], c[3]]) } else { u32::from_le_bytes([c[0], c[1], c[2], c[3]]) };
            s.push(char::from_u32(v).ok_or_else(|| HErr::Bad("'utf-32' codec can't decode: code point not in range(0x110000)".into()))?);
        }
        s
    } else if raw.starts_with(&[0xff, 0xfe]) || raw.starts_with(&[0xfe, 0xff]) || (raw.len() >= 2 && ((raw[0] == 0) != (raw[1] == 0))) {
        let big = raw.starts_with(&[0xfe, 0xff]) || (!raw.starts_with(&[0xff, 0xfe]) && raw[0] == 0);
        let body = if raw.starts_with(&[0xff, 0xfe]) || raw.starts_with(&[0xfe, 0xff]) { &raw[2..] } else { raw };
        if body.len() % 2 != 0 {
            return Err(HErr::Bad(format!("'utf-16-{}' codec can't decode byte 0x{:02x} in position {}: truncated data", if big { "be" } else { "le" }, body[body.len() - 1], body.len() - 1)));
        }
        let units: Vec<u16> = body.chunks(2).map(|c| if big { u16::from_be_bytes([c[0], c[1]]) } else { u16::from_le_bytes([c[0], c[1]]) }).collect();
        String::from_utf16(&units).map_err(|_| HErr::Bad("'utf-16' codec can't decode bytes: illegal encoding".into()))?
    } else {
        std::str::from_utf8(raw).map_err(|e| HErr::Bad(utf8_error_text(raw, &e)))?.to_string()
    };
    pyjson::loads(&text, false).map_err(|e| HErr::Bad(e.message()))
}

fn jget(m: &Map<String, Value>, k: &str) -> Value {
    m.get(k).cloned().unwrap_or(Value::Null)
}

fn json_body(v: &Value) -> Vec<u8> {
    dumps(v, UTF8).into_bytes()
}

// ------------------------------------------------------------------------------------------------ App

impl App {
    pub fn new(cfg: &Config) -> Result<App, String> {
        let root = resource_root(cfg.resources.as_deref())?;
        let token = token_urlsafe(24);
        let web = root.join("web");
        let rd = |p: PathBuf| std::fs::read(&p).map_err(|e| format!("{}: {}", p.display(), e));
        let page = replace_bytes(&universal_newlines(rd(web.join("index.html"))?), b"__SESSION_TOKEN__", token.as_bytes());
        let mut idx_files = BTreeMap::new();
        for name in IDX_FILES {
            let mut data = rd(web.join("idx").join(name))?;
            if ["project.html", "panels.html", "plot.html"].contains(&name) {
                data = replace_bytes(&data, b"__SESSION_TOKEN__", token.as_bytes());
            }
            idx_files.insert(name.to_string(), data);
        }
        let mut shipped: Vec<(String, String)> = Vec::new();
        let dir = root.join("data").join("shipped_table_settings");
        if let Ok(rdir) = std::fs::read_dir(&dir) {
            for e in rdir.flatten() {
                let p = e.path();
                if p.extension().map(|x| x == "xml").unwrap_or(false) {
                    let stem = p.file_stem().unwrap().to_string_lossy().into_owned();
                    let xml = String::from_utf8(universal_newlines(rd(p.clone())?)).map_err(|e| format!("{}: {}", p.display(), e))?;
                    shipped.push((stem, xml));
                }
            }
        }
        shipped.sort_by(|a, b| a.0.cmp(&b.0));
        let shipped_table_settings = Value::Array(shipped.into_iter().map(|(n, x)| json!({"name": n, "xml": x})).collect());
        let security_dir = cfg.security_dir.clone().unwrap_or_else(default_security_dir);
        let security = SecurityStore::open(&security_dir.to_string_lossy()).map_err(|e| lerr_text(&e))?;
        Ok(App {
            token,
            allowed: cfg.allowed_hosts.iter().map(|h| h.trim().to_lowercase()).filter(|h| !h.is_empty()).collect(),
            library: ProjectLibrary::new(cfg.projects_dir.as_deref()),
            security: Mutex::new(security),
            page,
            idx_files,
            shipped_table_settings,
            shipped_methods: root.join("data").join("shipped_methods.json"),
            port: cfg.port,
        })
    }

    pub fn projects_root(&self) -> &Path {
        &self.library.root
    }
    pub fn security_root(&self) -> PathBuf {
        self.security.lock().unwrap().root.clone()
    }

    fn valid_host(&self, req: &Request) -> bool {
        let header = req.header("host").unwrap_or("");
        if header == format!("127.0.0.1:{}", self.port) {
            return true;
        }
        let name = if header.matches(':').count() == 1 { header.rsplit_once(':').unwrap().0 } else { header };
        !self.allowed.is_empty() && self.allowed.contains(&name.to_lowercase())
    }

    pub fn handle(&self, req: &Request) -> Response {
        match req.method.as_str() {
            "GET" => self.get(req),
            "POST" => self.post(req),
            _ => Response { code: 501, body: format!("Unsupported method ({})", req.method).into_bytes(), kind: "text/plain; charset=utf-8".into() },
        }
    }

    fn json(code: u16, v: Value) -> Response {
        Response { code, body: json_body(&v), kind: "application/json; charset=utf-8".into() }
    }

    fn get(&self, req: &Request) -> Response {
        let html = "text/html; charset=utf-8";
        if !self.valid_host(req) {
            return Self::json(403, json!({"error": "Use the 127.0.0.1 URL printed by the server"}));
        }
        let path = req.target.as_str();
        let file = |body: &[u8], kind: &str| Response { code: 200, body: body.to_vec(), kind: kind.to_string() };
        if path == "/" {
            return file(&self.page, html);
        }
        if path == "/idx" || path == "/idx/" {
            return file(&self.idx_files["project.html"], html);
        }
        if let Some(name) = path.strip_prefix("/idx/") {
            if let Some(data) = self.idx_files.get(name) {
                let kind = match Path::new(name).extension().and_then(|e| e.to_str()) {
                    Some("html") => html,
                    Some("css") => "text/css; charset=utf-8",
                    _ => "text/javascript; charset=utf-8",
                };
                return file(data, kind);
            }
        }
        if path == "/favicon.ico" {
            return Response { code: 204, body: vec![], kind: "image/x-icon".into() };
        }
        if path == "/api/health" {
            return Self::json(200, json!({"status": "ok", "analysis_version": analysis_version(), "research_only": true}));
        }
        if path == "/api/shipped-table-settings" {
            return Self::json(200, json!({"settings": self.shipped_table_settings}));
        }
        Self::json(404, json!({"error": "Not found"}))
    }

    /// Everything of `do_POST` that needs the complete body. The caller has already read exactly `Content-Length` bytes.
    pub fn post(&self, req: &Request) -> Response {
        if !self.valid_host(req) || req.header("x-session-token") != Some(self.token.as_str()) {
            return Self::json(403, json!({"error": "Invalid local session"}));
        }
        let host = req.header("host");
        if let Some(origin) = req.header("origin") {
            if !origin.is_empty() {
                let h = host.unwrap_or("None");
                if origin != format!("http://{}", h) && origin != format!("https://{}", h) {
                    return Self::json(403, json!({"error": "Invalid origin"}));
                }
            }
        }
        let (path, query) = match req.target.split_once('?') {
            Some((p, q)) => (p, q.split_once('#').map(|x| x.0).unwrap_or(q)),
            None => (req.target.split_once('#').map(|x| x.0).unwrap_or(req.target.as_str()), ""),
        };
        if !POST_PATHS.contains(&path) {
            return Self::json(404, json!({"error": "Not found"}));
        }
        match self.dispatch(path, query, req) {
            Ok(body) => Response { code: 200, body, kind: "application/json; charset=utf-8".into() },
            Err(HErr::Bad(m)) => Self::json(400, json!({"error": m})),
            Err(HErr::Denied(m)) => Self::json(403, json!({"error": m, "access_denied": true})),
            Err(HErr::Other(m)) => Self::json(500, json!({"error": format!("{}: {}", path, m)})),
        }
    }

    fn security_guard(&self) -> std::sync::MutexGuard<'_, SecurityStore> {
        self.security.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn dispatch(&self, path: &str, query: &str, req: &Request) -> HR<Vec<u8>> {
        let raw = &req.body;
        let params = parse_qs(query);
        let ok = |v: Value| -> HR<Vec<u8>> { Ok(json_body(&v)) };
        match path {
            "/api/project" => return ok(crate::project_api::project_action(&loads_bytes(raw)?)?),
            "/api/controls" | "/api/marker-quality" | "/api/str-run" => {
                let kind = &path[5..];
                return json_task(kind, raw).map(|v| json_body(&v));
            }
            _ => {}
        }
        let ticket = req.header("x-user-ticket").unwrap_or("");
        match path {
            "/api/sample-info" => ok(crate::sample_info::sample_info(raw, first(&params, "filename").unwrap_or("sample.fsa"))?),
            "/api/security" => {
                let request = loads_bytes(raw)?;
                let mut store = self.security_guard();
                ok(security_request(&mut store, Some(ticket), &request)?)
            }
            "/api/idx-project" => {
                let request = loads_bytes(raw)?;
                let store = self.security_guard();
                let user = store.session_user(Some(ticket))?;
                let action = request.get("action").and_then(|a| a.as_str()).unwrap_or("");
                let is_obj = request.is_object();
                if is_obj && ["save_meta", "save_sample", "delete", "import", "rename", "save_as"].contains(&action) {
                    store.require(user.as_ref(), &["UPDATE_PROJECT"])?;
                } else if is_obj && ["list", "open", "open_sample", "export"].contains(&action) {
                    store.require(user.as_ref(), &["READ_PROJECT"])?;
                }
                let group_of = |g: &Value| if truthy(g) { g.clone() } else { json!(DEFAULT_GROUP) };
                let access = |op: &str, group: &Value, owner: &Value, check: bool| -> Result<bool, LErr> {
                    let g = group_of(group);
                    let allowed = store.object_allows(user.as_ref(), &g, op, owner);
                    if check {
                        return Ok(allowed);
                    }
                    if !allowed {
                        store.require_object(user.as_ref(), &g, op, owner)?;
                    }
                    Ok(true)
                };
                let name = user.as_ref().and_then(|u| u.get("name")).and_then(|n| n.as_str()).map(|s| s.to_string());
                ok(self.library.request(&request, Some(&access), name.as_deref())?)
            }
            "/api/codis" => ok(crate::codis::codis_request(&loads_bytes(raw)?)?),
            "/api/mixture" => ok(crate::mixture::mixture_request(&loads_bytes(raw)?)?),
            "/api/profiles" => ok(crate::profiles::profile_request(&loads_bytes(raw)?)?),
            "/api/matrix" => ok(crate::matrix::matrix_request(&loads_bytes(raw)?)?),
            "/api/analysis-method" => ok(crate::analysis_method::method_request(&loads_bytes(raw)?, &self.shipped_methods)?),
            "/api/import-gm-kit" => ok(crate::gm_kit_import::import_request(&loads_bytes(raw)?)?),
            "/api/pullup-quality" => {
                let request = loads_bytes(raw)?;
                let obj = request.as_object().ok_or("Pull-up quality request must be an object")?;
                ok(crate::pullup_quality::attach_pullup_quality(&jget(obj, "report"), &jget(obj, "definition"))?)
            }
            "/api/str" => {
                let request = loads_bytes(raw)?;
                let obj = request.as_object().ok_or("STR request must be a JSON object")?;
                let policy = policy_of(obj);
                let lq = jget(obj, "ladder_quality_definition");
                ok(crate::str_workflow::run_str_workflow(
                    &jget(obj, "sample"),
                    &jget(obj, "ladder"),
                    &jget(obj, "kit"),
                    &policy,
                    if lq.is_null() { None } else { Some(&lq) },
                )?)
            }
            "/api/demo-str-run" => ok(demo_str_run()?),
            "/api/demo-str" => ok(demo_str(first(&params, "variant").unwrap_or("ordinary"))?),
            _ => {
                let data: Vec<u8> = match path {
                    "/api/demo" => demo::demo_bytes(),
                    "/api/demo-sizing" => demo::sizing_demo_bytes(),
                    "/api/demo-pipeline" => demo::pipeline_demo_bytes(),
                    _ => raw.clone(),
                };
                let (filename, kwargs) = analyze_kwargs(&params)?;
                ok(analyze(&data, &filename, &kwargs)?)
            }
        }
    }
}

fn lerr_text(e: &LErr) -> String {
    match e {
        LErr::Value(m) | LErr::Denied(m) | LErr::Other(m) => m.clone(),
    }
}

fn policy_of(obj: &Map<String, Value>) -> String {
    match obj.get("missing_offset_policy") {
        None => "reject".to_string(),
        Some(Value::String(s)) => s.clone(),
        // a non-string can never be a valid policy; any invalid text reproduces the same rejection
        Some(_) => "\u{0}invalid".to_string(),
    }
}

/// Query string -> `analyze` keyword arguments, converted in the order of the Python call (first failure wins).
fn analyze_kwargs(p: &BTreeMap<String, Vec<String>>) -> HR<(String, Map<String, Value>)> {
    let s = |k: &str, d: &str| json!(first(p, k).unwrap_or(d));
    let opt = |k: &str| first(p, k).map(|v| json!(v)).unwrap_or(Value::Null);
    let mut kw = Map::new();
    kw.insert("threshold".into(), py_float(first(p, "threshold").unwrap_or("100"))?);
    kw.insert("distance".into(), py_int(first(p, "distance").unwrap_or("8"))?);
    kw.insert("anchors".into(), s("anchors", ""));
    kw.insert("sizing_mode".into(), s("sizing_mode", ""));
    kw.insert("standard_channel".into(), s("standard_channel", ""));
    kw.insert("standard_bp".into(), s("standard_bp", ""));
    kw.insert("max_residual_bp".into(), py_float(first(p, "max_residual_bp").unwrap_or("5"))?);
    kw.insert("standard_start".into(), py_int(first(p, "standard_start").unwrap_or("0"))?);
    kw.insert("standard_end".into(), match first(p, "standard_end") {
        Some(v) => py_int(v)?,
        None => Value::Null,
    });
    kw.insert("calibration_model".into(), s("calibration_model", "linear"));
    kw.insert("analysis_engine".into(), s("analysis_engine", "residual"));
    kw.insert("height_mode".into(), s("height_mode", "explicit"));
    kw.insert("quality_threshold".into(), py_float(first(p, "quality_threshold").unwrap_or("0.75"))?);
    kw.insert("num_extra_peaks".into(), py_int(first(p, "num_extra_peaks").unwrap_or("25"))?);
    kw.insert("spike_definition".into(), opt("spike_definition"));
    kw.insert("sizing_quality_definition".into(), opt("sizing_quality_definition"));
    kw.insert("pullup_definition".into(), opt("pullup_definition"));
    kw.insert("channel_thresholds".into(), opt("channel_thresholds"));
    kw.insert("detector".into(), opt("detector"));
    let range = parse_analysis_range(&opt("analysis_range"))?;
    kw.insert("analysis_range".into(), match range {
        Some((a, b)) => json!([a, b]),
        None => Value::Null,
    });
    kw.insert("sizing_override".into(), opt("sizing_override"));
    kw.insert("normalization".into(), s("normalization", "false"));
    kw.insert("matrix".into(), opt("matrix"));
    Ok((first(p, "filename").unwrap_or("synthetic.fsa").to_string(), kw))
}

/// Body of the heavy JSON endpoints (`_json_task`).
pub fn json_task(kind: &str, raw: &[u8]) -> HR<Value> {
    let request = loads_bytes(raw)?;
    let obj_of = |msg: &'static str| request.as_object().ok_or(HErr::Bad(msg.into()));
    match kind {
        "str-run" => {
            let obj = obj_of("Run STR request must be an object")?;
            let lq = jget(obj, "ladder_quality_definition");
            let policy = policy_of(obj);
            let opts = crate::run_workflow::RunOptions {
                ladder_quality_definition: &lq,
                missing_offset_policy: &policy,
                marker_gate: false,
                homozygous_min_height: json!(200.0),
                heterozygous_min_height: json!(100.0),
            };
            Ok(crate::run_workflow::run_str_run_workflow(&jget(obj, "sample"), &jget(obj, "ladders"), &jget(obj, "kit"), &opts)?)
        }
        "marker-quality" => {
            let obj = obj_of("Marker quality request must be an object")?;
            Ok(crate::marker_quality_workflow::attach_marker_quality(&jget(obj, "report"), &jget(obj, "definition"))?)
        }
        "controls" => {
            let obj = obj_of("Control request must be a JSON object")?;
            Ok(crate::control_workflow::attach_control_result(&jget(obj, "report"), &jget(obj, "definition"))?)
        }
        _ => Err(HErr::Bad(format!("Unknown task {}", kind))),
    }
}

fn analyze_fixture(bytes: &[u8], name: &str, settings: &Value) -> HR<Value> {
    let empty = Map::new();
    Ok(analyze(bytes, name, settings.as_object().unwrap_or(&empty))?)
}

fn demo_str_run() -> HR<Value> {
    let fx = demo::make_run_demo();
    let sample = analyze_fixture(&fx.fixture.sample_bytes, "invented-run-sample.fsa", &fx.fixture.settings)?;
    let mut ladders = Vec::new();
    for (id, data) in &fx.ladders {
        let a = analyze_fixture(data, id, &fx.fixture.settings)?;
        ladders.push(json!({"ladder_id": id, "analysis": crate::str_workflow::compact_analysis(&a)}));
    }
    let ladders = Value::Array(ladders);
    let opts = crate::run_workflow::RunOptions {
        ladder_quality_definition: &fx.ladder_quality_definition,
        missing_offset_policy: "reject",
        marker_gate: false,
        homozygous_min_height: json!(200.0),
        heterozygous_min_height: json!(100.0),
    };
    let report = crate::run_workflow::run_str_run_workflow(&sample, &ladders, &fx.fixture.kit, &opts)?;
    Ok(json!({"sample": sample, "ladders": ladders, "kit": fx.fixture.kit, "report": report,
        "ladder_quality_definition": fx.ladder_quality_definition, "marker_quality_definition": fx.marker_quality_definition}))
}

/// One `/api/demo-str` or `str-demo` variant: fixture, definitions and the attached workflow report.
pub struct StrDemo {
    pub fixture: demo::Fixture,
    pub quality_definition: Value,
    pub controls: Option<Value>,
    pub ladder_quality_definition: Option<Value>,
}

pub fn build_str_demo(variant: &str) -> HR<StrDemo> {
    if !DEMO_VARIANTS.contains(&variant) {
        return Err("Unknown invented STR demo variant".into());
    }
    let mut controls = None;
    let mut lq = if variant == "ladder" || variant == "ladder-rejected" {
        Some(demo::make_ladder_quality_demo_definition(variant == "ladder-rejected"))
    } else {
        None
    };
    let (mut fixture, mut quality);
    if variant == "amel" {
        let (f, q) = demo::make_cross_check_demo();
        fixture = f;
        quality = q;
    } else {
        fixture = demo::make_str_demo(&demo::StrOpts { include_spike: variant == "spike" || variant == "spike-unrequested", ..Default::default() });
        quality = demo::make_quality_demo_definition();
        if variant == "negative" {
            let (q, c) = demo::make_negative_demo_definitions();
            quality = q;
            controls = Some(c);
        }
        if variant == "spike" {
            fixture.settings["spike_definition"] = demo::make_spike_demo_definition();
        }
        if variant == "spike-low-initial" || variant == "spike-low-adjusted" {
            fixture = demo::make_spike_adjustment_demo(variant == "spike-low-adjusted");
        }
        if variant == "morphology" || variant == "morphology-unrequested" {
            fixture = demo::make_morphology_demo(variant == "morphology", 0);
        }
        if variant == "true-peak" || variant == "true-peak-excluded" {
            fixture = demo::make_true_peak_demo(variant == "true-peak-excluded", 0);
        }
        if variant.starts_with("pullup") {
            fixture = demo::make_pullup_demo(variant == "pullup", variant != "pullup-unrequested");
        }
        if variant.starts_with("sq-") {
            fixture = demo::make_post_sizing_demo(variant)?;
            quality["sample_sq"] = json!({"source": "post_sizing_quality"});
            if variant == "sq-ladder-fail" {
                let mut d = demo::make_ladder_quality_demo_definition(false);
                d["ladder_sq"] = json!({"source": "post_sizing_quality"});
                lq = Some(d);
            }
        }
        if variant == "ladder-calls" || variant == "ladder-sample-filters" {
            let (f, q) = demo::make_ladder_calling_demo(variant == "ladder-sample-filters");
            fixture = f;
            lq = Some(q);
        }
    }
    Ok(StrDemo { fixture, quality_definition: quality, controls, ladder_quality_definition: lq })
}

pub fn demo_marker_quality_attached(variant: &str) -> bool {
    MARKER_QUALITY_VARIANTS.contains(&variant) || variant.starts_with("sq-")
}

fn demo_str(variant: &str) -> HR<Value> {
    let d = build_str_demo(variant)?;
    let sample = analyze_fixture(&d.fixture.sample_bytes, "invented-sample.fsa", &d.fixture.settings)?;
    let ladder = analyze_fixture(&d.fixture.ladder_bytes, "invented-ladder.fsa", &d.fixture.settings)?;
    let mut report = crate::str_workflow::run_str_workflow(&sample, &ladder, &d.fixture.kit, "reject", d.ladder_quality_definition.as_ref())?;
    report["provenance"]["input_reports_are_caller_supplied"] = json!(false);
    if let Some(c) = &d.controls {
        report = crate::control_workflow::attach_control_result(&report, c)?;
    }
    if demo_marker_quality_attached(variant) {
        report = crate::marker_quality_workflow::attach_marker_quality(&report, &d.quality_definition)?;
    }
    let settings = &d.fixture.settings;
    let spike = match settings.get("spike_definition") {
        Some(v) => v.clone(),
        None if variant == "spike-unrequested" => demo::make_spike_demo_definition(),
        None => Value::Null,
    };
    Ok(json!({"sample": sample, "ladder": crate::str_workflow::compact_analysis(&ladder),
        "kit": d.fixture.kit, "report": report, "marker_quality_definition": d.quality_definition,
        "control_definition": d.controls, "demo_variant": variant,
        "ladder_quality_definition": d.ladder_quality_definition,
        "sizing_quality_definition": settings.get("sizing_quality_definition").cloned().unwrap_or(Value::Null),
        "pullup_definition": settings.get("pullup_definition").cloned().unwrap_or(Value::Null),
        "spike_definition": spike}))
}

// ------------------------------------------------------------------------------------------ transport

fn http_date() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (h, m, s) = (rem / 3600, rem % 3600 / 60, rem % 60);
    let wd = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][days.rem_euclid(7) as usize];
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if mo <= 2 { 1 } else { 0 };
    let mon = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][(mo - 1) as usize];
    format!("{}, {:02} {} {} {:02}:{:02}:{:02} GMT", wd, d, mon, y, h, m, s)
}

fn reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        413 => "Request Entity Too Large",
        431 => "Request Header Fields Too Large",
        501 => "Not Implemented",
        _ => "Internal Server Error",
    }
}

fn write_response(stream: &mut TcpStream, r: &Response) {
    let mut head = format!(
        "HTTP/1.0 {} {}\r\nServer: OpenTrace/{}\r\nDate: {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n",
        r.code, reason(r.code), VERSION, http_date(), r.kind, r.body.len()
    );
    head.push_str("Content-Security-Policy: default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; connect-src 'self'; object-src 'none'; frame-ancestors 'none'\r\n\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&r.body);
    let _ = stream.flush();
}

fn read_head(stream: &mut TcpStream) -> Option<(Vec<u8>, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let rest = buf.split_off(pos + 4);
            buf.truncate(pos);
            return Some((buf, rest));
        }
        if buf.len() > 65536 * 4 {
            return None;
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
}

fn handle_connection(app: &App, mut stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let Some((head, mut body)) = read_head(&mut stream) else { return };
    let text = String::from_utf8_lossy(&head).into_owned();
    let mut lines = text.split("\r\n");
    let mut parts = lines.next().unwrap_or("").split(' ');
    let (method, target) = (parts.next().unwrap_or("").to_string(), parts.next().unwrap_or("").to_string());
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':').map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string())))
        .collect();
    let mut req = Request { method, target, headers, body: vec![] };
    if req.method == "POST" {
        // Early rejections do not need the body; mirror the Python order (host/token/origin first, then size).
        let (path, _) = req.target.split_once('?').unwrap_or((req.target.as_str(), ""));
        let host_ok = app.valid_host(&req) && req.header("x-session-token") == Some(app.token.as_str());
        let origin_ok = match req.header("origin") {
            Some(o) if !o.is_empty() => {
                let h = req.header("host").unwrap_or("None");
                o == format!("http://{}", h) || o == format!("https://{}", h)
            }
            _ => true,
        };
        if host_ok && origin_ok && POST_PATHS.contains(&path) {
            let path = path.to_string();
            if req.header("expect").map(|e| e.eq_ignore_ascii_case("100-continue")).unwrap_or(false) {
                let _ = stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n");
            }
            let len_text = req.header("content-length").unwrap_or("0").to_string();
            let length = match py_int(&len_text) {
                Ok(v) => v.as_i64().unwrap_or(-1),
                Err(HErr::Bad(m)) => {
                    write_response(&mut stream, &App::json(400, json!({"error": m})));
                    return;
                }
                Err(_) => -1,
            };
            let limit = if path == "/api/analyze" || path == "/api/sample-info" { MAX_FILE_BYTES } else { MAX_JSON_BYTES } as i64;
            if length < 0 || length > limit {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                let mut left = length.max(0).min(2 * MAX_JSON_BYTES as i64) - body.len() as i64;
                let mut chunk = vec![0u8; 1 << 20];
                while left > 0 {
                    match stream.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => left -= n as i64,
                    }
                }
                let msg = format!("Request too large: {} bytes (limit {} MiB)", length, limit / (1024 * 1024));
                write_response(&mut stream, &App::json(413, json!({"error": msg})));
                return;
            }
            let length = length as usize;
            let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
            let mut chunk = vec![0u8; 1 << 20];
            while body.len() < length {
                match stream.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => body.extend_from_slice(&chunk[..n]),
                }
            }
            body.truncate(length);
            if body.len() != length {
                write_response(&mut stream, &App::json(400, json!({"error": "Incomplete request body"})));
                return;
            }
            req.body = body;
        }
    }
    let resp = app.handle(&req);
    write_response(&mut stream, &resp);
}

/// `serve(...)`: bind, print the banner, serve forever.
pub fn serve(cfg: Config) -> Result<(), String> {
    let mut cfg = cfg;
    let listener = TcpListener::bind((cfg.host.as_str(), cfg.port)).map_err(|e| format!("{}", e))?;
    cfg.port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let app = std::sync::Arc::new(App::new(&cfg)?);
    let shown = if cfg.host == "0.0.0.0" || cfg.host == "::" { "127.0.0.1" } else { cfg.host.as_str() };
    let url = format!("http://{}:{}/", shown, cfg.port);
    println!(
        "OpenTrace research UI: {}\nDesktop-style workspace: {}idx/\nProjects saved from the workspace go to: {}\nUsers and audit trail: {}\nCtrl+C to stop.",
        url, url, app.projects_root().display(), app.security_root().display()
    );
    let _ = std::io::stdout().flush();
    if cfg.open_browser {
        let _ = std::process::Command::new(if cfg!(target_os = "macos") { "open" } else if cfg!(windows) { "explorer" } else { "xdg-open" }).arg(&url).spawn();
    }
    for stream in listener.incoming().flatten() {
        let app = app.clone();
        std::thread::spawn(move || handle_connection(&app, stream));
    }
    Ok(())
}
