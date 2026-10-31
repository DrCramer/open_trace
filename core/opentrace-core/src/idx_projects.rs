//! Port of `genemapper_open/idx_projects.py`: the project library of the desktop-style workspace.
//!
//! A project is a folder `<root>/<slug>/` with `project.json` and `samples/<id>.fsa` + `samples/<id>.json.gz`.
//! Known deviations from CPython (all on hand-corrupted files only): `project.json` is read as UTF-8 /
//! UTF-8-with-BOM (Python would also sniff UTF-16/32), integers above 64 bit read as floats (see `pyjson`).
use crate::kit_definition::{py_float_repr, py_json_dumps};
use crate::matrix::{b64decode_strict, b64encode};
use crate::pullup_quality::{py_eq, truthy};
use crate::pyjson::{loads, LoadErr};
use serde_json::{json, Map, Value};
use std::cell::Cell;
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

pub const SCHEMA: i64 = 1;
pub const MAX_META_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_SAMPLE_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_FSA_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_PACKAGE_BYTES: usize = 2 * 1024 * 1024 * 1024;
pub const EXPORT_FORMAT: &str = "OpenTrace project export";
const DEFAULT_GROUP: &str = "GeneMapper ID-X Security Group";

/// Error classes the HTTP layer distinguishes.
#[derive(Debug, Clone, PartialEq)]
pub enum LErr {
    /// ValueError / TypeError / KeyError / RecursionError: HTTP 400 with the message.
    Value(String),
    /// `AccessDenied`: HTTP 403.
    Denied(String),
    /// Any other exception (OSError, BadZipFile, AttributeError, ...): HTTP 500 with `Type: message`.
    Other(String),
}

pub type LR<T> = Result<T, LErr>;

fn verr<T>(m: impl Into<String>) -> LR<T> {
    Err(LErr::Value(m.into()))
}

fn io(e: std::io::Error) -> LErr {
    LErr::Other(format!("OSError: {}", e))
}

/// `access(op, security_group, owner, check)`: raises `Denied` (or with `check` returns false) when not allowed.
pub type Access<'a> = Option<&'a dyn Fn(&str, &Value, &Value, bool) -> LR<bool>>;

// ---------- clock ----------
thread_local! {
    static FAKE_CLOCK: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Test hook: with `on`, `strftime_now` yields `2024-01-01T00:00:SS` with SS a counter (Python harness patches
/// `time.strftime` identically).
pub fn set_fake_clock(on: bool) {
    FAKE_CLOCK.with(|c| c.set(if on { Some(0) } else { None }));
}

pub fn strftime_now() -> String {
    if let Some(n) = FAKE_CLOCK.with(|c| c.get()) {
        FAKE_CLOCK.with(|c| c.set(Some(n + 1)));
        return format!("2024-01-01T00:{:02}:{:02}", (n / 60) % 60, n % 60);
    }
    let mut t: libc::time_t = 0;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe {
        libc::time(&mut t);
        libc::localtime_r(&t, &mut tm);
    }
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

// ---------- validation helpers ----------
fn name_ok(s: &str) -> bool {
    let n = s.chars().count();
    (1..=120).contains(&n)
        && s.chars().all(|c| !((c as u32) < 0x20 || "<>:\"/\\|?*".contains(c)))
}

fn check_name(v: &Value) -> LR<String> {
    if let Value::String(s) = v {
        if name_ok(s) && s.as_str() == s.trim() && s != "." && s != ".." {
            return Ok(s.clone());
        }
    }
    verr("Project name: 1..120 characters without control characters or < > : \" / \\ | ? *")
}

/// Python `re.match(r'^[A-Za-z0-9_-]{1,64}$', s)`; `$` also matches before one trailing newline.
fn sample_id_ok(s: &str) -> bool {
    let t = s.strip_suffix('\n').unwrap_or(s);
    let n = t.chars().count();
    (1..=64).contains(&n) && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Python `str(value)` for JSON values (containers only need to be recognisable as non-ids).
fn py_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Number(n) if n.is_f64() => py_float_repr(n.as_f64().unwrap()),
        Value::Number(n) => n.to_string(),
        Value::Array(_) => "[...]".into(),
        Value::Object(_) => "{...}".into(),
    }
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) if n.is_f64() => "float",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut in_bad = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            out.push(c);
            in_bad = false;
        } else if !in_bad {
            out.push('_');
            in_bad = true;
        }
    }
    let t: String = out.trim_matches('_').chars().take(40).collect();
    if t.is_empty() {
        "project".into()
    } else {
        t
    }
}

