//! Port of `genemapper_open/security.py`: users, access rights, audit trail and electronic signatures.
//!
//! Documents are handled as JSON values so that key order and types round-trip like the Python dicts.
//! Known deviations (inputs that only a hand-edited or hostile document could produce): structural type
//! confusion inside imported sections is rejected up front with "Malformed section" instead of failing later with
//! a TypeError; the tail of an XML parse error message differs from expat's.
use crate::idx_projects::{LErr, LR};
use crate::pullup_quality::{py_eq, truthy};
use crate::pydump::{dumps, py_str, type_name, Opts, UTF8};
use crate::pyjson::loads;
use hmac::{Hmac, Mac};
use serde_json::{json, Map, Value};
use sha2::Sha256;
use std::cell::Cell;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const GM: &str = "GeneMapper ID-X";
pub const ACC: &str = "AdminToolAcc";
pub const AUDIT: &str = "AuditGUI";
pub const ESIG: &str = "ESig";
pub const DEFAULT_SECURITY_GROUP: &str = "GeneMapper ID-X Security Group";
const RIGHT_NAMES: [&str; 7] = ["read", "update", "create", "delete", "control", "owner_update", "owner_delete"];
const UPON_FAILURE: [&str; 3] = ["remain_active", "disable", "suspend"];
const AUDIT_STATES: [&str; 3] = ["on", "off", "silent"];
const CHALLENGE: &str = "By entering your UserID and Password, you are signing the action";
const DEFAULT_ITERATIONS: u32 = 120_000;
const ACCESS_DENIED: &str = "You do not have sufficient rights to perform this action.";

type CatEntry = (&'static str, &'static str, &'static [&'static str]);
pub const CATALOG: &[CatEntry] = &[
    (GM, "Project", &["READ_PROJECT", "UPDATE_PROJECT"]),
    (GM, "Analysis Method", &["READ_ANALYSIS_METHOD", "UPDATE_ANALYSIS_METHOD", "UPDATE_ANALYSIS_RANGE", "UPDATE_SIZING_RANGE", "SAVE_AS_ANALYSIS_METHOD"]),
    (GM, "Panel Manager", &["READ_PANEL", "UPDATE_PANEL"]),
    (GM, "Size Standard", &["READ_SIZE_STANDARD", "UPDATE_SIZE_STANDARD"]),
    (GM, "Matrix", &["READ_MATRIX", "UPDATE_MATRIX"]),
    (GM, "Profile Manager", &["READ_PROFILE_CONCORDANCE", "UPDATE_PROFILE_CONCORDANCE"]),
    (GM, "Administration", &["Remote Shutdown"]),
    (
        ACC,
        "General",
        &[
            "May run application",
            "Add User to User Group",
            "Associate User Groups with Security Groups",
            "Associate User Groups with Users",
            "Change Password Policies",
            "Import Security Data",
            "Export Security Data",
        ],
    ),
    (ACC, "User Groups", &["Create", "Modify", "Change Control Security Group", "Delete"]),
    (ACC, "Profiles", &["Create", "Modify", "Change Control Security Group", "Delete"]),
    (ACC, "Users", &["Create", "Modify", "Change Control Security Group", "Delete"]),
    (ACC, "Security Groups", &["Create", "Modify", "Change Control Security Group"]),
    (ESIG, "ESig Administration Tool", &["May run application"]),
    (AUDIT, "Audit Management", &["Backup Audit Records", "Restore Audit Records", "Delete Audit Records"]),
    (AUDIT, "Audit History Viewer", &["May run application"]),
    (AUDIT, "Map Configuration Tool", &["May run application", "Enable and disable maps", "Change auditing state"]),
];

pub fn functions() -> Vec<String> {
    CATALOG.iter().flat_map(|(p, m, fs)| fs.iter().map(move |f| format!("{}/{}/{}", p, m, f))).collect()
}
fn products() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (p, _, _) in CATALOG {
        if !out.iter().any(|x| x == p) {
            out.push(p.to_string());
        }
    }
    out
}
fn modules() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (p, m, _) in CATALOG {
        let k = format!("{}/{}", p, m);
        if !out.contains(&k) {
            out.push(k);
        }
    }
    out
}

fn function_key(name: &Value) -> LR<String> {
    if let Value::String(s) = name {
        let fs = functions();
        if fs.iter().any(|f| f == s) {
            return Ok(s.clone());
        }
        // _SHORT: first full key per bare function name; must belong to GeneMapper
        if let Some(k) = fs.iter().find(|f| f.rsplit('/').next() == Some(s.as_str())) {
            if k.starts_with(&format!("{}/", GM)) {
                return Ok(k.clone());
            }
        }
    }
    Err(LErr::Value(format!("Unknown function: {}", py_str(name))))
}

// ---------------------------------------------------------------- clock / randomness / hashing hooks
thread_local! {
    static FAKE_TIME: Cell<Option<i64>> = const { Cell::new(None) };
    static FAKE_RAND: Cell<Option<u64>> = const { Cell::new(None) };
    static ITERATIONS: Cell<u32> = const { Cell::new(DEFAULT_ITERATIONS) };
}

/// Test hook: fixed clock in microseconds since the Unix epoch (None = real clock).
pub fn set_fake_time(micros: Option<i64>) {
    FAKE_TIME.with(|c| c.set(micros));
}
/// Test hook: deterministic "random" bytes (counter based).
pub fn set_fake_rand(on: bool) {
    FAKE_RAND.with(|c| c.set(if on { Some(0) } else { None }));
}
pub fn set_iterations(n: u32) {
    ITERATIONS.with(|c| c.set(n));
}

fn now_micros() -> i64 {
    if let Some(t) = FAKE_TIME.with(|c| c.get()) {
        return t;
    }
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap();
    d.as_secs() as i64 * 1_000_000 + d.subsec_micros() as i64
}

fn rand_bytes(n: usize) -> Vec<u8> {
    if let Some(c) = FAKE_RAND.with(|c| c.get()) {
        FAKE_RAND.with(|x| x.set(Some(c + 1)));
        let be = c.to_be_bytes();
        let mut out = vec![0u8; n];
        for i in 0..n.min(8) {
            out[n - 1 - i] = be[7 - i];
        }
        return out;
    }
    let mut out = vec![0u8; n];
    getrandom::getrandom(&mut out).expect("os randomness");
    out
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{:02x}", x)).collect()
}
fn token_hex(n: usize) -> String {
    hex(&rand_bytes(n))
}
fn token_urlsafe(n: usize) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let b = rand_bytes(n);
    let mut out = String::new();
    for chunk in b.chunks(3) {
        let v = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let x = ((v[0] as u32) << 16) | ((v[1] as u32) << 8) | v[2] as u32;
        out.push(A[(x >> 18) as usize & 63] as char);
        out.push(A[(x >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(A[(x >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(A[x as usize & 63] as char);
        }
    }
    out
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `strftime('%Y-%m-%dT%H:%M:%SZ')` of a UTC instant (microseconds are dropped).
fn iso(micros: i64) -> String {
    let secs = micros.div_euclid(1_000_000);
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m, d, rem / 3600, (rem / 60) % 60, rem % 60)
}

/// `datetime.strptime(s, '%Y-%m-%dT%H:%M:%SZ')` as UTC microseconds.
fn parse_iso(v: &Value) -> LR<i64> {
    let s = match v {
        Value::String(s) => s,
        other => return Err(LErr::Value(format!("strptime() argument 1 must be str, not {}", type_name(other)))),
    };
    let c: Vec<char> = s.chars().collect();
    // alternatives per field, in the order of the regular expressions of `_strptime`
    fn digits(c: &[char], i: usize, n: usize) -> Option<i64> {
        if i + n > c.len() || !c[i..i + n].iter().all(|x| x.is_ascii_digit()) {
            return None;
        }
        Some(c[i..i + n].iter().collect::<String>().parse().unwrap())
    }
    let alts: [&[(usize, i64, i64)]; 6] = [
        &[(4, 0, 9999)],
        &[(2, 10, 12), (2, 1, 9), (1, 1, 9)],
        &[(2, 30, 31), (2, 10, 29), (2, 1, 9), (1, 1, 9), (2, 1, 9)],
        &[(2, 20, 23), (2, 0, 19), (1, 0, 9)],
        &[(2, 0, 59), (1, 0, 9)],
        &[(2, 60, 61), (2, 0, 59), (1, 0, 9)],
    ];
    // field i with width/range alternatives; the `0[1-9]`/`[0-1]\d`/`[0-5]\d` classes are encoded by ranges
    fn field(c: &[char], i: usize, f: usize, alts: &[&[(usize, i64, i64)]; 6]) -> Vec<(i64, usize)> {
        let mut out = Vec::new();
        match f {
            1 => {
                if let Some(v) = digits(c, i, 2) {
                    if (10..=12).contains(&v) {
                        out.push((v, 2));
                    }
                    if (1..=9).contains(&v) && c[i] == '0' {
                        out.push((v, 2));
                    }
                }
                if let Some(v) = digits(c, i, 1) {
                    if (1..=9).contains(&v) {
                        out.push((v, 1));
                    }
                }
            }
            2 => {
                if let Some(v) = digits(c, i, 2) {
                    if (30..=31).contains(&v) || (10..=29).contains(&v) || (c[i] == '0' && (1..=9).contains(&v)) {
                        out.push((v, 2));
                    }
                }
                if let Some(v) = digits(c, i, 1) {
                    if (1..=9).contains(&v) {
                        out.push((v, 1));
                    }
                }
                if i + 2 <= c.len() && c[i] == ' ' && c[i + 1].is_ascii_digit() && c[i + 1] != '0' {
                    out.push((c[i + 1].to_digit(10).unwrap() as i64, 2));
                }
            }
            3 => {
                if let Some(v) = digits(c, i, 2) {
                    if v <= 23 {
                        out.push((v, 2));
                    }
                }
                if let Some(v) = digits(c, i, 1) {
                    out.push((v, 1));
                }
            }
            4 => {
                if let Some(v) = digits(c, i, 2) {
                    if v <= 59 {
                        out.push((v, 2));
                    }
                }
                if let Some(v) = digits(c, i, 1) {
                    out.push((v, 1));
                }
            }
            5 => {
                if let Some(v) = digits(c, i, 2) {
                    if v <= 61 {
                        out.push((v, 2));
                    }
                }
                if let Some(v) = digits(c, i, 1) {
                    out.push((v, 1));
                }
            }
            _ => {
                let _ = alts;
            }
        }
        out
    }
    fn lit(c: &[char], i: usize, ch: char) -> bool {
        i < c.len() && c[i].to_lowercase().eq(ch.to_lowercase())
    }
    // backtracking match: Y - m - d T H : M : S Z
    fn go(c: &[char], i: usize, f: usize, vals: &mut Vec<(i64, usize)>, alts: &[&[(usize, i64, i64)]; 6]) -> Option<usize> {
        if f == 6 {
            return if lit(c, i, 'Z') { Some(i + 1) } else { None };
        }
        let seps = ['-', '-', 'T', ':', ':'];
        let opts: Vec<(i64, usize)> = if f == 0 { digits(c, i, 4).map(|v| vec![(v, 4)]).unwrap_or_default() } else { field(c, i, f, alts) };
        for (v, n) in opts {
            vals.push((v, n));
            let j = i + n;
            let next = if f < 5 {
                if lit(c, j, seps[f]) {
                    go(c, j + 1, f + 1, vals, alts)
                } else {
                    None
                }
            } else {
                go(c, j, f + 1, vals, alts)
            };
            if next.is_some() {
                return next;
            }
            vals.pop();
        }
        None
    }
    let mut vals = Vec::new();
    let end = match go(&c, 0, 0, &mut vals, &alts) {
        Some(e) => e,
        None => return Err(LErr::Value(format!("time data {} does not match format '%Y-%m-%dT%H:%M:%SZ'", crate::pydump::str_repr(s)))),
    };
    if end != c.len() {
        return Err(LErr::Value(format!("unconverted data remains: {}", c[end..].iter().collect::<String>())));
    }
    let (y, mo, d, h, mi, se) = (vals[0].0, vals[1].0, vals[2].0, vals[3].0, vals[4].0, vals[5].0);
    if y < 1 {
        return Err(LErr::Value(format!("year {} is out of range", y)));
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let mdays = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][(mo - 1) as usize];
    if d > mdays {
        return Err(LErr::Value("day is out of range for month".into()));
    }
    if se > 59 {
        return Err(LErr::Value("second must be in 0..59".into()));
    }
    Ok((days_from_civil(y, mo, d) * 86400 + h * 3600 + mi * 60 + se) * 1_000_000)
}

fn hash_password(password: &Value, salt: Option<&Value>, iterations: Option<&Value>) -> LR<Value> {
    let salt_v = match salt {
        Some(s) if truthy(s) => s.clone(),
        _ => Value::String(token_hex(16)),
    };
    let iters_v = match iterations {
        Some(i) if truthy(i) => i.clone(),
        _ => json!(ITERATIONS.with(|c| c.get())),
    };
    let pw = match password {
        Value::String(s) => s.as_bytes().to_vec(),
        other => return Err(LErr::Other(format!("AttributeError: '{}' object has no attribute 'encode'", type_name(other)))),
    };
    let salt_s = match &salt_v {
        Value::String(s) => s.clone(),
        other => return Err(LErr::Value(format!("fromhex() argument must be str, not {}", type_name(other)))),
    };
    let salt_b = parse_hex(&salt_s)?;
    let n = match &iters_v {
        Value::Number(n) if !n.is_f64() && n.as_i64().map(|x| x > 0).unwrap_or(false) => n.as_i64().unwrap() as u32,
        _ => return Err(LErr::Value("iterations must be a positive integer".into())),
    };
    let mut out = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<Sha256>(&pw, &salt_b, n, &mut out);
    Ok(json!({"salt": salt_v, "iterations": iters_v, "hash": hex(&out)}))
}

fn parse_hex(s: &str) -> LR<Vec<u8>> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let h = |x: u8| (x as char).to_digit(16);
        match (b.get(i).and_then(|x| h(*x)), b.get(i + 1).and_then(|x| h(*x))) {
            (Some(a), Some(c)) => out.push((a * 16 + c) as u8),
            _ => return Err(LErr::Value(format!("non-hexadecimal number found in fromhex() arg at position {}", if b.get(i).and_then(|x| h(*x)).is_some() { i + 1 } else { i }))),
        }
        i += 2;
    }
    Ok(out)
}

fn compare_digest(a: &Value, b: &Value) -> LR<bool> {
    match (a, b) {
        (Value::String(x), Value::String(y)) => {
            if !x.is_ascii() || !y.is_ascii() {
                return Err(LErr::Value("comparing strings with non-ASCII characters is not supported".into()));
            }
            Ok(x == y)
        }
        _ => Err(LErr::Value(format!("unsupported operand types(s) or combination of types: '{}' and '{}'", type_name(a), type_name(b)))),
    }
}

fn check_password(password: &Value, record: &Value) -> LR<bool> {
    if !truthy(record) {
        return Ok(false);
    }
    let get = |k: &str| -> LR<&Value> { record.get(k).ok_or_else(|| LErr::Value(format!("'{}'", k))) };
    let test = hash_password(password, Some(get("salt")?), Some(get("iterations")?))?;
    compare_digest(&test["hash"], get("hash")?)
}

// ---------------------------------------------------------------- small value helpers
fn verr<T>(m: impl Into<String>) -> LR<T> {
    Err(LErr::Value(m.into()))
}
fn field(v: &Value, k: &str) -> Value {
    v.get(k).cloned().unwrap_or(Value::Null)
}
fn sfield<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("")
}
fn as_obj(v: &Value) -> LR<&Map<String, Value>> {
    match v {
        Value::Object(m) => Ok(m),
        other => Err(LErr::Other(format!("AttributeError: '{}' object has no attribute 'get'", type_name(other)))),
    }
}
fn py_iter(v: &Value) -> LR<Vec<Value>> {
    match v {
        Value::Array(a) => Ok(a.clone()),
        Value::String(s) => Ok(s.chars().map(|c| Value::String(c.to_string())).collect()),
        Value::Object(m) => Ok(m.keys().map(|k| Value::String(k.clone())).collect()),
        other => verr(format!("'{}' object is not iterable", type_name(other))),
    }
}
fn trunc_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}
fn py_int(v: &Value) -> LR<i64> {
    match v {
        Value::Bool(b) => Ok(*b as i64),
        Value::Number(n) if !n.is_f64() => n.as_i64().ok_or_else(|| LErr::Value("Python int too large to convert to C long".into())),
        Value::Number(n) => {
            let f = n.as_f64().unwrap();
            if !f.is_finite() {
                return verr("cannot convert float infinity to integer");
            }
            Ok(f.trunc() as i64)
        }
        Value::String(s) => {
            let t = s.trim();
            let (neg, body) = match t.strip_prefix('-') {
                Some(r) => (true, r),
                None => (false, t.strip_prefix('+').unwrap_or(t)),
            };
            let ok = !body.is_empty()
                && body.chars().all(|c| c.is_ascii_digit() || c == '_')
                && !body.starts_with('_')
                && !body.ends_with('_')
                && !body.contains("__");
            if !ok {
                return verr(format!("invalid literal for int() with base 10: {}", crate::pydump::str_repr(s)));
            }
            let n: i64 = body.replace('_', "").parse().map_err(|_| LErr::Value("Python int too large to convert to C long".into()))?;
            Ok(if neg { -n } else { n })
        }
        other => verr(format!("int() argument must be a string, a bytes-like object or a real number, not '{}'", type_name(other))),
    }
}

fn name_re_ok(s: &str) -> bool {
    let c: Vec<char> = s.chars().collect();
    !c.is_empty()
        && c.len() <= 30
        && c[0].is_ascii_alphanumeric()
        && c[1..].iter().all(|x| x.is_ascii_alphanumeric() || " ._-".contains(*x))
}

// ---------------------------------------------------------------- shipped defaults
fn rights_map(items: &[(&str, bool)]) -> Value {
    Value::Object(items.iter().map(|(k, v)| (k.to_string(), Value::Bool(*v))).collect())
}
fn profile(name: &str, description: &str, default: bool, rights: Value) -> Value {
    json!({"name": name, "description": description, "default_enable": default, "rights": rights})
}
fn default_profiles() -> Vec<Value> {
    let gm = |m: &str, f: &str| format!("{}/{}/{}", GM, m, f);
    let remote = gm("Administration", "Remote Shutdown");
    vec![
        profile("Admin Profile", "Admin Profile for Access Control", true, json!({})),
        profile("Scientist", "Full GeneMapper ID-X Control. No access to Security, Audit and ESig configuration tools.", false, rights_map(&[(GM, true), (&remote, false)])),
        profile(
            "Technician/Read Only",
            "This profile has read only access to GeneMapper ID-X. No access to Security, Audit and ESig configuration tools.",
            false,
            rights_map(&[
                (&gm("Analysis Method", "READ_ANALYSIS_METHOD"), true),
                (&gm("Size Standard", "READ_SIZE_STANDARD"), true),
                (&gm("Panel Manager", "READ_PANEL"), true),
                (&gm("Project", "READ_PROJECT"), true),
                (&gm("Matrix", "READ_MATRIX"), true),
                (&gm("Profile Manager", "READ_PROFILE_CONCORDANCE"), true),
            ]),
        ),
        profile(
            "Analyst",
            "Access to all GeneMapper ID-X functionality except Analysist Method. Analyst can only edit the sizing range and analysis range for an Analysis Method. No access to Security, Audit and ESig configuration tools.",
            false,
            rights_map(&[
                (GM, true),
                (&format!("{}/Analysis Method", GM), false),
                (&gm("Analysis Method", "READ_ANALYSIS_METHOD"), true),
                (&gm("Analysis Method", "UPDATE_ANALYSIS_RANGE"), true),
                (&gm("Analysis Method", "UPDATE_SIZING_RANGE"), true),
                (&gm("Analysis Method", "SAVE_AS_ANALYSIS_METHOD"), true),
                (&gm("Profile Manager", "UPDATE_PROFILE_CONCORDANCE"), false),
                (&remote, false),
            ]),
        ),
    ]
}
fn default_users() -> Vec<Value> {
    let all3: &[&str] = &["Administrators", "Casework User Group", "Databasing User Group"];
    let rows: Vec<(&str, &str, &str, &str, &[&str])> = vec![
        ("Administrator", "Administrator user", "User Administrator", "Admin Profile", all3),
        ("gmidx", "GeneMapper DefaultUser", "GeneMapper DefaultUser", "Admin Profile", all3),
        ("Casework Analyst", "Casework Analyst", "", "Analyst", &["Casework User Group"]),
        ("Databasing Analyst", "Databasing Analyst", "", "Analyst", &["Databasing User Group"]),
        ("Casework Tech", "Casework Technician", "", "Technician/Read Only", &["Casework User Group"]),
        ("Databasing Tech", "Databasing Technician", "", "Technician/Read Only", &["Databasing User Group"]),
        ("Casework Tech Lead", "Casework Technical Lead", "", "Admin Profile", &["Casework User Group"]),
        ("Databasing Tech Lead", "Databasing Technical Lead", "", "Admin Profile", &["Databasing User Group"]),
        ("Lab Director", "Laboratory Director", "", "Admin Profile", all3),
        ("Practice User", "Practice User", "For use with Getting Started Guide only", "Scientist", &["Practice User Group"]),
    ];
    rows.into_iter()
        .map(|(n, f, d, p, g)| {
            json!({"name": n, "full_name": f, "description": d, "profile": p, "groups": g, "disabled": false, "password": null,
                   "password_created": null, "history": [], "failed": [], "suspended_until": null, "grace_used": 0, "created": null, "last_login": null})
        })
        .collect()
}
fn rights_full() -> Value {
    Value::Object(RIGHT_NAMES.iter().map(|k| (k.to_string(), Value::Bool(true))).collect())
}
fn rights_ru(owner: bool) -> Value {
    Value::Object(RIGHT_NAMES.iter().map(|k| (k.to_string(), Value::Bool(matches!(*k, "read" | "update") || (owner && k.starts_with("owner_"))))).collect())
}
fn default_security_groups() -> Vec<Value> {
    vec![
        json!({"name": "Admin Security Group", "description": "Admin Security Group for Access Control", "rights": {"Administrators": rights_full()}}),
        json!({"name": DEFAULT_SECURITY_GROUP, "description": "GeneMapper default security group",
               "rights": {"All Users": rights_full(), "Administrators": rights_full(), "Casework User Group": rights_ru(false), "Databasing User Group": rights_ru(false)}}),
        json!({"name": "Casework Security Group", "description": "", "rights": {"Administrators": rights_full(), "Casework User Group": rights_ru(false)}}),
        json!({"name": "Databasing Security Group", "description": "", "rights": {"Administrators": rights_full(), "Databasing User Group": rights_ru(false)}}),
        json!({"name": "Practice Security Group", "description": "For use with Getting Started Guide only",
               "rights": {"Administrators": rights_full(), "Practice User Group": rights_ru(false), "All Users": rights_ru(true)}}),
    ]
}
fn audit_attr(name: &str, state: &str, auditable: bool, typ: &str, form: i64) -> Value {
    json!({"name": name, "type": typ, "state": state, "auditable": auditable, "form": form})
}
fn a(name: &str, state: &str) -> Value {
    audit_attr(name, state, true, "primitive", 0)
}
fn cmd(st: &str) -> Vec<Value> {
    vec![a("created", st), a("modified", st), a("deleted", st), audit_attr("name", st, false, "primitive", 0)]
}
fn amap(name: &str, typ: &str, attrs: Vec<Value>) -> Value {
    json!({"name": name, "type": typ, "enabled": true, "attributes": attrs})
}
fn default_audit_maps() -> Vec<Value> {
    let cat = |mut v: Vec<Value>, extra: Vec<Value>| {
        v.extend(extra);
        v
    };
    let mut allele = cmd("on");
    allele.truncate(3);
    allele.push(audit_attr("name", "silent", false, "primitive", 0));
    let mut c3 = cmd("silent");
    c3.truncate(3);
    vec![
        amap("Allele", "AlleleT", allele),
        amap("Kit", "KitT", cat(cmd("silent"), vec![audit_attr("PANEL", "on", true, "PanelT", 2), audit_attr("BINSET", "on", true, "BinSetT", 2)])),
        amap("SizeStandard", "SizeStandardT", c3.clone()),
        amap(
            "Sample",
            "SampleT",
            cat(cmd("silent"), vec![a("OverrideSQ", "silent"), a("OverrideGQ", "silent"), a("OverrideCGQ", "silent"), audit_attr("ALLELE", "on", true, "AlleleT", 2)]),
        ),
        amap("Marker", "MarkerT", cat(cmd("silent"), vec![audit_attr("BIN", "on", true, "BinT", 2)])),
        amap(
            "Run",
            "RunT",
            vec![
                a("created", "silent"),
                audit_attr("modified", "silent", false, "primitive", 0),
                a("deleted", "silent"),
                audit_attr("name", "silent", false, "primitive", 0),
                audit_attr("OverrideIQC", "silent", false, "primitive", 0),
                audit_attr("OverrideSQC", "silent", false, "primitive", 0),
                audit_attr("SAMPLE", "on", true, "SampleT", 2),
            ],
        ),
        amap("Panel", "PanelT", cat(cmd("silent"), vec![audit_attr("MARKER", "on", true, "MarkerT", 2)])),
        amap("AnalysisMethod", "AnalysisMethodT", cmd("silent")),
        amap(
            "BinSet",
            "BinSetT",
            vec![a("created", "silent"), audit_attr("modified", "silent", false, "primitive", 0), a("deleted", "silent"), audit_attr("name", "silent", false, "primitive", 0)],
        ),
        amap("Bin", "BinT", cmd("silent")),
        amap("Project", "ProjectT", cat(cmd("silent"), vec![a("analyzed", "silent"), audit_attr("RUN", "on", true, "RunT", 2)])),
        amap("Matrix", "MatrixT", c3),
    ]
}
fn esig_group(name: &str, noun: &str, title: &str) -> Value {
    json!({"name": name, "required": false, "challenge": CHALLENGE, "actions": [
        {"name": format!("CREATE_{}", noun), "meaning": format!("Create New {}", title)},
        {"name": format!("MODIFY_{}", noun), "meaning": format!("Modify {}", title)},
        {"name": format!("DELETE_{}", noun), "meaning": format!("Delete {}", title)}]})
}