fn get(m: &Map<String, Value>, k: &str) -> Value {
    m.get(k).cloned().unwrap_or(Value::Null)
}

fn or_default(v: Value) -> Value {
    if truthy(&v) {
        v
    } else {
        Value::String(DEFAULT_GROUP.into())
    }
}

fn dumps_checked(v: &Value) -> LR<String> {
    fn has_nonfinite(v: &Value) -> bool {
        match v {
            Value::String(s) => s.starts_with("\u{10FFFF}nonfinite"),
            Value::Array(a) => a.iter().any(has_nonfinite),
            Value::Object(m) => m.values().any(has_nonfinite),
            _ => false,
        }
    }
    if has_nonfinite(v) {
        return verr("Out of range float values are not JSON compliant");
    }
    Ok(py_json_dumps(v))
}

fn load_err(e: LoadErr) -> LErr {
    match e {
        LoadErr::Decode(m) | LoadErr::Recursion(m) => LErr::Value(m),
        other => LErr::Value(other.message()),
    }
}

fn json_bytes(raw: &[u8]) -> LR<Value> {
    let raw = raw.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(raw);
    match std::str::from_utf8(raw) {
        Ok(t) => loads(t, false).map_err(load_err),
        Err(e) => verr(format!("'utf-8' codec can't decode byte: {}", e)),
    }
}

fn atomic_write(path: &Path, data: &[u8]) -> LR<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    let tag = format!("{:08x}", (std::process::id() as u64).wrapping_mul(0x9E3779B1) ^ (nanos as u64) ^ N.fetch_add(1, Ordering::Relaxed).wrapping_mul(0x85EBCA6B) & 0xffff_ffff);
    let fname = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = path.with_file_name(format!("{}.tmp{}", fname, tag));
    let mut f = fs::File::create(&tmp).map_err(io)?;
    f.write_all(data).map_err(io)?;
    drop(f);
    fs::rename(&tmp, path).map_err(io)
}

fn gzip_compress(data: &[u8]) -> Vec<u8> {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(6));
    enc.write_all(data).expect("vec write");
    enc.finish().expect("vec finish")
}

fn gzip_decompress(data: &[u8]) -> LR<Vec<u8>> {
    let mut out = Vec::new();
    flate2::read::MultiGzDecoder::new(data)
        .read_to_end(&mut out)
        .map_err(|e| LErr::Other(format!("BadGzipFile: {}", e)))?;
    Ok(out)
}

fn py_len(v: &Value) -> LR<usize> {
    match v {
        Value::String(s) => Ok(s.chars().count()),
        Value::Array(a) => Ok(a.len()),
        Value::Object(m) => Ok(m.len()),
        other => verr(format!("object of type '{}' has no len()", type_name(other))),
    }
}

/// Python `pathlib.PurePosixPath(s)` rendered back with `str()`.
fn path_str(s: &str) -> String {
    if s.is_empty() {
        return ".".into();
    }
    let lead = if s.starts_with("//") && !s.starts_with("///") {
        "//"
    } else if s.starts_with('/') {
        "/"
    } else {
        ""
    };
    let parts: Vec<&str> = s.split('/').filter(|p| !p.is_empty() && *p != ".").collect();
    let body = parts.join("/");
    if lead.is_empty() && body.is_empty() {
        ".".into()
    } else {
        format!("{}{}", lead, body)
    }
}

pub fn default_root() -> PathBuf {
    match std::env::var("OPENTRACE_PROJECTS") {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
            Path::new(&home).join("OpenTrace").join("projects")
        }
    }
}

pub struct ProjectLibrary {
    pub root: PathBuf,
}

fn guard_access(access: Access, op: &str, group: &Value, owner: &Value) -> LR<()> {
    if let Some(f) = access {
        f(op, group, owner, false)?;
    }
    Ok(())
}

impl ProjectLibrary {
    pub fn new(root: Option<&Path>) -> Self {
        ProjectLibrary { root: root.map(|p| p.to_path_buf()).unwrap_or_else(default_root) }
    }

    fn dirs(&self) -> LR<Vec<PathBuf>> {
        if !self.root.is_dir() {
            return Ok(vec![]);
        }
        let mut out = Vec::new();
        for e in fs::read_dir(&self.root).map_err(io)? {
            let d = e.map_err(io)?.path();
            if d.is_dir() && d.join("project.json").is_file() {
                out.push(d);
            }
        }
        Ok(out)
    }