pub fn default_document() -> Value {
    json!({
        "schema": 1,
        "settings": {"require_login": false},
        "policies": {"max_failed_attempts": 5, "send_log_message": true, "upon_failure": "suspend", "suspension_minutes": 10,
                     "lifetime_enabled": false, "lifetime_days": 90, "grace_logins": 6, "reuse_restricted": true,
                     "passwords_kept": 10, "min_length": 6, "reuse_period_days": 30},
        "profiles": default_profiles(),
        "users": default_users(),
        "user_groups": [
            {"name": "Administrators", "description": "Administrators for Access Control", "all_users": false},
            {"name": "All Users", "description": "All Users for Access Control", "all_users": true},
            {"name": "Casework User Group", "description": "", "all_users": false},
            {"name": "Databasing User Group", "description": "", "all_users": false},
            {"name": "Practice User Group", "description": "For use with Getting Started Guide only", "all_users": false}],
        "security_groups": default_security_groups(),
        "audit": {"enabled": true, "maps": default_audit_maps(), "reasons": [], "free_text": true},
        "esig": [esig_group("GM_PROJECT", "PROJECT", "Project"), esig_group("GM_CHEMISTRY_KIT", "CHEMISTRY_KIT", "Chemistry Kit"),
                 esig_group("GM_ANALYSIS_METHOD", "ANALYSIS_METHOD", "Analysis Method"), esig_group("GM_SIZE_STANDARD", "SIZE_STANDARD", "Size Standard"),
                 esig_group("GM_MATRIX", "MATRIX", "Matrix")],
    })
}

/// Most specific entry wins: function, then module, then product, then the profile default.
pub fn profile_allows(profile: &Value, function: &Value) -> LR<bool> {
    let key = function_key(function)?;
    let empty = json!({});
    let rights = match profile.get("rights") {
        Some(r) => r,
        None => &empty,
    };
    let mut parts = key.splitn(3, '/');
    let product = parts.next().unwrap();
    let module = parts.next().unwrap();
    for k in [key.clone(), format!("{}/{}", product, module), product.to_string()] {
        let has = match rights {
            Value::Object(m) => m.get(&k),
            _ => None,
        };
        if let Some(v) = has {
            return Ok(truthy(v));
        }
    }
    Ok(profile.get("default_enable").map(truthy).unwrap_or(false))
}

fn fsrw<T>(r: std::io::Result<T>) -> LR<T> {
    r.map_err(|e| LErr::Other(format!("OSError: {}", e)))
}

fn read_text(p: &Path) -> LR<String> {
    let b = fsrw(fs::read(p))?;
    String::from_utf8(b).map_err(|e| LErr::Value(format!("'utf-8' codec can't decode: {}", e)))
}

fn load_err(e: crate::pyjson::LoadErr) -> LErr {
    LErr::Value(e.message())
}

fn jloads(text: &str) -> LR<Value> {
    loads(text, false).map_err(load_err)
}

/// `json.loads(value)` where value comes from a request (must be str).
fn jloads_value(v: &Value) -> LR<Value> {
    match v {
        Value::String(s) => jloads(s),
        other => verr(format!("the JSON object must be str, bytes or bytearray, not {}", type_name(other))),
    }
}

fn expand_user(p: &str) -> PathBuf {
    if p == "~" || p.starts_with("~/") {
        if let Ok(h) = std::env::var("HOME") {
            return Path::new(&h).join(p.trim_start_matches('~').trim_start_matches('/'));
        }
    }
    PathBuf::from(p)
}

pub struct SecurityStore {
    pub root: PathBuf,
    doc_path: PathBuf,
    audit_path: PathBuf,
    key: Vec<u8>,
    pub doc: Value,
    /// login tickets in insertion order (ticket, user name)
    pub tickets: Vec<(String, String)>,
}

macro_rules! arr {
    ($self:expr, $($k:expr),+) => {{
        let mut v = &$self.doc;
        $( v = &v[$k]; )+
        v.as_array().cloned().unwrap_or_default()
    }};
}

fn shape_ok(doc: &Value) -> bool {
    let obj = |k: &str| doc.get(k).map(|v| v.is_object()).unwrap_or(false);
    let ar = |k: &str| doc.get(k).map(|v| v.is_array()).unwrap_or(false);
    let sections = obj("settings") && obj("policies") && ar("profiles") && ar("users") && ar("user_groups") && ar("security_groups") && ar("esig");
    let audit = doc.get("audit").map(|a| a.is_object() && a["maps"].is_array() && a["reasons"].is_array()).unwrap_or(false);
    sections && audit
}

impl SecurityStore {
    pub fn open(root: &str) -> LR<SecurityStore> {
        let root = expand_user(root);
        fsrw(fs::create_dir_all(&root))?;
        let doc_path = root.join("security.json");
        let audit_path = root.join("audit.jsonl");
        let key_path = root.join("audit.key");
        if !key_path.exists() {
            fsrw(fs::write(&key_path, token_hex(32)))?;
        }
        let key_txt = read_text(&key_path)?;
        let key = parse_hex(key_txt.trim())?;
        let mut s = SecurityStore { root, doc_path, audit_path, key, doc: Value::Null, tickets: Vec::new() };
        s.doc = s.load()?;
        Ok(s)
    }

    fn load(&self) -> LR<Value> {
        if self.doc_path.exists() {
            let mut doc = jloads(&read_text(&self.doc_path)?)?;
            let m = match &mut doc {
                Value::Object(m) => m,
                other => return Err(LErr::Other(format!("AttributeError: '{}' object has no attribute 'get'", type_name(other)))),
            };
            if !m.get("schema").map(|s| py_eq(s, &json!(1))).unwrap_or(false) {
                return verr("Unsupported security document");
            }
            if let Value::Object(base) = default_document() {
                for (k, v) in base {
                    if !m.contains_key(&k) {
                        m.insert(k, v);
                    }
                }
            }
            if !shape_ok(&doc) {
                return Err(LErr::Other("TypeError: malformed security document".into()));
            }
            return Ok(doc);
        }
        let mut doc = default_document();
        let stamp = iso(now_micros());
        for u in doc["users"].as_array_mut().unwrap() {
            u["created"] = Value::String(stamp.clone());
        }
        self.write_doc(&doc)?;
        Ok(doc)
    }

    fn write_doc(&self, doc: &Value) -> LR<()> {
        let tmp = self.doc_path.with_extension("tmp");
        fsrw(fs::write(&tmp, dumps(doc, Opts { ensure_ascii: false, sort_keys: false, indent: Some(1) })))?;
        fsrw(fs::rename(&tmp, &self.doc_path))
    }

    fn write(&self) -> LR<()> {
        self.write_doc(&self.doc)
    }

    // -------------------------------------------------------- lookups
    fn user_idx(&self, name: &Value) -> Option<usize> {
        self.doc["users"].as_array().unwrap().iter().position(|u| u.get("name").map(|n| py_eq(n, name)).unwrap_or(false))
    }
    pub fn user(&self, name: &Value) -> Option<Value> {
        self.user_idx(name).map(|i| self.doc["users"][i].clone())
    }
    fn profile_idx(&self, name: &Value) -> Option<usize> {
        self.doc["profiles"].as_array().unwrap().iter().position(|p| p.get("name").map(|n| py_eq(n, name)).unwrap_or(false))
    }
    fn profile(&self, name: &Value) -> Option<Value> {
        self.profile_idx(name).map(|i| self.doc["profiles"][i].clone())
    }

    pub fn can(&self, user: Option<&Value>, function: &str) -> LR<bool> {
        let user = match user {
            Some(u) => u,
            None => return Ok(false),
        };
        match self.profile(&field(user, "profile")) {
            Some(p) => profile_allows(&p, &json!(function)),
            None => Ok(false),
        }
    }

    pub fn rights(&self, user: &Value) -> LR<Vec<(String, bool)>> {
        functions().into_iter().map(|k| Ok((k.clone(), self.can(Some(user), &k)?))).collect()
    }

    pub fn require(&self, user: Option<&Value>, functions: &[&str]) -> LR<()> {
        for f in functions {
            if self.can(user, f)? {
                return Ok(());
            }
        }
        Err(LErr::Denied(ACCESS_DENIED.into()))
    }

    pub fn session_user(&self, ticket: Option<&str>) -> LR<Option<Value>> {
        let t = ticket.unwrap_or("");
        if let Some((_, name)) = self.tickets.iter().find(|(k, _)| k == t) {
            if let Some(u) = self.user(&json!(name)) {
                if !truthy(&field(&u, "disabled")) {
                    return Ok(Some(u));
                }
            }
        }
        if !truthy(&self.doc["settings"]["require_login"]) {
            if let Some(u) = self.user(&json!("Administrator")) {
                return Ok(Some(u));
            }
            let run = format!("{}/General/May run application", ACC);
            for u in arr!(self, "users") {
                if self.can(Some(&u), &run)? {
                    return Ok(Some(u));
                }
            }
        }
        Ok(None)
    }

    pub fn groups_of(&self, user: &Value) -> Vec<String> {
        let mine = user.get("groups").cloned().unwrap_or_else(|| json!([]));
        let mine = mine.as_array().cloned().unwrap_or_default();
        arr!(self, "user_groups")
            .iter()
            .filter(|g| truthy(&field(g, "all_users")) || mine.iter().any(|x| g.get("name").map(|n| py_eq(n, x)).unwrap_or(false)))
            .map(|g| sfield(g, "name").to_string())
            .collect()
    }

    // -------------------------------------------------------- data-item access (security groups)
    pub fn object_rights(&self, user: Option<&Value>, group: &Value) -> Vec<String> {
        let user = match user {
            Some(u) => u,
            None => return vec![],
        };
        let groups = arr!(self, "security_groups");
        let sg = groups
            .iter()
            .find(|g| g.get("name").map(|n| py_eq(n, group)).unwrap_or(false))
            .or_else(|| groups.iter().find(|g| sfield(g, "name") == DEFAULT_SECURITY_GROUP));
        let sg = match sg {
            Some(g) => g,
            None => return vec![],
        };
        let mine = self.groups_of(user);
        let mut out: Vec<String> = Vec::new();
        if let Some(Value::Object(rs)) = sg.get("rights") {
            for (ug, rights) in rs {
                if mine.contains(ug) {
                    if let Value::Object(r) = rights {
                        for (name, on) in r {
                            if truthy(on) && !out.contains(name) {
                                out.push(name.clone());
                            }
                        }
                    }
                }
            }
        }
        out
    }

    pub fn object_allows(&self, user: Option<&Value>, group: &Value, op: &str, owner: &Value) -> bool {
        let rights = self.object_rights(user, group);
        if rights.iter().any(|r| r == op) {
            return true;
        }
        if op == "update" || op == "delete" {
            if let Some(u) = user {
                if truthy(owner) && field(u, "name").as_str().map(|n| py_eq(owner, &json!(n))).unwrap_or(false) && rights.iter().any(|r| *r == format!("owner_{}", op)) {
                    return true;
                }
            }
        }
        false
    }

    pub fn require_object(&self, user: Option<&Value>, group: &Value, op: &str, owner: &Value) -> LR<()> {
        if !self.object_allows(user, group, op, owner) {
            let g = if truthy(group) { py_str(group) } else { DEFAULT_SECURITY_GROUP.to_string() };
            return Err(LErr::Denied(format!("Access to an item of security group \"{}\" ({}) is not allowed for your user groups.", g, op)));
        }
        Ok(())
    }

    // -------------------------------------------------------- authentication
    fn locked(&self, u: &Value, now: i64) -> LR<bool> {
        let s = field(u, "suspended_until");
        if !truthy(&s) {
            return Ok(false);
        }
        Ok(parse_iso(&s)? > now)
    }

    fn pol(&self, k: &str) -> Value {
        field(&self.doc["policies"], k)
    }
    fn pol_i(&self, k: &str) -> i64 {
        self.doc["policies"][k].as_i64().unwrap_or(0)
    }

    fn password_problem(&self, u: &Value, password: &Value) -> LR<Option<String>> {
        let min = self.pol_i("min_length");
        let bad_len = match password {
            Value::String(s) => (s.chars().count() as i64) < min || s.chars().count() > 30,
            _ => true,
        };
        if bad_len {
            return Ok(Some(format!("Password must be {} to 30 characters.", py_str(&self.pol("min_length")))));
        }
        let pw = password.as_str().unwrap();
        if pw.chars().any(|c| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)) {
            return Ok(Some("Password must not contain spaces.".into()));
        }
        if truthy(&self.pol("reuse_restricted")) {
            let now = now_micros();
            let history: Vec<Value> = u.get("history").and_then(|h| h.as_array()).cloned().unwrap_or_default();
            let keep = self.pol_i("passwords_kept");
            let kept: Vec<Value> = if keep != 0 {
                let n = keep.max(0) as usize;
                history[history.len().saturating_sub(n)..].to_vec()
            } else {
                vec![]
            };
            let mut recent = Vec::new();
            for h in &history {
                let at = field(h, "at");
                if truthy(&at) && (now - parse_iso(&at)?).div_euclid(86_400_000_000) < self.pol_i("reuse_period_days") {
                    recent.push(h.clone());
                }
            }
            let mut cands: Vec<Value> = Vec::new();
            if truthy(&field(u, "password")) {
                cands.push(field(u, "password"));
            }
            cands.extend(kept);
            cands.extend(recent);
            for h in &cands {
                if check_password(password, h)? {
                    return Ok(Some("This password was used earlier - please choose another password.".into()));
                }
            }
        }
        Ok(None)
    }

    /// `_set_password` on `users[i]`
    fn set_password(&mut self, i: usize, password: &Value) -> LR<()> {
        let u = self.doc["users"][i].clone();
        if let Some(p) = self.password_problem(&u, password)? {
            return verr(p);
        }
        let keep = self.pol_i("passwords_kept").max(1) as usize;
        let now = now_micros();
        let hashed_cur = field(&u, "password");
        let um = self.doc["users"][i].as_object_mut().unwrap();
        if truthy(&hashed_cur) {
            let mut rec = match &hashed_cur {
                Value::Object(m) => m.clone(),
                _ => Map::new(),
            };
            rec.insert("at".into(), um.get("password_created").cloned().unwrap_or(Value::Null));
            let hist = um.entry("history").or_insert_with(|| json!([]));
            if let Value::Array(h) = hist {
                h.push(Value::Object(rec));
                let n = h.len();
                if n > keep {
                    h.drain(0..n - keep);
                }
            }
        }
        let hp = hash_password(password, None, None)?;
        let um = self.doc["users"][i].as_object_mut().unwrap();
        um.insert("password".into(), hp);
        um.insert("password_created".into(), Value::String(iso(now)));
        um.insert("grace_used".into(), json!(0));
        Ok(())
    }

    pub fn login(&mut self, name: &str, password: &str) -> LR<Value> {
        let now = now_micros();
        let namev = json!(name);
        let ui = match self.user_idx(&namev) {
            Some(i) => i,
            None => return verr("The username does not exist. Contact the administrator."),
        };
        let u = self.doc["users"][ui].clone();
        if truthy(&field(&u, "disabled")) {
            return verr("The username is currently disabled, Contact the administrator.");
        }
        if self.locked(&u, now)? {
            let left_s = (now_susp(&u)? - now) as f64 / 1e6;
            let left = 1.max((left_s / 60.0).floor() as i64 + 1);
            return verr(format!("User Name: {} is currently locked out for {} minutes.", name, left));
        }
        if !truthy(&field(&u, "password")) {
            return verr("No password has been set for this account. Contact the administrator.");
        }
        if !check_password(&json!(password), &field(&u, "password"))? {
            let mut failed: Vec<Value> = Vec::new();
            for t in u.get("failed").and_then(|f| f.as_array()).cloned().unwrap_or_default() {
                if ((now - parse_iso(&t)?) as f64 / 1e6) < 86400.0 {
                    failed.push(t);
                }
            }
            failed.push(Value::String(iso(now)));
            let max = self.pol_i("max_failed_attempts");
            let upon = self.pol("upon_failure");
            let mins = self.pol_i("suspension_minutes");
            let um = self.doc["users"][ui].as_object_mut().unwrap();
            let mut failed_v = Value::Array(failed.clone());
            if failed.len() as i64 >= max {
                if upon == json!("disable") {
                    um.insert("disabled".into(), Value::Bool(true));
                } else if upon == json!("suspend") {
                    um.insert("suspended_until".into(), Value::String(iso(now + mins * 60_000_000)));
                }
                failed_v = json!([]);
            }
            um.insert("failed".into(), failed_v);
            self.write()?;
            if truthy(&self.pol("send_log_message")) {
                self.append(&json!({"map": "Security", "instance": name, "attribute": "login", "old": "", "new": "failed", "reason": "", "event": "Login failed"}), None, &Value::Null)?;
            }
            return verr("The password is not correct. Re-enter your password or contact the administrator.");
        }
        let mut must_change = false;
        if truthy(&self.pol("lifetime_enabled")) && truthy(&field(&u, "password_created")) {
            let created = parse_iso(&field(&u, "password_created"))?;
            if (now - created).div_euclid(86_400_000_000) >= self.pol_i("lifetime_days") {
                let used = u.get("grace_used").and_then(|g| g.as_i64()).unwrap_or(0) + 1;
                self.doc["users"][ui].as_object_mut().unwrap().insert("grace_used".into(), json!(used));
                must_change = used > self.pol_i("grace_logins");
            }
        }
        {
            let um = self.doc["users"][ui].as_object_mut().unwrap();
            um.insert("failed".into(), json!([]));
            um.insert("suspended_until".into(), Value::Null);
            um.insert("last_login".into(), Value::String(iso(now)));
        }
        self.write()?;
        let ticket = token_urlsafe(24);
        if !must_change {
            self.tickets.push((ticket.clone(), name.to_string()));
        }
        let uu = self.doc["users"][ui].clone();
        self.append(&json!({"map": "Security", "instance": name, "attribute": "login", "old": "", "new": "success", "reason": "", "event": "Login"}), Some(&uu), &Value::Null)?;
        Ok(json!({"ticket": if must_change { Value::Null } else { Value::String(ticket) }, "must_change_password": must_change}))
    }

    pub fn logout(&mut self, ticket: Option<&str>) -> LR<Value> {
        let t = ticket.unwrap_or("");
        if let Some(pos) = self.tickets.iter().position(|(k, _)| k == t) {
            let (_, name) = self.tickets.remove(pos);
            let u = self.user(&json!(name));
            self.append(&json!({"map": "Security", "instance": name, "attribute": "logout", "old": "", "new": "", "reason": "", "event": "Logout"}), u.as_ref(), &Value::Null)?;
        }
        Ok(json!({"ok": true}))
    }

    pub fn change_password(&mut self, name: &str, old: &Value, new: &Value) -> LR<Value> {
        let namev = json!(name);
        let ui = self.user_idx(&namev);
        let ok = match ui {
            None => false,
            Some(i) => {
                let u = &self.doc["users"][i];
                truthy(&field(u, "password")) && check_password(&if truthy(old) { old.clone() } else { json!("") }, &field(u, "password"))?
            }
        };
        if !ok {
            return verr("Incorrect Old Password.  Please re-enter!");
        }
        if py_eq(old, new) {
            return verr("New password must be different from current password!");
        }
        self.set_password(ui.unwrap(), new)?;
        self.write()?;
        Ok(json!({"ok": true}))
    }

    pub fn verify_signature(&mut self, user: Option<&Value>, name: &Value, password: &Value, group: &Value, action: &Value) -> LR<Value> {
        let user = match user {
            Some(u) if py_eq(name, &field(u, "name")) => u,
            _ => return verr("Invalid userID.  Only the logged-in user may sign."),
        };
        let pw_ok = truthy(&field(user, "password")) && check_password(&if truthy(password) { password.clone() } else { json!("") }, &field(user, "password"))?;
        if !pw_ok {
            return verr("The password is not correct. Re-enter your password or contact the administrator.");
        }
        let groups = arr!(self, "esig");
        let g = groups.iter().find(|g| g.get("name").map(|n| py_eq(n, group)).unwrap_or(false));
        let act = g.and_then(|g| g.get("actions").and_then(|a| a.as_array())).and_then(|acts| acts.iter().find(|a| a.get("name").map(|n| py_eq(n, action)).unwrap_or(false)));
        let act = match act {
            Some(a) => a.clone(),
            None => return verr("Unknown e-signature action"),
        };
        let g = g.unwrap();
        let rec = self.append(
            &json!({"map": "ESig", "instance": group, "attribute": action, "old": "", "new": field(&act, "meaning"), "reason": field(g, "challenge"), "event": "Electronic Signature"}),
            Some(user),
            &Value::Null,
        )?;
        Ok(json!({"ok": true, "meaning": field(&act, "meaning"), "record": rec["id"]}))
    }

    // -------------------------------------------------------- administration
    pub fn public_user(&self, u: &Value) -> LR<Value> {
        let mut out = Map::new();
        for k in ["name", "full_name", "description", "profile", "groups", "disabled", "created", "last_login", "password_created"] {
            out.insert(k.into(), field(u, k));
        }
        let locked = self.locked(u, now_micros())?;
        out.insert("has_password".into(), Value::Bool(truthy(&field(u, "password"))));
        out.insert("locked_out".into(), Value::Bool(locked));
        let status = if truthy(&field(u, "disabled")) {
            "disabled"
        } else if locked {
            "suspended"
        } else {
            "active"
        };
        out.insert("status".into(), json!(status));
        Ok(Value::Object(out))
    }

    pub fn admin_state(&self) -> LR<Value> {
        let d = &self.doc;
        let users = arr!(self, "users");
        let mut pu = Vec::new();
        for u in &users {
            pu.push(self.public_user(u)?);
        }
        let groups: Vec<Value> = arr!(self, "user_groups")
            .into_iter()
            .map(|g| {
                let members: Vec<Value> = users
                    .iter()
                    .filter(|u| {
                        truthy(&field(&g, "all_users")) || u.get("groups").and_then(|x| x.as_array()).map(|a| a.iter().any(|x| py_eq(x, &field(&g, "name")))).unwrap_or(false)
                    })
                    .map(|u| field(u, "name"))
                    .collect();
                let mut m = g.as_object().cloned().unwrap_or_default();
                m.insert("members".into(), Value::Array(members));
                Value::Object(m)
            })
            .collect();
        let catalog: Vec<Value> = CATALOG.iter().map(|(p, m, fs)| json!({"product": p, "module": m, "functions": fs})).collect();
        Ok(json!({"settings": d["settings"], "policies": d["policies"], "profiles": d["profiles"], "users": pu, "user_groups": groups,
                  "security_groups": d["security_groups"], "catalog": catalog, "upon_failure": UPON_FAILURE}))
    }

    pub fn save_user(&mut self, data: &Value, previous: &Value) -> LR<Value> {
        let dm = as_obj(data)?;
        let name = py_str(dm.get("name").unwrap_or(&json!(""))).trim().to_string();
        if !name_re_ok(&name) {
            return verr("User name can only consist of letters, digits, spaces, \".\", \"_\" or \"-\" (max 30).");
        }
        let pf = field(data, "profile");
        if self.profile(&pf).is_none() {
            return verr("Profile must be selected");
        }
        let ugs = arr!(self, "user_groups");
        let mut groups = Vec::new();
        for g in py_iter(dm.get("groups").unwrap_or(&json!([])))? {
            if ugs.iter().any(|x| x.get("name").map(|n| py_eq(n, &g)).unwrap_or(false) && !truthy(&field(x, "all_users"))) {
                groups.push(g);
            }
        }
        let ui = if truthy(previous) { self.user_idx(previous) } else { None };
        if truthy(previous) && ui.is_none() {
            return verr(format!("Unknown user: {}", py_str(previous)));
        }
        let prev_or = if truthy(previous) { previous.clone() } else { json!("") };
        if !py_eq(&json!(name), &prev_or) && self.user_idx(&json!(name)).is_some() {
            return verr("You have entered a User name that already exists.");
        }
        let ui = match ui {
            Some(i) => i,
            None => {
                let created = iso(now_micros());
                self.doc["users"].as_array_mut().unwrap().push(json!({"name": name, "password": null, "password_created": null, "history": [], "failed": [],
                    "suspended_until": null, "grace_used": 0, "created": created, "last_login": null}));
                self.doc["users"].as_array().unwrap().len() - 1
            }
        };
        if py_eq(previous, &json!("Administrator")) && name != "Administrator" {
            return verr("* This is the administrative User and cannot be renamed");
        }
        let full = trunc_chars(&py_str(dm.get("full_name").unwrap_or(&json!(""))), 80);
        let desc = trunc_chars(&py_str(dm.get("description").unwrap_or(&json!(""))), 200);
        {
            let um = self.doc["users"][ui].as_object_mut().unwrap();
            um.insert("name".into(), json!(name));
            um.insert("full_name".into(), json!(full));
            um.insert("description".into(), json!(desc));
            um.insert("profile".into(), pf.clone());
            um.insert("groups".into(), Value::Array(groups));
            um.insert("disabled".into(), Value::Bool(dm.get("disabled").map(truthy).unwrap_or(false)));
        }
        if name == "Administrator" && truthy(&field(&self.doc["users"][ui], "disabled")) {
            return verr("* This is the administrative User and cannot be disabled");
        }
        if let Some(p) = dm.get("password") {
            if truthy(p) {
                self.set_password(ui, p)?;
            }
        }
        if dm.get("unlock").map(truthy).unwrap_or(false) {
            let um = self.doc["users"][ui].as_object_mut().unwrap();
            um.insert("suspended_until".into(), Value::Null);
            um.insert("failed".into(), json!([]));
        }
        self.check_admin_left()?;
        self.write()?;
        self.public_user(&self.doc["users"][ui].clone())
    }

    fn check_admin_left(&self) -> LR<()> {
        let f = format!("{}/General/May run application", ACC);
        for u in arr!(self, "users") {
            if self.can(Some(&u), &f)? && !truthy(&field(&u, "disabled")) {
                return Ok(());
            }
        }
        verr("At least one enabled user must keep access to the Security Manager.")
    }

    pub fn delete_user(&mut self, name: &Value, current: Option<&Value>) -> LR<Value> {
        if py_eq(name, &json!("Administrator")) {
            return verr("* This is the administrative User and cannot be deleted");
        }
        if let Some(c) = current {
            if py_eq(name, &field(c, "name")) {
                return verr("You cannot delete the user who is logged in.");
            }
        }
        let before = self.doc["users"].as_array().unwrap().len();
        self.doc["users"].as_array_mut().unwrap().retain(|u| !u.get("name").map(|n| py_eq(n, name)).unwrap_or(false));
        if self.doc["users"].as_array().unwrap().len() == before {
            return verr(format!("Unknown user: {}", py_str(name)));
        }
        let n = name.as_str().map(|s| s.to_string());
        self.tickets.retain(|(_, tn)| Some(tn) != n.as_ref());
        self.write()?;
        Ok(json!({"ok": true}))
    }

    pub fn save_profile(&mut self, data: &Value, previous: &Value) -> LR<Value> {
        let dm = as_obj(data)?;
        let name = py_str(dm.get("name").unwrap_or(&json!(""))).trim().to_string();
        if name.is_empty() || name.chars().count() > 60 {
            return verr("Invalid name, Please try again");
        }
        let mut rights = Map::new();
        let rv = dm.get("rights").cloned().unwrap_or(Value::Null);
        let rv = if truthy(&rv) { rv } else { json!({}) };
        let (fs, ms, ps) = (functions(), modules(), products());
        match &rv {
            Value::Object(m) => {
                for (k, v) in m {
                    if !fs.contains(k) && !ms.contains(k) && !ps.contains(k) {
                        return verr(format!("Unknown access element: {}", k));
                    }
                    rights.insert(k.clone(), Value::Bool(truthy(v)));
                }
            }
            other => return Err(LErr::Other(format!("AttributeError: '{}' object has no attribute 'items'", type_name(other)))),
        }
        let pi = if truthy(previous) { self.profile_idx(previous) } else { None };
        let prev_or = if truthy(previous) { previous.clone() } else { json!("") };
        if !py_eq(&json!(name), &prev_or) && self.profile(&json!(name)).is_some() {
            return verr("Name has to be unique.");
        }
        let pi = match pi {
            Some(i) => i,
            None => {
                self.doc["profiles"].as_array_mut().unwrap().push(json!({}));
                self.doc["profiles"].as_array().unwrap().len() - 1
            }
        };
        {
            let pm = self.doc["profiles"][pi].as_object_mut().unwrap();
            pm.insert("name".into(), json!(name));
            pm.insert("description".into(), json!(trunc_chars(&py_str(dm.get("description").unwrap_or(&json!(""))), 400)));
            pm.insert("default_enable".into(), Value::Bool(dm.get("default_enable").map(truthy).unwrap_or(false)));
            pm.insert("rights".into(), Value::Object(rights));
        }
        if truthy(previous) && !py_eq(previous, &json!(name)) {
            for u in self.doc["users"].as_array_mut().unwrap() {
                if u.get("profile").map(|p| py_eq(p, previous)).unwrap_or(false) {
                    u["profile"] = json!(name);
                }
            }
        }
        self.check_admin_left()?;
        self.write()?;
        Ok(self.doc["profiles"][pi].clone())
    }

    pub fn delete_profile(&mut self, name: &Value) -> LR<Value> {
        if arr!(self, "users").iter().any(|u| u.get("profile").map(|p| py_eq(p, name)).unwrap_or(false)) {
            return verr("Cannot delete an object that is being referenced.");
        }
        self.doc["profiles"].as_array_mut().unwrap().retain(|p| !p.get("name").map(|n| py_eq(n, name)).unwrap_or(false));
        self.write()?;
        Ok(json!({"ok": true}))
    }

    pub fn save_group(&mut self, data: &Value, previous: &Value) -> LR<Value> {
        let dm = as_obj(data)?;
        let name = py_str(dm.get("name").unwrap_or(&json!(""))).trim().to_string();
        if name.is_empty() || name.chars().count() > 30 {
            return verr("Group name must be less 30 characters, Please try again");
        }
        let gi = if truthy(previous) {
            self.doc["user_groups"].as_array().unwrap().iter().position(|g| g.get("name").map(|n| py_eq(n, previous)).unwrap_or(false))
        } else {
            None
        };
        let prev_or = if truthy(previous) { previous.clone() } else { json!("") };
        if !py_eq(&json!(name), &prev_or) && arr!(self, "user_groups").iter().any(|x| sfield(x, "name") == name) {
            return verr("Name has to be unique.");
        }
        let gi = match gi {
            Some(i) => i,
            None => {
                self.doc["user_groups"].as_array_mut().unwrap().push(json!({"all_users": false}));
                self.doc["user_groups"].as_array().unwrap().len() - 1
            }
        };
        {
            let gm = self.doc["user_groups"][gi].as_object_mut().unwrap();
            gm.insert("name".into(), json!(name));
            gm.insert("description".into(), json!(trunc_chars(&py_str(dm.get("description").unwrap_or(&json!(""))), 200)));
        }
        let members: Vec<Value> = py_iter(dm.get("members").unwrap_or(&json!([])))?;
        let all_users = truthy(&field(&self.doc["user_groups"][gi], "all_users"));
        for u in self.doc["users"].as_array_mut().unwrap() {
            let cur: Vec<Value> = u.get("groups").and_then(|g| g.as_array()).cloned().unwrap_or_default();
            let mut gs: Vec<Value> = cur.into_iter().filter(|x| !py_eq(x, previous) && !py_eq(x, &json!(name))).collect();
            if members.iter().any(|m| u.get("name").map(|n| py_eq(n, m)).unwrap_or(false)) && !all_users {
                gs.push(json!(name));
            }
            u["groups"] = Value::Array(gs);
        }
        if truthy(previous) && !py_eq(previous, &json!(name)) {
            let prev_s = py_str(previous);
            for sg in self.doc["security_groups"].as_array_mut().unwrap() {
                if let Some(Value::Object(r)) = sg.get_mut("rights") {
                    if r.contains_key(&prev_s) {
                        let v = r.shift_remove(&prev_s).unwrap();
                        r.insert(name.clone(), v);
                    }
                }
            }
        }
        self.write()?;
        Ok(self.doc["user_groups"][gi].clone())
    }

    pub fn delete_group(&mut self, name: &Value) -> LR<Value> {
        let gi = self.doc["user_groups"].as_array().unwrap().iter().position(|g| g.get("name").map(|n| py_eq(n, name)).unwrap_or(false));
        let gi = match gi {
            Some(i) => i,
            None => return verr(format!("Unknown user group: {}", py_str(name))),
        };
        if truthy(&field(&self.doc["user_groups"][gi], "all_users")) {
            return verr("Cannot delete the All User UserGroup.");
        }
        if py_eq(name, &json!("Administrators")) {
            return verr("Cannot delete an Access Control admin object.");
        }
        self.doc["user_groups"].as_array_mut().unwrap().remove(gi);
        for u in self.doc["users"].as_array_mut().unwrap() {
            let cur: Vec<Value> = u.get("groups").and_then(|g| g.as_array()).cloned().unwrap_or_default();
            u["groups"] = Value::Array(cur.into_iter().filter(|x| !py_eq(x, name)).collect());
        }
        let n = py_str(name);
        for sg in self.doc["security_groups"].as_array_mut().unwrap() {
            if let Some(Value::Object(r)) = sg.get_mut("rights") {
                r.shift_remove(&n);
            }
        }
        self.write()?;
        Ok(json!({"ok": true}))
    }

    pub fn save_security_group(&mut self, data: &Value, previous: &Value) -> LR<Value> {
        let dm = as_obj(data)?;
        let name = py_str(dm.get("name").unwrap_or(&json!(""))).trim().to_string();
        if name.is_empty() || name.chars().count() > 60 {
            return verr("Invalid name, Please try again");
        }
        let names: Vec<String> = arr!(self, "user_groups").iter().map(|g| sfield(g, "name").to_string()).collect();
        let mut rights = Map::new();
        let rv = dm.get("rights").cloned().unwrap_or(Value::Null);
        let rv = if truthy(&rv) { rv } else { json!({}) };
        match &rv {
            Value::Object(m) => {
                for (ug, r) in m {
                    if !names.contains(ug) {
                        return verr(format!("Unknown user group: {}", ug));
                    }
                    let rm = as_obj(r)?;
                    rights.insert(ug.clone(), Value::Object(RIGHT_NAMES.iter().map(|k| (k.to_string(), Value::Bool(rm.get(*k).map(truthy).unwrap_or(false)))).collect()));
                }
            }
            other => return Err(LErr::Other(format!("AttributeError: '{}' object has no attribute 'items'", type_name(other)))),
        }
        let si = if truthy(previous) {
            self.doc["security_groups"].as_array().unwrap().iter().position(|g| g.get("name").map(|n| py_eq(n, previous)).unwrap_or(false))
        } else {
            None
        };
        let prev_or = if truthy(previous) { previous.clone() } else { json!("") };
        if !py_eq(&json!(name), &prev_or) && arr!(self, "security_groups").iter().any(|x| sfield(x, "name") == name) {
            return verr("Name has to be unique.");
        }
        let si = match si {
            Some(i) => i,
            None => {
                self.doc["security_groups"].as_array_mut().unwrap().push(json!({}));
                self.doc["security_groups"].as_array().unwrap().len() - 1
            }
        };
        {
            let sm = self.doc["security_groups"][si].as_object_mut().unwrap();
            sm.insert("name".into(), json!(name));
            sm.insert("description".into(), json!(trunc_chars(&py_str(dm.get("description").unwrap_or(&json!(""))), 200)));
            sm.insert("rights".into(), Value::Object(rights));
        }
        self.write()?;
        Ok(self.doc["security_groups"][si].clone())
    }

    pub fn save_policies(&mut self, data: &Value) -> LR<Value> {
        let empty = Map::new();
        let dm = match data {
            Value::Object(m) => m,
            _ => &empty,
        };
        let mut p = self.doc["policies"].as_object().cloned().unwrap_or_default();
        let ints: [(&str, i64, i64); 7] = [
            ("max_failed_attempts", 1, 99),
            ("suspension_minutes", 1, 9999),
            ("lifetime_days", 1, 9999),
            ("grace_logins", 0, 99),
            ("passwords_kept", 0, 99),
            ("min_length", 1, 30),
            ("reuse_period_days", 0, 9999),
        ];
        for (k, lo, hi) in ints {
            if let Some(v) = dm.get(k) {
                let n = py_int(v)?;
                if !(lo <= n && n <= hi) {
                    return verr(format!("{} must be between {} and {}", k, lo, hi));
                }
                p.insert(k.into(), json!(n));
            }
        }
        for k in ["send_log_message", "lifetime_enabled", "reuse_restricted"] {
            if let Some(v) = dm.get(k) {
                p.insert(k.into(), Value::Bool(truthy(v)));
            }
        }
        if let Some(v) = dm.get("upon_failure") {
            if !UPON_FAILURE.iter().any(|x| py_eq(v, &json!(x))) {
                return verr("Unknown failure policy");
            }
            p.insert("upon_failure".into(), v.clone());
        }
        self.doc["policies"] = Value::Object(p);
        self.write()?;
        Ok(self.doc["policies"].clone())
    }

    pub fn save_settings(&mut self, data: &Value, current: Option<&Value>) -> LR<Value> {
        let mut s = self.doc["settings"].as_object().cloned().unwrap_or_default();
        if let Some(v) = data.get("require_login") {
            let want = truthy(v);
            if want && !current.map(|c| truthy(&field(c, "password"))).unwrap_or(false) {
                return verr("Set a password for your own account before requiring login.");
            }
            s.insert("require_login".into(), Value::Bool(want));
        }
        self.doc["settings"] = Value::Object(s);
        self.write()?;
        Ok(self.doc["settings"].clone())
    }

    pub fn export_security(&self) -> String {
        let mut m = Map::new();
        m.insert("format".into(), json!("opentrace-security"));
        if let Value::Object(d) = &self.doc {
            for (k, v) in d {
                if k != "audit" && k != "esig" {
                    m.insert(k.clone(), v.clone());
                }
            }
        }
        dumps(&Value::Object(m), Opts { ensure_ascii: false, sort_keys: false, indent: Some(1) })
    }

    pub fn import_security(&mut self, text: &Value, current: Option<&Value>) -> LR<Value> {
        let _ = current;
        let data = jloads_value(text)?;
        let dm = as_obj(&data)?;
        if !dm.get("format").map(|f| py_eq(f, &json!("opentrace-security"))).unwrap_or(false) || !dm.get("schema").map(|s| py_eq(s, &json!(1))).unwrap_or(false) {
            return verr("Not an OpenTrace security export");
        }
        let keys = ["settings", "policies", "profiles", "users", "user_groups", "security_groups"];
        for k in keys {
            if !dm.contains_key(k) {
                return verr(format!("Missing section: {}", k));
            }
        }
        for k in keys {
            let ok = if k == "settings" || k == "policies" { dm[k].is_object() } else { dm[k].is_array() };
            if !ok {
                return verr(format!("Malformed section: {}", k));
            }
        }
        let backup = self.doc.clone();
        for k in keys {
            self.doc[k] = dm[k].clone();
        }
        let check = (|| -> LR<()> {
            for u in arr!(self, "users") {
                if self.profile(&field(&u, "profile")).is_none() {
                    return verr(format!("User {} refers to an unknown profile", py_str(&field(&u, "name"))));
                }
            }
            self.check_admin_left()
        })();
        if let Err(e) = check {
            self.doc = backup;
            return Err(e);
        }
        let names: Vec<String> = arr!(self, "users").iter().map(|u| sfield(u, "name").to_string()).collect();
        self.tickets.retain(|(_, n)| names.contains(n));
        self.write()?;
        Ok(json!({"ok": true, "users": self.doc["users"].as_array().unwrap().len()}))
    }

    // -------------------------------------------------------- audit configuration
    pub fn audit_state(&self, map_name: &Value, attribute: &Value) -> String {
        let a = &self.doc["audit"];
        if !truthy(&a["enabled"]) {
            return "off".into();
        }
        let maps = a["maps"].as_array().cloned().unwrap_or_default();
        let m = match maps.iter().find(|m| m.get("name").map(|n| py_eq(n, map_name)).unwrap_or(false)) {
            Some(m) if truthy(&field(m, "enabled")) => m.clone(),
            _ => return "off".into(),
        };
        let attrs = m["attributes"].as_array().cloned().unwrap_or_default();
        match attrs.iter().find(|x| x.get("name").map(|n| py_eq(n, attribute)).unwrap_or(false)) {
            Some(x) if truthy(&field(x, "auditable")) => sfield(x, "state").to_string(),
            _ => "off".into(),
        }
    }

    pub fn save_audit(&mut self, data: &Value, user: Option<&Value>) -> LR<Value> {
        let dm = as_obj(data)?;
        let mut a = self.doc["audit"].clone();
        let mcfg = format!("{}/Map Configuration Tool", AUDIT);
        if let Some(v) = dm.get("enabled") {
            self.require(user, &[&format!("{}/Enable and disable maps", mcfg)])?;
            a["enabled"] = Value::Bool(truthy(v));
        }
        if let Some(maps) = dm.get("maps") {
            for m in py_iter(maps)? {
                let mm = as_obj(&m)?;
                let mname = mm.get("name").cloned().unwrap_or(Value::Null);
                let ci = a["maps"].as_array().unwrap().iter().position(|c| c.get("name").map(|n| py_eq(n, &mname)).unwrap_or(false));
                let ci = match ci {
                    Some(i) => i,
                    None => return verr(format!("Unknown audit map: {}", py_str(&mname))),
                };
                let cur_enabled = truthy(&field(&a["maps"][ci], "enabled"));
                let want = mm.get("enabled").map(truthy).unwrap_or(cur_enabled);
                if want != cur_enabled {
                    self.require(user, &[&format!("{}/Enable and disable maps", mcfg)])?;
                    a["maps"][ci]["enabled"] = Value::Bool(truthy(&mm["enabled"]));
                }
                if let Some(ats) = mm.get("attributes") {
                    for at in py_iter(ats)? {
                        let am = as_obj(&at)?;
                        let aname = am.get("name").cloned().unwrap_or(Value::Null);
                        let xi = a["maps"][ci]["attributes"].as_array().unwrap().iter().position(|x| x.get("name").map(|n| py_eq(n, &aname)).unwrap_or(false));
                        let xi = match xi {
                            Some(i) => i,
                            None => return verr(format!("Unknown attribute {} of {}", py_str(&aname), py_str(&field(&a["maps"][ci], "name")))),
                        };
                        let x = a["maps"][ci]["attributes"][xi].clone();
                        let st = am.get("state").cloned().unwrap_or_else(|| field(&x, "state"));
                        if !AUDIT_STATES.iter().any(|s| py_eq(&st, &json!(s))) {
                            return verr("Audit state must be On, Off or Silent");
                        }
                        let auditable = am.get("auditable").map(truthy).unwrap_or_else(|| truthy(&field(&x, "auditable")));
                        if !py_eq(&st, &field(&x, "state")) || auditable != truthy(&field(&x, "auditable")) {
                            self.require(user, &[&format!("{}/Change auditing state", mcfg)])?;
                        }
                        a["maps"][ci]["attributes"][xi]["state"] = st;
                        a["maps"][ci]["attributes"][xi]["auditable"] = Value::Bool(auditable);
                    }
                }
            }
        }
        if let Some(rs) = dm.get("reasons") {
            self.require(user, &[&format!("{}/Change auditing state", mcfg)])?;
            let mut seen: Vec<String> = Vec::new();
            let mut reasons = Vec::new();
            for r in py_iter(rs)? {
                let rm = as_obj(&r)?;
                let text = py_str(rm.get("reason").unwrap_or(&json!(""))).trim().to_string();
                if text.is_empty() {
                    return verr("Values for Reason must be provided. Please try again.");
                }
                if seen.contains(&text.to_lowercase()) {
                    return verr("Audit Reason with this reason already exists, Please try again.");
                }
                seen.push(text.to_lowercase());
                reasons.push(json!({"reason": trunc_chars(&text, 500), "description": trunc_chars(&py_str(rm.get("description").unwrap_or(&json!(""))), 1000),
                                    "active": rm.get("active").map(truthy).unwrap_or(true)}));
            }
            a["reasons"] = Value::Array(reasons);
        }
        if let Some(v) = dm.get("free_text") {
            self.require(user, &[&format!("{}/Change auditing state", mcfg)])?;
            a["free_text"] = Value::Bool(truthy(v));
        }
        let old = self.doc["audit"].clone();
        self.doc["audit"] = a.clone();
        self.write()?;
        let mut changes: Vec<(String, &str, String, String)> = Vec::new();
        let b = |v: &Value| if truthy(v) { "True" } else { "False" }.to_string();
        let om = old["maps"].as_array().cloned().unwrap_or_default();
        let nm = a["maps"].as_array().cloned().unwrap_or_default();
        for (mo, mn) in om.iter().zip(nm.iter()) {
            if truthy(&field(mo, "enabled")) != truthy(&field(mn, "enabled")) {
                changes.push((sfield(mn, "name").to_string(), "enabled", b(&field(mo, "enabled")), b(&field(mn, "enabled"))));
            }
            let oa = mo["attributes"].as_array().cloned().unwrap_or_default();
            let na = mn["attributes"].as_array().cloned().unwrap_or_default();
            for (xo, xn) in oa.iter().zip(na.iter()) {
                if !py_eq(&field(xo, "state"), &field(xn, "state")) || truthy(&field(xo, "auditable")) != truthy(&field(xn, "auditable")) {
                    let f = |x: &Value| format!("{}{}", py_str(&field(x, "state")), if truthy(&field(x, "auditable")) { "" } else { " (not auditable)" });
                    changes.push((format!("{}.{}", sfield(mn, "name"), sfield(xn, "name")), "state", f(xo), f(xn)));
                }
            }
        }
        if truthy(&old["enabled"]) != truthy(&a["enabled"]) {
            changes.push(("Audit Trail".into(), "enabled", b(&old["enabled"]), b(&a["enabled"])));
        }
        let reason = py_str(dm.get("reason").unwrap_or(&json!("")));
        for (inst, attr, o, n) in changes {
            self.append(&json!({"map": "AuditMap", "instance": inst, "attribute": attr, "old": o, "new": n, "reason": reason, "event": "Change Audit Setting"}), user, &Value::Null)?;
        }
        Ok(a)
    }

    pub fn export_maps(&self) -> String {
        fn esc(s: &str) -> String {
            let mut o = String::new();
            for c in s.chars() {
                match c {
                    '&' => o.push_str("&amp;"),
                    '<' => o.push_str("&lt;"),
                    '>' => o.push_str("&gt;"),
                    '"' => o.push_str("&quot;"),
                    '\r' => o.push_str("&#13;"),
                    '\n' => o.push_str("&#10;"),
                    '\t' => o.push_str("&#09;"),
                    c => o.push(c),
                }
            }
            o
        }
        let b = |v: &Value| if truthy(v) { "true" } else { "false" };
        let s = |v: &Value| match v {
            Value::String(s) => s.clone(),
            other => py_str(other),
        };
        let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        let maps = self.doc["audit"]["maps"].as_array().cloned().unwrap_or_default();
        out.push_str(&format!("<AuditMaps enabled=\"{}\">", b(&self.doc["audit"]["enabled"])));
        for m in &maps {
            out.push_str(&format!("\n  <AuditMap name=\"{}\" type=\"{}\" enabled=\"{}\"", esc(&s(&field(m, "name"))), esc(&s(&field(m, "type"))), b(&field(m, "enabled"))));
            let attrs = m["attributes"].as_array().cloned().unwrap_or_default();
            if attrs.is_empty() {
                out.push_str(" />");
                continue;
            }
            out.push('>');
            for at in &attrs {
                let st = s(&field(at, "state"));
                let mut cap = String::new();
                for (i, c) in st.chars().enumerate() {
                    if i == 0 {
                        cap.extend(c.to_uppercase());
                    } else {
                        cap.extend(c.to_lowercase());
                    }
                }
                out.push_str(&format!(
                    "\n    <Attribute name=\"{}\" type=\"{}\" state=\"{}\" auditable=\"{}\" form=\"{}\" />",
                    esc(&s(&field(at, "name"))),
                    esc(&s(&field(at, "type"))),
                    esc(&cap),
                    b(&field(at, "auditable")),
                    esc(&py_str(&field(at, "form")))
                ));
            }
            out.push_str("\n  </AuditMap>");
        }
        if maps.is_empty() {
            out.truncate(out.len() - 1);
            out.push_str(" />");
        } else {
            out.push_str("\n</AuditMaps>");
        }
        out.push('\n');
        out
    }

    pub fn parse_maps(&self, xml: &Value) -> LR<Value> {
        let text = match xml {
            Value::String(s) => s,
            other => return Err(LErr::Other(format!("TypeError: a bytes-like object is required, not '{}'", type_name(other)))),
        };
        let doc = match roxmltree::Document::parse(text) {
            Ok(d) => d,
            Err(e) => return verr(format!("Not an audit map file: {}", expat_message(text, &e))),
        };
        let root = doc.root_element();
        let plain = |n: &roxmltree::Node, tag: &str| n.tag_name().namespace().is_none() && n.tag_name().name() == tag;
        if !plain(&root, "AuditMaps") {
            return verr("Not an audit map file");
        }
        let attr = |n: &roxmltree::Node, k: &str| -> Value { n.attribute(k).map(|v| json!(v)).unwrap_or(Value::Null) };
        let mut maps = Vec::new();
        for me in root.children().filter(|c| c.is_element() && plain(c, "AuditMap")) {
            let attrs: Vec<Value> = me
                .children()
                .filter(|c| c.is_element() && plain(c, "Attribute"))
                .map(|a| {
                    json!({"name": attr(&a, "name"), "state": a.attribute("state").unwrap_or("").to_lowercase(), "auditable": a.attribute("auditable") == Some("true")})
                })
                .collect();
            maps.push(json!({"name": attr(&me, "name"), "enabled": me.attribute("enabled") == Some("true"), "attributes": attrs}));
        }
        Ok(json!({"enabled": root.attribute("enabled").unwrap_or("true") == "true", "maps": maps}))
    }

    pub fn save_esig(&mut self, groups: &Value, user: Option<&Value>) -> LR<Value> {
        let new_in = py_iter(groups)?;
        let mut new = self.doc["esig"].clone();
        for g in &new_in {
            let gm = as_obj(g)?;
            let gname = gm.get("name").cloned().unwrap_or(Value::Null);
            let cur = self.doc["esig"].as_array().unwrap().iter().any(|x| x.get("name").map(|n| py_eq(n, &gname)).unwrap_or(false));
            if !cur {
                return verr(format!("Unknown action group: {}", py_str(&gname)));
            }
            let ti = new.as_array().unwrap().iter().position(|x| x.get("name").map(|n| py_eq(n, &gname)).unwrap_or(false)).unwrap();
            let required = gm.get("required").map(truthy).unwrap_or_else(|| truthy(&field(&new[ti], "required")));
            new[ti]["required"] = Value::Bool(required);
            let ch = py_str(gm.get("challenge").unwrap_or(&new[ti]["challenge"]));
            if ch.trim().is_empty() || ch.chars().count() > 1024 {
                return verr("Electronic Signature Challenge Text (Max. 1024 characters)");
            }
            new[ti]["challenge"] = json!(ch);
            let mut means: Vec<(Value, Value)> = Vec::new();
            for a in py_iter(gm.get("actions").unwrap_or(&json!([])))? {
                let am = as_obj(&a)?;
                means.push((am.get("name").cloned().unwrap_or(Value::Null), am.get("meaning").cloned().unwrap_or(Value::Null)));
            }
            let n_acts = new[ti]["actions"].as_array().map(|a| a.len()).unwrap_or(0);
            for k in 0..n_acts {
                let an = field(&new[ti]["actions"][k], "name");
                // dict comprehension: the last entry for a key wins
                let m = means.iter().rev().find(|(n, _)| py_eq(n, &an)).map(|(_, v)| v.clone());
                if let Some(m) = m {
                    if truthy(&m) {
                        new[ti]["actions"][k]["meaning"] = json!(trunc_chars(&py_str(&m), 255));
                    }
                }
            }
        }
        let olds = self.doc["esig"].as_array().cloned().unwrap_or_default();
        let news = new.as_array().cloned().unwrap_or_default();
        for (o, g) in olds.iter().zip(news.iter()) {
            if truthy(&field(o, "required")) != truthy(&field(g, "required")) {
                let b = |v: &Value| if truthy(v) { "True" } else { "False" };
                self.append(
                    &json!({"map": "ESig", "instance": field(g, "name"), "attribute": "required", "old": b(&field(o, "required")), "new": b(&field(g, "required")), "reason": "", "event": "Change ESig Setting"}),
                    user,
                    &Value::Null,
                )?;
            }
        }
        self.doc["esig"] = new.clone();
        self.write()?;
        Ok(new)
    }

    // -------------------------------------------------------- audit records
    fn code(&self, rec: &Value) -> String {
        let mut body = Map::new();
        if let Value::Object(m) = rec {
            for (k, v) in m {
                if k != "hash" {
                    body.insert(k.clone(), v.clone());
                }
            }
        }
        let text = dumps(&Value::Object(body), Opts { ensure_ascii: false, sort_keys: true, indent: None });
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&self.key).expect("hmac key");
        mac.update(text.as_bytes());
        hex(&mac.finalize().into_bytes())
    }

    fn records(&self) -> LR<Vec<Value>> {
        if !self.audit_path.exists() {
            return Ok(vec![]);
        }
        let text = read_text(&self.audit_path)?.replace("\r\n", "\n").replace('\r', "\n");
        let mut out = Vec::new();
        for line in text.split('\n') {
            let l = line.trim();
            if !l.is_empty() {
                out.push(jloads(l)?);
            }
        }
        Ok(out)
    }

    fn last(&self) -> LR<Option<Value>> {
        if !self.audit_path.exists() {
            return Ok(None);
        }
        let b = fsrw(fs::read(&self.audit_path))?;
        let start = b.len().saturating_sub(65536);
        let tail = &b[start..];
        let mut last: Option<&[u8]> = None;
        let mut i = 0;
        let mut lines: Vec<&[u8]> = Vec::new();
        while i <= tail.len() {
            let mut j = i;
            while j < tail.len() && tail[j] != b'\n' && tail[j] != b'\r' {
                j += 1;
            }
            lines.push(&tail[i..j]);
            if j < tail.len() && tail[j] == b'\r' && tail.get(j + 1) == Some(&b'\n') {
                j += 1;
            }
            i = j + 1;
        }
        for l in lines {
            if !String::from_utf8_lossy(l).trim().is_empty() {
                last = Some(l);
            }
        }
        match last {
            None => Ok(None),
            Some(l) => {
                let t = std::str::from_utf8(l).map_err(|e| LErr::Value(format!("'utf-8' codec can't decode: {}", e)))?;
                Ok(Some(jloads(t)?))
            }
        }
    }

    /// `_append_unlocked(item, user, tz)`
    pub fn append(&self, item: &Value, user: Option<&Value>, tz: &Value) -> LR<Value> {
        let last = self.last()?;
        let g = |k: &str| -> String { py_str(item.get(k).unwrap_or(&json!(""))) };
        let id = match &last {
            Some(l) => {
                let n = l.get("id").and_then(|x| x.as_i64()).ok_or_else(|| LErr::Value("'id'".into()))?;
                json!(n + 1)
            }
            None => json!(1),
        };
        let tzs = if truthy(tz) { py_str(tz) } else { "UTC".to_string() };
        let mut rec = Map::new();
        rec.insert("id".into(), id);
        rec.insert("time".into(), json!(iso(now_micros())));
        rec.insert("tz".into(), json!(trunc_chars(&tzs, 64)));
        rec.insert("user".into(), match user {
            Some(u) => field(u, "name"),
            None => json!(g("instance")),
        });
        rec.insert("full_name".into(), match user {
            Some(u) => u.get("full_name").cloned().unwrap_or_else(|| json!("")),
            None => json!(""),
        });
        rec.insert("map".into(), json!(trunc_chars(&g("map"), 64)));
        rec.insert("instance".into(), json!(trunc_chars(&g("instance"), 256)));
        rec.insert("attribute".into(), json!(trunc_chars(&g("attribute"), 128)));
        rec.insert("old".into(), json!(trunc_chars(&g("old"), 1000)));
        rec.insert("new".into(), json!(trunc_chars(&g("new"), 1000)));
        rec.insert("reason".into(), json!(trunc_chars(&g("reason"), 1000)));
        rec.insert("event".into(), json!(trunc_chars(&g("event"), 128)));
        rec.insert("prev".into(), match &last {
            Some(l) => l.get("hash").cloned().ok_or_else(|| LErr::Value("'hash'".into()))?,
            None => json!(""),
        });
        let mut rec = Value::Object(rec);
        let h = self.code(&rec);
        rec["hash"] = json!(h);
        let mut f = fsrw(fs::OpenOptions::new().create(true).append(true).open(&self.audit_path))?;
        fsrw(f.write_all(format!("{}\n", dumps(&rec, UTF8)).as_bytes()))?;
        Ok(rec)
    }

    pub fn record(&self, items: &Value, user: Option<&Value>, tz: &Value) -> LR<Value> {
        if user.is_none() {
            return Err(LErr::Denied("Login required".into()));
        }
        let list = match items {
            Value::Array(a) if a.len() <= 500 => a,
            _ => return verr("Audit records must be a list (max 500)"),
        };
        let mut ids = Vec::new();
        for it in list {
            if !it.is_object() {
                return verr("Audit record must be an object");
            }
            let state = self.audit_state(&field(it, "map"), &field(it, "attribute"));
            if state == "off" {
                continue;
            }
            if state == "on" && py_str(it.get("reason").unwrap_or(&json!(""))).trim().is_empty() {
                return verr("Must specify reason(s) for change");
            }
            ids.push(self.append(it, user, tz)?["id"].clone());
        }
        Ok(json!({"recorded": ids.len(), "ids": ids}))
    }

    pub fn history(&self, q: &Value) -> LR<Value> {
        let recs = self.records()?;
        let qm = as_obj(q)?;
        let gs = |k: &str| -> Value {
            match qm.get(k) {
                Some(v) if truthy(v) => v.clone(),
                _ => json!(""),
            }
        };
        let (mp, inst, usr, attr) = (gs("map"), gs("instance"), gs("user"), gs("attribute"));
        let text = gs("text");
        let case = qm.get("match_case").map(truthy).unwrap_or(false);
        let whole = qm.get("whole_word").map(truthy).unwrap_or(false);
        let (frm, to) = (gs("from"), gs("to"));
        let mut out: Vec<Value> = Vec::new();
        let strs = |v: &Value, what: &str| -> LR<String> {
            match v {
                Value::String(s) => Ok(s.clone()),
                other => verr(format!("'<' not supported between instances of 'str' and '{}'", type_name(other)).replace("'<'", if what == "gt" { "'>'" } else { "'<'" })),
            }
        };
        for r in &recs {
            let rget = |k: &str| -> LR<Value> { r.get(k).cloned().ok_or_else(|| LErr::Value(format!("'{}'", k))) };
            if truthy(&mp) && !py_eq(&rget("map")?, &mp) {
                continue;
            }
            if truthy(&inst) {
                let a = match &inst {
                    Value::String(s) => s.to_lowercase(),
                    other => return verr(format!("'in <string>' requires string as left operand, not {}", type_name(other))),
                };
                let b = match rget("instance")? {
                    Value::String(s) => s.to_lowercase(),
                    other => return verr(format!("'{}' object has no attribute 'lower'", type_name(&other))),
                };
                if !b.contains(&a) {
                    continue;
                }
            }
            if truthy(&usr) && !py_eq(&rget("user")?, &usr) {
                continue;
            }
            if truthy(&attr) && !py_eq(&rget("attribute")?, &attr) {
                continue;
            }
            let day: String = match rget("time")? {
                Value::String(s) => s.chars().take(10).collect(),
                other => return verr(format!("'{}' object is not subscriptable", type_name(&other))),
            };
            if (truthy(&frm) && day < strs(&frm, "lt")?) || (truthy(&to) && day > strs(&to, "gt")?) {
                continue;
            }
            if truthy(&text) {
                let needle = match &text {
                    Value::String(s) => s.clone(),
                    other => return Err(LErr::Other(format!("TypeError: expected str instance, {} found", type_name(other)))),
                };
                let mut hit = false;
                for k in ["instance", "attribute", "old", "new", "reason", "user", "event"] {
                    let hay = py_str(r.get(k).unwrap_or(&json!("")));
                    if search_literal(&hay, &needle, !case, whole) {
                        hit = true;
                        break;
                    }
                }
                if !hit {
                    continue;
                }
            }
            let mut rr = r.as_object().cloned().unwrap_or_default();
            let verified = compare_digest(&json!(self.code(r)), r.get("hash").unwrap_or(&json!("")))?;
            rr.insert("verified".into(), Value::Bool(verified));
            out.push(Value::Object(rr));
        }
        let mut instances: Vec<((String, String), i64)> = Vec::new();
        for r in &out {
            let key = (sfield(r, "map").to_string(), sfield(r, "instance").to_string());
            match instances.iter_mut().find(|(k, _)| *k == key) {
                Some((_, c)) => *c += 1,
                None => instances.push((key, 1)),
            }
        }
        instances.sort_by(|a, b| a.0.cmp(&b.0));
        let limit = match qm.get("limit") {
            Some(v) if truthy(v) => py_int(v)?,
            _ => 5000,
        };
        let start = {
            let s = -limit;
            if s < 0 {
                (out.len() as i64 + s).max(0)
            } else {
                s.min(out.len() as i64)
            }
        } as usize;
        let total = out.len();
        let mut users: Vec<String> = Vec::new();
        let mut maps: Vec<String> = Vec::new();
        for r in &recs {
            for (k, dst) in [("user", &mut users), ("map", &mut maps)] {
                match r.get(k) {
                    Some(Value::String(s)) => {
                        if !dst.contains(s) {
                            dst.push(s.clone());
                        }
                    }
                    Some(other) => return verr(format!("unsupported type {}", type_name(other))),
                    None => return verr(format!("'{}'", k)),
                }
            }
        }
        users.sort();
        maps.sort();
        Ok(json!({"records": out[start..].to_vec(), "total": total,
                  "instances": instances.iter().map(|((m, i), c)| json!({"map": m, "instance": i, "count": c})).collect::<Vec<_>>(),
                  "users": users, "maps": maps}))
    }

    pub fn count(&self) -> LR<usize> {
        Ok(self.records()?.len())
    }

    pub fn backup(&self) -> LR<String> {
        let v = json!({"format": "opentrace-audit", "schema": 1, "exported": iso(now_micros()), "records": self.records()?});
        Ok(dumps(&v, Opts { ensure_ascii: false, sort_keys: false, indent: Some(0) }))
    }

    pub fn restore(&mut self, text: &Value, user: Option<&Value>) -> LR<Value> {
        let data = jloads_value(text)?;
        let dm = as_obj(&data)?;
        if !dm.get("format").map(|f| py_eq(f, &json!("opentrace-audit"))).unwrap_or(false) {
            return verr("Not an OpenTrace audit backup");
        }
        let cur = self.records()?;
        let mut have: Vec<String> = Vec::new();
        for r in &cur {
            have.push(r.get("hash").and_then(|h| h.as_str()).ok_or_else(|| LErr::Value("'hash'".into()))?.to_string());
        }
        let mut add: Vec<Value> = Vec::new();
        for r in py_iter(dm.get("records").unwrap_or(&json!([])))? {
            if let Value::Object(m) = &r {
                let h = m.get("hash").cloned().unwrap_or(Value::Null);
                if matches!(h, Value::Array(_) | Value::Object(_)) {
                    return verr(format!("unhashable type: '{}'", type_name(&h)));
                }
                let known = matches!(&h, Value::String(s) if have.contains(s));
                if !known {
                    add.push(r.clone());
                }
            }
        }
        for r in &add {
            for k in ["id", "time", "user", "map", "instance", "attribute", "old", "new", "reason", "hash"] {
                if !r.get(k).is_some() {
                    return verr("Malformed audit record in backup");
                }
            }
        }
        let mut all: Vec<Value> = cur.clone();
        all.extend(add.iter().cloned());
        let mut keyed: Vec<(String, f64, Value)> = Vec::new();
        for r in all {
            let t = match r.get("time") {
                Some(Value::String(s)) => s.clone(),
                _ => return verr("'<' not supported between instances of 'str' and 'int'"),
            };
            let id = match r.get("id") {
                Some(Value::Number(n)) => n.as_f64().unwrap(),
                _ => return verr("'<' not supported between instances of 'str' and 'int'"),
            };
            keyed.push((t, id, r));
        }
        keyed.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)));
        let allr: Vec<Value> = keyed.into_iter().map(|(_, _, r)| r).collect();
        self.rewrite(&allr)?;
        self.append(&json!({"map": "Audit", "instance": "Audit Records", "attribute": "restored", "old": "", "new": add.len().to_string(), "reason": "", "event": "Restore Audit Records"}), user, &Value::Null)?;
        let mut unverified = 0;
        for r in &add {
            if !compare_digest(&json!(self.code(r)), &r["hash"])? {
                unverified += 1;
            }
        }
        Ok(json!({"restored": add.len(), "unverified": unverified}))
    }

    fn rewrite(&self, records: &[Value]) -> LR<()> {
        let tmp = self.audit_path.with_extension("tmp");
        let mut s = String::new();
        for r in records {
            s.push_str(&dumps(r, UTF8));
            s.push('\n');
        }
        fsrw(fs::write(&tmp, s))?;
        fsrw(fs::rename(&tmp, &self.audit_path))
    }

    pub fn delete(&mut self, frm: &Value, to: &Value, user: Option<&Value>) -> LR<Value> {
        let recs = self.records()?;
        let (f, t) = (py_str_or_empty(frm), py_str_or_empty(to));
        let keep: Vec<Value> = if !f.is_empty() || !t.is_empty() {
            if f.is_empty() || t.is_empty() {
                return verr("Start Date or End Date cannot be Empty!");
            }
            if t < f {
                return verr("End Date should be greater then Start Date!");
            }
            let mut k = Vec::new();
            for r in &recs {
                let day: String = match r.get("time") {
                    Some(Value::String(s)) => s.chars().take(10).collect(),
                    _ => return verr("'time'"),
                };
                if !(f <= day && day <= t) {
                    k.push(r.clone());
                }
            }
            k
        } else {
            vec![]
        };
        self.rewrite(&keep)?;
        let n = recs.len() - keep.len();
        let old = if !f.is_empty() { format!("{} - {}", f, t) } else { "all".to_string() };
        self.append(&json!({"map": "Audit", "instance": "Audit Records", "attribute": "deleted", "old": old, "new": n.to_string(), "reason": "", "event": "Delete Audit Records"}), user, &Value::Null)?;
        Ok(json!({"deleted": n}))
    }

    // -------------------------------------------------------- client view
    pub fn session_view(&self, user: Option<&Value>) -> LR<Value> {
        let d = &self.doc;
        let (pu, groups, rights, orights) = match user {
            Some(u) => {
                let r: Vec<Value> = self.rights(u)?.into_iter().filter(|(_, v)| *v).map(|(k, _)| json!(k)).collect();
                let mut orr = Map::new();
                for g in arr!(self, "security_groups") {
                    let mut rs = self.object_rights(Some(u), &field(&g, "name"));
                    rs.sort();
                    orr.insert(sfield(&g, "name").to_string(), json!(rs));
                }
                (self.public_user(u)?, json!(self.groups_of(u)), Value::Array(r), Value::Object(orr))
            }
            None => (Value::Null, json!([]), json!([]), json!({})),
        };
        let login_users: Vec<Value> = arr!(self, "users").iter().filter(|u| !truthy(&field(u, "disabled"))).map(|u| field(u, "name")).collect();
        let sgs: Vec<Value> = arr!(self, "security_groups").iter().map(|g| field(g, "name")).collect();
        Ok(json!({"require_login": d["settings"]["require_login"], "user": pu, "groups": groups, "rights": rights,
                  "audit": {"enabled": d["audit"]["enabled"], "maps": d["audit"]["maps"], "reasons": d["audit"]["reasons"], "free_text": field(&d["audit"], "free_text")},
                  "esig": d["esig"], "policies": {"min_length": d["policies"]["min_length"]}, "login_users": login_users,
                  "security_groups": sgs, "object_rights": orights}))
    }
}