    fn read_meta(&self, folder: &Path) -> LR<Map<String, Value>> {
        let raw = fs::read(folder.join("project.json")).map_err(io)?;
        if raw.len() > MAX_META_BYTES {
            return verr("project.json exceeds 8 MiB");
        }
        let meta = json_bytes(&raw)?;
        if let Value::Object(m) = meta {
            if m.get("schema").map(|s| py_eq(s, &json!(SCHEMA))).unwrap_or(false) && matches!(m.get("name"), Some(Value::String(_))) {
                return Ok(m);
            }
        }
        verr(format!("{}: not an OpenTrace project", folder.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()))
    }

    fn find(&self, name: &Value) -> LR<Option<PathBuf>> {
        let name = check_name(name)?;
        for folder in self.dirs()? {
            if let Ok(m) = self.read_meta(&folder) {
                if m.get("name").and_then(|n| n.as_str()) == Some(name.as_str()) {
                    return Ok(Some(folder));
                }
            }
        }
        Ok(None)
    }

    fn require(&self, name: &Value) -> LR<PathBuf> {
        match self.find(name)? {
            Some(f) => Ok(f),
            None => verr(format!("{} does not exist.", py_str(name))),
        }
    }

    fn guard(&self, folder: &Path, access: Access, op: &str) -> LR<()> {
        if access.is_some() {
            let meta = self.read_meta(folder)?;
            guard_access(access, op, &get(&meta, "security_group"), &get(&meta, "owner"))?;
        }
        Ok(())
    }