/// Approximation of CPython's expat `ParseError` text for the errors `roxmltree` reports.
pub fn expat_message(text: &str, e: &roxmltree::Error) -> String {
    use roxmltree::Error as E;
    let chars: Vec<char> = text.chars().collect();
    let end_pos = || {
        let mut line = 1;
        let mut col = 0;
        for c in &chars {
            if *c == '\n' {
                line += 1;
                col = 0;
            } else {
                col += 1;
            }
        }
        (line, col)
    };
    let at = |p: &roxmltree::TextPos, dc: i64| (p.row, (p.col as i64 + dc).max(0) as u32);
    let (msg, (l, c)): (&str, (u32, u32)) = match e {
        E::NoRootNode | E::UnclosedRootNode | E::UnexpectedEndOfStream => {
            let lt = chars.iter().rposition(|c| *c == '<');
            let gt = chars.iter().rposition(|c| *c == '>');
            let in_tag = match (lt, gt) {
                (Some(a), Some(b)) => a > b,
                (Some(_), None) => true,
                _ => false,
            };
            if in_tag && !text.trim_end().ends_with("-->") {
                let line = 1 + chars[..lt.unwrap()].iter().filter(|c| **c == '\n').count() as u32;
                ("unclosed token", (line, 0))
            } else {
                let (l, c) = end_pos();
                ("no element found", (l, c))
            }
        }
        E::UnexpectedCloseTag(_, _, p) => ("mismatched tag", at(p, 1)),
        E::DuplicatedAttribute(_, p) => ("duplicate attribute", at(p, -1)),
        E::UnknownNamespace(_, p) => ("unbound prefix", (p.row, 0)),
        E::UnknownEntityReference(_, p) => ("undefined entity", at(p, -1)),
        E::UnknownToken(p) | E::InvalidName(p) => {
            let first = chars.iter().position(|c| !c.is_whitespace());
            // byte-exact prefix before the error position: if it already is a complete document the rest is junk
            let mut off = 0usize;
            let (mut row, mut col) = (1u32, 1u32);
            for (i, ch) in chars.iter().enumerate() {
                if row == p.row && col == p.col {
                    off = i;
                    break;
                }
                if *ch == '\n' {
                    row += 1;
                    col = 1;
                } else {
                    col += 1;
                }
                off = i + 1;
            }
            let prefix: String = chars[..off].iter().collect();
            match first {
                Some(i) if chars[i] != '<' && p.row == 1 && p.col as usize == i + 1 && (chars[i].is_alphabetic() || chars[i] == '_') => ("syntax error", at(p, -1)),
                _ if off > 0 && roxmltree::Document::parse(&prefix).is_ok() => ("junk after document element", at(p, -1)),
                _ => ("not well-formed (invalid token)", at(p, -1)),
            }
        }
        E::MalformedEntityReference(p) => ("not well-formed (invalid token)", at(p, 0)),
        E::InvalidCharacterData(p) => ("not well-formed (invalid token)", at(p, -2)),
        E::NonXmlChar(_, p) | E::InvalidChar(_, _, p) | E::InvalidChar2(_, _, p) | E::InvalidString(_, p) | E::InvalidAttributeValue(p) => {
            ("not well-formed (invalid token)", at(p, -1))
        }
        other => return format!("{}", other),
    };
    format!("{}: line {}, column {}", msg, l, c)
}

fn py_str_or_empty(v: &Value) -> String {
    if truthy(v) {
        py_str(v)
    } else {
        String::new()
    }
}

fn now_susp(u: &Value) -> LR<i64> {
    parse_iso(&field(u, "suspended_until"))
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn lower1(c: char) -> char {
    let mut it = c.to_lowercase();
    match (it.next(), it.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

/// `re.search(r'\b?<escaped needle>\b?', hay, re.I?)`.
fn search_literal(hay: &str, needle: &str, icase: bool, whole: bool) -> bool {
    let h: Vec<char> = hay.chars().collect();
    let n: Vec<char> = needle.chars().collect();
    let eq = |a: char, b: char| if icase { a == b || lower1(a) == lower1(b) } else { a == b };
    let boundary = |pos: usize| -> bool {
        let before = pos > 0 && is_word(h[pos - 1]);
        let after = pos < h.len() && is_word(h[pos]);
        before != after
    };
    if n.len() > h.len() {
        return false;
    }
    for i in 0..=(h.len() - n.len()) {
        if (0..n.len()).all(|j| eq(h[i + j], n[j])) && (!whole || (boundary(i) && boundary(i + n.len()))) {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------- request dispatch
pub fn security_request(store: &mut SecurityStore, ticket: Option<&str>, request: &Value) -> LR<Value> {
    let req = match request {
        Value::Object(m) => m,
        _ => return verr("Security request must be an object"),
    };
    let g = |k: &str| -> Value { req.get(k).cloned().unwrap_or(Value::Null) };
    let act_v = g("action");
    let act = act_v.as_str().unwrap_or("");
    let user = store.session_user(ticket)?;
    let u = user.as_ref();
    let admin = format!("{}/General/May run application", ACC);
    let falsy_to = |v: Value, d: Value| if truthy(&v) { v } else { d };
    match act {
        "session" => return store.session_view(u),
        "login" => {
            let name = py_str(req.get("name").unwrap_or(&json!("")));
            let pw = py_str(req.get("password").unwrap_or(&json!("")));
            let mut r = store.login(&name, &pw)?;
            if r["ticket"].as_str().map(|t| !t.is_empty()).unwrap_or(false) {
                let uu = store.user(&g("name"));
                let view = store.session_view(uu.as_ref())?;
                if let (Value::Object(rm), Value::Object(vm)) = (&mut r, view) {
                    for (k, v) in vm {
                        rm.insert(k, v);
                    }
                }
            }
            return Ok(r);
        }
        "logout" => return store.logout(ticket),
        "change_password" => {
            let n = g("name");
            let name = if truthy(&n) { n } else { u.map(|x| field(x, "name")).unwrap_or(Value::Null) };
            return store.change_password(&py_str(&name), &g("old"), &g("new"));
        }
        "verify" => return store.verify_signature(u, &g("name"), &g("password"), &g("group"), &g("action_name")),
        "record" => return store.record(&g("records"), u, &g("tz")),
        _ => {}
    }
    if user.is_none() {
        return Err(LErr::Denied("Login required".into()));
    }
    let prev = g("previous");
    let pick = |a: &str, b: &str| -> String { if truthy(&prev) { a.to_string() } else { b.to_string() } };
    match act {
        "admin_state" => {
            store.require(u, &[&admin])?;
            store.admin_state()
        }
        "save_user" => {
            store.require(u, &[&pick(&format!("{}/Users/Modify", ACC), &format!("{}/Users/Create", ACC))])?;
            store.save_user(&falsy_to(g("user"), json!({})), &prev)
        }
        "delete_user" => {
            store.require(u, &[&format!("{}/Users/Delete", ACC)])?;
            store.delete_user(&g("name"), u)
        }
        "save_profile" => {
            store.require(u, &[&pick(&format!("{}/Profiles/Modify", ACC), &format!("{}/Profiles/Create", ACC))])?;
            store.save_profile(&falsy_to(g("profile"), json!({})), &prev)
        }
        "delete_profile" => {
            store.require(u, &[&format!("{}/Profiles/Delete", ACC)])?;
            store.delete_profile(&g("name"))
        }
        "save_group" => {
            store.require(u, &[&pick(&format!("{}/User Groups/Modify", ACC), &format!("{}/User Groups/Create", ACC))])?;
            store.save_group(&falsy_to(g("group"), json!({})), &prev)
        }
        "delete_group" => {
            store.require(u, &[&format!("{}/User Groups/Delete", ACC)])?;
            store.delete_group(&g("name"))
        }
        "save_security_group" => {
            store.require(u, &[&pick(&format!("{}/Security Groups/Modify", ACC), &format!("{}/Security Groups/Create", ACC))])?;
            store.save_security_group(&falsy_to(g("group"), json!({})), &prev)
        }
        "save_policies" => {
            store.require(u, &[&format!("{}/General/Change Password Policies", ACC)])?;
            store.save_policies(&falsy_to(g("policies"), json!({})))
        }
        "save_settings" => {
            store.require(u, &[&admin])?;
            let settings = store.save_settings(&falsy_to(g("settings"), json!({})), u)?;
            let mut out = json!({"settings": settings});
            if truthy(&settings["require_login"]) && !store.tickets.iter().any(|(t, _)| Some(t.as_str()) == ticket) {
                let t = token_urlsafe(24);
                out["ticket"] = json!(t);
                store.tickets.push((t, sfield(u.unwrap(), "name").to_string()));
            }
            Ok(out)
        }
        "export" => {
            store.require(u, &[&format!("{}/General/Export Security Data", ACC)])?;
            Ok(json!({"text": store.export_security()}))
        }
        "import" => {
            store.require(u, &[&format!("{}/General/Import Security Data", ACC)])?;
            store.import_security(&req.get("text").cloned().unwrap_or_else(|| json!("")), u)
        }
        "audit_config" => {
            store.require(u, &[&format!("{}/Map Configuration Tool/May run application", AUDIT)])?;
            Ok(store.doc["audit"].clone())
        }
        "save_audit" => {
            store.require(u, &[&format!("{}/Map Configuration Tool/May run application", AUDIT)])?;
            store.save_audit(&falsy_to(g("audit"), json!({})), u)
        }
        "export_maps" => {
            store.require(u, &[&format!("{}/Map Configuration Tool/May run application", AUDIT)])?;
            Ok(json!({"xml": store.export_maps()}))
        }
        "parse_maps" => {
            store.require(u, &[&format!("{}/Map Configuration Tool/May run application", AUDIT)])?;
            store.parse_maps(&req.get("xml").cloned().unwrap_or_else(|| json!("")))
        }
        "history" => {
            store.require(u, &[&format!("{}/Audit History Viewer/May run application", AUDIT)])?;
            store.history(&falsy_to(g("query"), json!({})))
        }
        "count" => {
            store.require(u, &[&format!("{}/Audit History Viewer/May run application", AUDIT), &format!("{}/Audit Management/Backup Audit Records", AUDIT)])?;
            Ok(json!({"count": store.count()?}))
        }
        "backup" => {
            store.require(u, &[&format!("{}/Audit Management/Backup Audit Records", AUDIT)])?;
            Ok(json!({"text": store.backup()?}))
        }
        "restore" => {
            store.require(u, &[&format!("{}/Audit Management/Restore Audit Records", AUDIT)])?;
            store.restore(&req.get("text").cloned().unwrap_or_else(|| json!("")), u)
        }
        "delete_records" => {
            store.require(u, &[&format!("{}/Audit Management/Delete Audit Records", AUDIT)])?;
            store.delete(&g("from"), &g("to"), u)
        }
        "save_esig" => {
            store.require(u, &[&format!("{}/ESig Administration Tool/May run application", ESIG)])?;
            store.save_esig(&falsy_to(g("groups"), json!([])), u)
        }
        _ => verr(format!("Unknown security action: {}", py_str(&act_v))),
    }
}