    pub fn list(&self, access: Access) -> LR<Value> {
        let mut out: Vec<(String, Value)> = Vec::new();
        for folder in self.dirs()? {
            let meta = match self.read_meta(&folder) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if let Some(f) = access {
                if !f("read", &get(&meta, "security_group"), &get(&meta, "owner"), true)? {
                    continue;
                }
            }
            let samples = match meta.get("samples") {
                Some(v) => py_len(v)?,
                None => 0,
            };
            let group = match meta.get("security_group") {
                Some(v) if truthy(v) => v.clone(),
                _ => Value::String(DEFAULT_GROUP.into()),
            };
            let name = meta["name"].as_str().unwrap().to_string();
            let row = json!({
                "name": name,
                "created": get(&meta, "created"),
                "modified": get(&meta, "modified"),
                "samples": samples,
                "comment": meta.get("comment").cloned().unwrap_or_else(|| Value::String(String::new())),
                "security_group": group,
                "owner": meta.get("owner").cloned().unwrap_or_else(|| Value::String(String::new())),
            });
            out.push((name.to_lowercase(), row));
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(Value::Array(out.into_iter().map(|(_, r)| r).collect()))
    }

    pub fn save_meta(&self, name: &Value, project: &Value, create: bool, overwrite: bool, access: Access, user_name: Option<&str>) -> LR<Value> {
        let name_s = check_name(name)?;
        let proj = match project {
            Value::Object(m) if matches!(m.get("samples"), Some(Value::Array(_))) => m,
            _ => return verr("Project with a samples list required"),
        };
        let samples = proj["samples"].as_array().unwrap();
        for s in samples {
            let ok = match s {
                Value::Object(m) => sample_id_ok(&py_str(m.get("id").unwrap_or(&Value::String(String::new())))),
                _ => false,
            };
            if !ok {
                return verr("Each sample needs an id of letters, digits, _ or -");
            }
        }
        let folder = self.find(name)?;
        if folder.is_some() && create && !overwrite {
            return verr(format!("{} has already been used, please enter a new name.", name_s));
        }
        let now = strftime_now();
        let group = or_default(get(proj, "security_group"));
        let user_v = |u: Option<&str>| -> Value {
            match u {
                Some(s) if !s.is_empty() => Value::String(s.into()),
                _ => Value::String(String::new()),
            }
        };
        let owner: Value;
        if let Some(f) = &folder {
            let old = self.read_meta(f)?;
            owner = match old.get("owner") {
                Some(v) if truthy(v) => v.clone(),
                _ => user_v(user_name),
            };
            if access.is_some() {
                guard_access(access, "update", &get(&old, "security_group"), &get(&old, "owner"))?;
                let old_group = or_default(get(&old, "security_group"));
                if !py_eq(&group, &old_group) {
                    guard_access(access, "create", &group, &Value::Null)?;
                }
            }
        } else {
            owner = user_v(user_name);
            guard_access(access, "create", &group, &Value::Null)?;
        }
        let (folder, created) = match folder {
            Some(f) => {
                let created = self.read_meta(&f)?.get("created").cloned().unwrap_or_else(|| Value::String(now.clone()));
                (f, created)
            }
            None => {
                fs::create_dir_all(&self.root).map_err(io)?;
                let base = slug(&name_s);
                let mut k = 1;
                let mut folder = self.root.join(&base);
                while folder.exists() {
                    k += 1;
                    folder = self.root.join(format!("{}_{}", base, k));
                }
                fs::create_dir_all(folder.join("samples")).map_err(io)?;
                (folder, Value::String(now.clone()))
            }
        };
        let mut meta = proj.clone();
        meta.insert("schema".into(), json!(SCHEMA));
        meta.insert("name".into(), Value::String(name_s.clone()));
        meta.insert("created".into(), created.clone());
        meta.insert("modified".into(), Value::String(now.clone()));
        meta.insert("security_group".into(), group.clone());
        meta.insert("owner".into(), owner.clone());
        let data = dumps_checked(&Value::Object(meta))?.into_bytes();
        if data.len() > MAX_META_BYTES {
            return verr("Project metadata exceeds 8 MiB");
        }
        atomic_write(&folder.join("project.json"), &data)?;
        let keep: Vec<String> = samples.iter().map(|s| py_str(&s["id"])).collect();
        for e in fs::read_dir(folder.join("samples")).map_err(io)? {
            let path = e.map_err(io)?.path();
            let fname = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let stem = fname.split('.').next().unwrap_or("").to_string();
            if !keep.contains(&stem) && !fname.ends_with(".tmp") {
                fs::remove_file(&path).map_err(io)?;
            }
        }
        Ok(json!({"name": name_s, "created": created, "modified": now, "security_group": group, "owner": owner}))
    }

    pub fn save_sample(&self, name: &Value, sample_id: &Value, fsa_b64: &Value, data: &Value, access: Access) -> LR<Value> {
        let folder = self.require(name)?;
        self.guard(&folder, access, "update")?;
        if !sample_id_ok(&py_str(sample_id)) {
            return verr("Invalid sample id");
        }
        let sid = py_str(sample_id);
        let samples = folder.join("samples");
        fs::create_dir_all(&samples).map_err(io)?;
        if !fsa_b64.is_null() {
            let s = match fsa_b64 {
                Value::String(s) => s,
                other => return verr(format!("argument should be a bytes-like object or ASCII string, not '{}'", type_name(other))),
            };
            let raw = b64decode_strict(s).map_err(LErr::Value)?;
            if raw.len() > MAX_FSA_BYTES {
                return verr("FSA exceeds 64 MiB");
            }
            atomic_write(&samples.join(format!("{}.fsa", sid)), &raw)?;
        }
        if !data.is_null() {
            let encoded = dumps_checked(data)?.into_bytes();
            if encoded.len() > MAX_SAMPLE_BYTES {
                return verr("Sample data exceeds 128 MiB");
            }
            atomic_write(&samples.join(format!("{}.json.gz", sid)), &gzip_compress(&encoded))?;
        }
        Ok(json!({"id": sample_id}))
    }

    pub fn open(&self, name: &Value, access: Access) -> LR<Value> {
        let folder = self.require(name)?;
        self.guard(&folder, access, "read")?;
        let mut meta = self.read_meta(&folder)?;
        let samples_dir = folder.join("samples");
        let mut stored: Vec<String> = Vec::new();
        if samples_dir.is_dir() {
            for e in fs::read_dir(&samples_dir).map_err(io)? {
                stored.push(e.map_err(io)?.file_name().to_string_lossy().into_owned());
            }
        }
        match meta.get_mut("samples") {
            None => {}
            Some(Value::Array(items)) => {
                for s in items.iter_mut() {
                    match s {
                        Value::Object(m) => {
                            let id = match m.get("id") {
                                Some(v) => py_str(v),
                                None => return verr("'id'"),
                            };
                            m.insert("has_fsa".into(), Value::Bool(stored.contains(&format!("{}.fsa", id))));
                            m.insert("has_data".into(), Value::Bool(stored.contains(&format!("{}.json.gz", id))));
                        }
                        Value::String(_) => return verr("string indices must be integers, not 'str'"),
                        Value::Array(_) => return verr("list indices must be integers or slices, not str"),
                        other => return verr(format!("'{}' object is not subscriptable", type_name(other))),
                    }
                }
            }
            Some(Value::Object(m)) => {
                if let Some(Value::String(_)) = m.keys().next().map(|k| Value::String(k.clone())) {
                    return verr("string indices must be integers, not 'str'");
                }
            }
            Some(Value::String(s)) => {
                if !s.is_empty() {
                    return verr("string indices must be integers, not 'str'");
                }
            }
            Some(other) => return verr(format!("'{}' object is not iterable", type_name(other))),
        }
        Ok(Value::Object(meta))
    }

    pub fn open_sample(&self, name: &Value, sample_id: &Value, access: Access) -> LR<Value> {
        let folder = self.require(name)?;
        self.guard(&folder, access, "read")?;
        if !sample_id_ok(&py_str(sample_id)) {
            return verr("Invalid sample id");
        }
        let sid = py_str(sample_id);
        let mut out = Map::new();
        out.insert("id".into(), sample_id.clone());
        out.insert("fsa_b64".into(), Value::Null);
        out.insert("data".into(), Value::Null);
        let fsa = folder.join("samples").join(format!("{}.fsa", sid));
        if fsa.is_file() {
            out.insert("fsa_b64".into(), Value::String(b64encode(&fs::read(&fsa).map_err(io)?)));
        }
        let blob = folder.join("samples").join(format!("{}.json.gz", sid));
        if blob.is_file() {
            let raw = gzip_decompress(&fs::read(&blob).map_err(io)?)?;
            if raw.len() > MAX_SAMPLE_BYTES {
                return verr("Sample data exceeds 128 MiB");
            }
            out.insert("data".into(), json_bytes(&raw)?);
        }
        Ok(Value::Object(out))
    }

    pub fn delete(&self, name: &Value, access: Access) -> LR<Value> {
        let folder = self.require(name)?;
        self.guard(&folder, access, "delete")?;
        fs::remove_dir_all(&folder).map_err(io)?;
        Ok(json!({"deleted": name}))
    }

    pub fn rename(&self, name: &Value, new_name: &Value, access: Access) -> LR<Value> {
        let folder = self.require(name)?;
        let new_s = check_name(new_name)?;
        self.guard(&folder, access, "update")?;
        if !py_eq(new_name, name) && self.find(new_name)?.is_some() {
            return verr(format!("{} has already been used, please enter a new name.", new_s));
        }
        let mut meta = self.read_meta(&folder)?;
        meta.insert("name".into(), Value::String(new_s.clone()));
        meta.insert("modified".into(), Value::String(strftime_now()));
        atomic_write(&folder.join("project.json"), dumps_checked(&Value::Object(meta))?.as_bytes())?;
        Ok(json!({"name": new_s, "old": name}))
    }

    pub fn save_as(&self, name: &Value, new_name: &Value, access: Access, user_name: Option<&str>) -> LR<Value> {
        let folder = self.require(name)?;
        self.guard(&folder, access, "read")?;
        let meta = self.read_meta(&folder)?;
        let project: Map<String, Value> =
            meta.iter().filter(|(k, _)| !["schema", "name", "created", "modified", "owner"].contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect();
        let info = self.save_meta(new_name, &Value::Object(project), true, false, access, user_name)?;
        let target = self.find(new_name)?.ok_or_else(|| LErr::Other("TypeError: unsupported operand type(s) for /: 'NoneType' and 'str'".into()))?;
        for e in fs::read_dir(folder.join("samples")).map_err(io)? {
            let path = e.map_err(io)?.path();
            let fname = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            if !fname.ends_with(".tmp") && path.is_file() {
                fs::copy(&path, target.join("samples").join(&fname)).map_err(io)?;
            }
        }
        Ok(info)
    }

    // ---------- export / import ----------
    pub fn export(&self, name: &Value, settings: bool, sample_files: bool, access: Access) -> LR<Value> {
        use zip::write::SimpleFileOptions;
        use zip::CompressionMethod;
        let folder = self.require(name)?;
        self.guard(&folder, access, "read")?;
        let mut meta = self.read_meta(&folder)?;
        if !settings {
            meta.shift_remove("snapshots");
        }
        let mut buf = Cursor::new(Vec::new());
        let zerr = |e: zip::result::ZipError| LErr::Other(format!("ZipError: {}", e));
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            let manifest = json!({"format": EXPORT_FORMAT, "version": 1, "name": name, "exported": strftime_now(),
                                   "analysis_settings": settings, "sample_files": sample_files});
            z.start_file("opentrace-project.json", deflated).map_err(zerr)?;
            z.write_all(py_json_dumps(&manifest).as_bytes()).map_err(io)?;
            z.start_file("project.json", deflated).map_err(zerr)?;
            z.write_all(dumps_checked(&Value::Object(meta.clone()))?.as_bytes()).map_err(io)?;
            let empty = Value::Array(vec![]);
            let samples = match meta.get("samples") {
                None => &empty,
                Some(v) => v,
            };
            let items: Vec<Value> = match samples {
                Value::Array(a) => a.clone(),
                Value::Null | Value::Bool(_) | Value::Number(_) => return verr(format!("'{}' object is not iterable", type_name(samples))),
                _ => return verr("string indices must be integers, not 'str'"),
            };
            let mut seen: Vec<String> = Vec::new();
            for s in &items {
                let sid = match s {
                    Value::Object(m) => match m.get("id") {
                        Some(v) => py_str(v),
                        None => return verr("'id'"),
                    },
                    Value::String(_) => return verr("string indices must be integers, not 'str'"),
                    Value::Array(_) => return verr("list indices must be integers or slices, not str"),
                    other => return verr(format!("'{}' object is not subscriptable", type_name(other))),
                };
                if seen.contains(&sid) {
                    continue;
                }
                seen.push(sid.clone());
                let data = folder.join("samples").join(format!("{}.json.gz", sid));
                if data.is_file() {
                    z.start_file(format!("samples/{}.json.gz", sid), stored).map_err(zerr)?;
                    z.write_all(&fs::read(&data).map_err(io)?).map_err(io)?;
                }
                let fsa = folder.join("samples").join(format!("{}.fsa", sid));
                if sample_files && fsa.is_file() {
                    z.start_file(format!("samples/{}.fsa", sid), deflated).map_err(zerr)?;
                    z.write_all(&fs::read(&fsa).map_err(io)?).map_err(io)?;
                }
            }
            z.finish().map_err(zerr)?;
        }
        let data = buf.into_inner();
        let count = match meta.get("samples") {
            Some(v) => py_len(v)?,
            None => 0,
        };
        Ok(json!({"name": name, "package_b64": b64encode(&data), "bytes": data.len(), "samples": count}))
    }

    pub fn import_package(&self, package_b64: &Value, name: &Value, mode: &Value, access: Access, user_name: Option<&str>) -> LR<Value> {
        let mode_s = match mode.as_str() {
            Some(m) if m == "skip" || m == "overwrite" => m.to_string(),
            _ => return verr("mode must be skip or overwrite"),
        };
        let pkg = match package_b64 {
            Value::String(s) => s,
            _ => return verr("Project package required"),
        };
        let raw = b64decode_strict(pkg).map_err(LErr::Value)?;
        if raw.len() > MAX_PACKAGE_BYTES {
            return verr("Project package exceeds 2 GiB");
        }
        let mut z = match zip::ZipArchive::new(Cursor::new(raw)) {
            Ok(z) => z,
            Err(_) => return verr("Not an OpenTrace project export file"),
        };
        let mut member = |n: &str, limit: usize| -> LR<Option<Vec<u8>>> {
            let mut f = match z.by_name(n) {
                Ok(f) => f,
                Err(_) => return Ok(None),
            };
            if f.size() as usize > limit {
                return verr(format!("{}: too large", n));
            }
            let mut out = Vec::new();
            f.read_to_end(&mut out).map_err(|e| LErr::Other(format!("BadZipFile: {}", e)))?;
            Ok(Some(out))
        };
        let manifest = member("opentrace-project.json", 1 << 20)?;
        let manifest_ok = match manifest {
            None => false,
            Some(b) => match json_bytes(&b)? {
                Value::Object(m) => m.get("format").and_then(|f| f.as_str()) == Some(EXPORT_FORMAT),
                other => return Err(LErr::Other(format!("AttributeError: '{}' object has no attribute 'get'", type_name(&other)))),
            },
        };
        if !manifest_ok {
            return verr("Not an OpenTrace project export file");
        }
        let meta_raw = member("project.json", MAX_META_BYTES)?;
        let meta = match meta_raw {
            Some(b) if !b.is_empty() => json_bytes(&b)?,
            _ => Value::Null,
        };
        let mut meta = match meta {
            Value::Object(m) if matches!(m.get("samples"), Some(Value::Array(_))) => m,
            _ => return verr("Project package has no project.json"),
        };
        let target = if truthy(name) { name.clone() } else { get(&meta, "name") };
        check_name(&target)?;
        if self.find(&target)?.is_some() {
            return verr(format!("{} has already been used, please enter a new name.", py_str(&target)));
        }
        let mut changed = 0;
        let mut project: Map<String, Value> =
            meta.iter().filter(|(k, _)| !["schema", "name", "created", "modified", "owner"].contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect();
        if let Some(Value::Array(items)) = project.get_mut("samples") {
            for s in items.iter_mut() {
                match s {
                    Value::Object(m) => {
                        m.shift_remove("has_fsa");
                        m.shift_remove("has_data");
                        if mode_s == "overwrite" && m.get("status").and_then(|v| v.as_str()) == Some("analyzed") {
                            m.insert("status".into(), Value::String("changed".into()));
                            changed += 1;
                        }
                    }
                    other => return Err(LErr::Other(format!("AttributeError: '{}' object has no attribute 'pop'", type_name(other)))),
                }
            }
        }
        let project_v = Value::Object(project);
        let info = self.save_meta(&target, &project_v, true, false, access, user_name)?;
        let folder = self.find(&target)?.ok_or_else(|| LErr::Other("TypeError: NoneType / str".into()))?;
        let mut fsa_count = 0;
        let sample_list = project_v["samples"].as_array().unwrap().clone();
        for s in &sample_list {
            let sid = py_str(&s["id"]);
            if let Some(data) = member(&format!("samples/{}.json.gz", sid), MAX_SAMPLE_BYTES)? {
                gzip_decompress(&data)?;
                atomic_write(&folder.join("samples").join(format!("{}.json.gz", sid)), &data)?;
            }
            if let Some(fsa) = member(&format!("samples/{}.fsa", sid), MAX_FSA_BYTES)? {
                atomic_write(&folder.join("samples").join(format!("{}.fsa", sid)), &fsa)?;
                fsa_count += 1;
            }
        }
        let mut out = match info {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        out.insert("samples".into(), json!(sample_list.len()));
        out.insert("sample_files".into(), json!(fsa_count));
        out.insert("status_changed".into(), json!(changed));
        out.insert("mode".into(), Value::String(mode_s));
        let snaps = meta.remove("snapshots").filter(truthy).unwrap_or_else(|| json!({}));
        out.insert("snapshots".into(), snaps);
        Ok(Value::Object(out))
    }

    pub fn request(&self, request: &Value, access: Access, user_name: Option<&str>) -> LR<Value> {
        let req = match request {
            Value::Object(m) => m,
            _ => return verr("Project request must be an object"),
        };
        let g = |k: &str| get(req, k);
        let tf = |k: &str, default: bool| -> bool {
            match req.get(k) {
                Some(v) => truthy(v),
                None => default,
            }
        };
        let action = g("action");
        match action.as_str() {
            Some("list") => Ok(json!({"root": path_str(&self.root.to_string_lossy()), "projects": self.list(access)?})),
            Some("save_meta") => self.save_meta(&g("name"), &g("project"), tf("create", false), tf("overwrite", true), access, user_name),
            Some("save_sample") => self.save_sample(&g("name"), &g("id"), &g("fsa_b64"), &g("data"), access),
            Some("open") => self.open(&g("name"), access),
            Some("open_sample") => self.open_sample(&g("name"), &g("id"), access),
            Some("delete") => self.delete(&g("name"), access),
            Some("rename") => self.rename(&g("name"), &g("new_name"), access),
            Some("save_as") => self.save_as(&g("name"), &g("new_name"), access, user_name),
            Some("export") => self.export(&g("name"), tf("settings", true), tf("sample_files", false), access),
            Some("import") => {
                let mode = req.get("mode").cloned().unwrap_or_else(|| Value::String("skip".into()));
                self.import_package(&g("package_b64"), &g("new_name"), &mode, access, user_name)
            }
            _ => verr("action must be list, save_meta, save_sample, open, open_sample, delete, rename, save_as, export or import"),
        }
    }
}
