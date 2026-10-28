//! Port of `genemapper_open/project_store.py`: SQLite-backed local project with append-only review history.
//!
//! Identifiers (`uuid4`) and timestamps come from [`new_uuid`]/[`now_iso`]; [`set_deterministic`] swaps them for
//! counters so Rust and the Python reference can be compared byte for byte.
//! Known gaps: integers beyond 64 bits in stored JSON, and `datetime.fromisoformat` for exotic input is mirrored
//! from CPython's C implementation (see [`iso_has_tz`]).
use crate::profiles::strip;
use crate::pullup_quality::{py_eq, sha256_hex};
use crate::pyjson::{self, LoadErr};
use rusqlite::{params, Connection, OpenFlags};
use serde_json::{json, Map, Value};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: i64 = 2;
pub const EXPORT_FORMAT: &str = "opentrace-local-project";
pub const MAX_JSON_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_EXPORT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_RUN_INPUT_BYTES: usize = 16 * 1024 * 1024;
const ACTIONS: [&str; 9] = [
    "accept", "exclude", "change-label", "note", "undo", "add-allele", "delete-artifact-label", "override-gq",
    "override-cgq",
];
const PEAK_EDITS: [&str; 2] = ["add-allele", "delete-artifact-label"];
const OVERRIDES: [&str; 2] = ["override-gq", "override-cgq"];

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// `ProjectStoreError` (a ValueError).
    Store,
    /// KeyError / TypeError / sqlite DatabaseError / JSON errors: anything the Python code lets through or maps to
    /// "malformed"/"not a supported SQLite project".
    Other,
}

#[derive(Debug, Clone)]
pub struct SErr {
    pub kind: Kind,
    pub msg: String,
}

pub type R<T> = Result<T, SErr>;

pub fn se(msg: impl Into<String>) -> SErr {
    SErr { kind: Kind::Store, msg: msg.into() }
}
pub fn other(msg: impl Into<String>) -> SErr {
    SErr { kind: Kind::Other, msg: msg.into() }
}
impl std::fmt::Display for SErr {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&self.msg)
    }
}
fn db(e: rusqlite::Error) -> SErr {
    match &e {
        rusqlite::Error::SqliteFailure(_, Some(m)) => other(m.clone()),
        _ => other(e.to_string()),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// identifiers and clock

thread_local! {
    static DET: Cell<bool> = Cell::new(false);
    static UUID_N: Cell<u64> = Cell::new(0);
    static NOW_N: Cell<u64> = Cell::new(0);
    static ENTROPY: RefCell<Option<std::fs::File>> = RefCell::new(None);
}

/// Replace uuid4 / now by counters (`00000000-0000-4000-8000-%012x`, `2024-01-01T00:00:SS.NNNNNN+00:00`).
pub fn set_deterministic(on: bool) {
    DET.with(|d| d.set(on));
    UUID_N.with(|c| c.set(0));
    NOW_N.with(|c| c.set(0));
}

pub fn new_uuid() -> String {
    if DET.with(Cell::get) {
        let n = UUID_N.with(|c| {
            c.set(c.get() + 1);
            c.get()
        });
        return format!("00000000-0000-4000-8000-{:012x}", n);
    }
    let mut b = [0u8; 16];
    let ok = ENTROPY.with(|e| {
        use std::io::Read;
        let mut e = e.borrow_mut();
        if e.is_none() {
            *e = std::fs::File::open("/dev/urandom").ok();
        }
        e.as_mut().map_or(false, |f| f.read_exact(&mut b).is_ok())
    });
    if !ok {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let t = t ^ ((std::process::id() as u128) << 64) ^ (&b as *const _ as usize as u128);
        b.copy_from_slice(&t.to_le_bytes());
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{:02x}", x)).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `datetime.now(timezone.utc).isoformat(timespec="microseconds")`.
pub fn now_iso() -> String {
    if DET.with(Cell::get) {
        let n = NOW_N.with(|c| {
            c.set(c.get() + 1);
            c.get()
        });
        return format!("2024-01-01T00:00:{:02}.{:06}+00:00", n % 60, n);
    }
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap();
    let secs = d.as_secs() as i64;
    let (y, mo, da) = civil(secs.div_euclid(86400));
    let rem = secs.rem_euclid(86400);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:06}+00:00",
        y,
        mo,
        da,
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        d.subsec_micros()
    )
}

// ---------------------------------------------------------------------------------------------------------------
// validation helpers

pub fn text(v: &Value, name: &str, limit: usize) -> R<String> {
    match v {
        Value::String(s) if !strip(s).is_empty() && s.chars().count() <= limit => Ok(s.clone()),
        _ => Err(se(format!("{} must be nonempty text of at most {} characters", name, limit))),
    }
}
pub fn text_s(s: &str, name: &str, limit: usize) -> R<String> {
    text(&Value::String(s.to_string()), name, limit)
}

pub fn sha(v: &Value) -> R<String> {
    match v {
        Value::String(s) if s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) => {
            Ok(s.clone())
        }
        _ => Err(se("SHA-256 must be 64 lowercase hexadecimal characters")),
    }
}

fn digits(b: &[u8], p: usize, n: usize) -> Option<(u32, usize)> {
    if p + n > b.len() {
        return None;
    }
    let mut v = 0u32;
    for i in 0..n {
        let c = b[p + i];
        if !c.is_ascii_digit() {
            return None;
        }
        v = v * 10 + (c - b'0') as u32;
    }
    Some((v, p + n))
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                29
            } else {
                28
            }
        }
    }
}

fn weekday_jan4(y: i64) -> i64 {
    // days since 0001-01-01 style ordinal -> weekday (Mon=0) of Jan 4th
    let a = y - 1;
    let ordinal = a * 365 + a / 4 - a / 100 + a / 400 + 4;
    (ordinal + 6) % 7
}

fn iso_weeks(y: i64) -> i64 {
    // a year has 53 ISO weeks if Jan 1 is Thursday, or leap year and Jan 1 is Wednesday
    let jan1 = (weekday_jan4(y) - 3).rem_euclid(7);
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    if jan1 == 3 || (leap && jan1 == 2) {
        53
    } else {
        52
    }
}

fn valid_ymd(y: u32, m: u32, d: u32) -> bool {
    (1..=9999).contains(&y) && (1..=12).contains(&m) && d >= 1 && d <= days_in_month(y as i64, m)
}

/// Date part of CPython's `parse_isoformat_date`; returns false on failure/invalid date.
fn parse_date(b: &[u8], len: usize) -> bool {
    let at = |i: usize| -> u8 { b.get(i).copied().unwrap_or(0) };
    let (year, mut p) = match digits(b, 0, 4) {
        Some(x) => x,
        None => return false,
    };
    let uses_sep = at(p) == b'-';
    if uses_sep {
        p += 1;
    }
    if at(p) == b'W' {
        p += 1;
        let (week, np) = match digits(b, p, 2) {
            Some(x) => x,
            None => return false,
        };
        p = np;
        let mut day = 1u32;
        if p < len {
            if uses_sep {
                if at(p) != b'-' {
                    return false;
                }
                p += 1;
            }
            match digits(b, p, 1) {
                Some((d, _)) => day = d,
                None => return false,
            }
        }
        if !(1..=9999).contains(&year) || week < 1 || week as i64 > iso_weeks(year as i64) || !(1..=7).contains(&day) {
            return false;
        }
        // iso_to_ymd result must be within datetime range
        let jan4 = (weekday_jan4(year as i64)) as i64;
        let days = (week as i64 - 1) * 7 + day as i64 - 1 - jan4;
        // ordinal of Jan 4 plus days
        let a = year as i64 - 1;
        let ord = a * 365 + a / 4 - a / 100 + a / 400 + 4 + days;
        return ord >= 1 && ord <= 3652059;
    }
    let (month, np) = match digits(b, p, 2) {
        Some(x) => x,
        None => return false,
    };
    p = np;
    let c = at(p);
    p += 1;
    if uses_sep && c != b'-' {
        return false;
    }
    if !uses_sep {
        p -= 1;
    }
    let (day, _) = match digits(b, p, 2) {
        Some(x) => x,
        None => return false,
    };
    valid_ymd(year, month, day)
}

/// `parse_hh_mm_ss_ff`: returns (rc, hour, minute, second, usec) where rc<0 is an error, 1 means trailing data.
fn parse_hhmmssff(b: &[u8], start: usize, end: usize) -> (i32, [u32; 3], u32) {
    let at = |i: usize| -> u8 { b.get(i).copied().unwrap_or(0) };
    let mut vals = [0u32; 3];
    let mut p = start;
    let mut has_sep = true;
    let mut broke = false;
    for i in 0..3 {
        match digits(&b[..end.min(b.len())], p, 2) {
            Some((v, np)) => {
                vals[i] = v;
                p = np;
            }
            None => return (-3, vals, 0),
        }
        let c = at(p);
        p += 1;
        if i == 0 {
            has_sep = c == b':';
        }
        if p >= end {
            return ((c != 0) as i32, vals, 0);
        } else if has_sep && c == b':' {
            continue;
        } else if c == b'.' || c == b',' {
            broke = true;
            break;
        } else if !has_sep {
            p -= 1;
        } else {
            return (-4, vals, 0);
        }
    }
    let _ = broke;
    let remains = end.saturating_sub(p);
    let to_parse = remains.min(6);
    let micro;
    match digits(&b[..end.min(b.len())], p, to_parse) {
        Some((v, np)) => {
            micro = v;
            p = np;
        }
        None => return (-3, vals, 0),
    };
    let mut micro = micro;
    if to_parse == 0 {
        // parse_digits with zero digits succeeds with 0 in C; correction[-1] is undefined behaviour there, mirror as error
        return (-3, vals, 0);
    }
    if to_parse < 6 {
        micro *= [100000, 10000, 1000, 100, 10][to_parse - 1];
    }
    while at(p).is_ascii_digit() {
        p += 1;
    }
    ((at(p) != 0) as i32, vals, micro)
}

/// Mirror of `datetime.fromisoformat(value)`: `Err(())` when Python raises ValueError, `Ok(has_tz)` otherwise.
pub fn iso_has_tz(value: &str) -> Result<bool, ()> {
    let b = value.as_bytes();
    let len = b.len();
    if len < 7 {
        return Err(());
    }
    let sep = {
        if len == 7 {
            7
        } else if b[4] == b'-' {
            if b[5] == b'W' {
                if len < 8 {
                    return Err(());
                }
                if len > 8 && b[8] == b'-' {
                    // YYYY-Www-D (10), or YYYY-Www followed by '-' and an hour (8)
                    if len > 9 && b[9].is_ascii_digit() && (len == 10 || !b[10].is_ascii_digit()) {
                        10
                    } else {
                        8
                    }
                } else {
                    8
                }
            } else {
                10
            }
        } else if b[4] == b'W' {
            let mut idx = 7;
            while idx < len && b[idx].is_ascii_digit() {
                idx += 1;
            }
            if idx < 9 {
                idx
            } else if idx % 2 == 0 {
                7
            } else {
                8
            }
        } else {
            8
        }
    };
    if sep > len {
        return Err(());
    }
    let mut date_b = b[..sep.min(len)].to_vec();
    date_b.push(0);
    if !parse_date(&date_b, sep) {
        return Err(());
    }
    if len <= sep {
        return Ok(false);
    }
    let mut p = sep;
    if b[p] & 0x80 == 0 {
        p += 1;
    } else {
        p += match b[p] & 0xf0 {
            0xe0 => 3,
            0xf0 => 4,
            _ => 2,
        };
    }
    if p > len {
        return Err(());
    }
    let t = &b[p..];
    let tl = t.len();
    let mut tb = t.to_vec();
    tb.push(0);
    let mut tz_pos = 0;
    while tz_pos < tl && !matches!(tb[tz_pos], b'Z' | b'+' | b'-') {
        tz_pos += 1;
    }
    let (rv, hms, _us) = parse_hhmmssff(&tb, 0, tz_pos);
    if rv < 0 {
        return Err(());
    }
    if hms[0] > 23 || hms[1] > 59 || hms[2] > 59 {
        return Err(());
    }
    if tz_pos == tl {
        return if rv == 1 { Err(()) } else { Ok(false) };
    }
    if tb[tz_pos] == b'Z' {
        return if tb.get(tz_pos + 1).copied().unwrap_or(0) != 0 { Err(()) } else { Ok(true) };
    }
    if tz_pos > 0 && matches!(tb[tz_pos - 1], b'.' | b',') {
        return Err(());
    }
    let (rv2, tz, tzus) = parse_hhmmssff(&tb, tz_pos + 1, tl);
    if rv2 != 0 {
        return Err(());
    }
    let total = tz[0] as i64 * 3600 + tz[1] as i64 * 60 + tz[2] as i64;
    // offset must be strictly within one day (microseconds included)
    if total * 1_000_000 + tzus as i64 >= 86_400_000_000 {
        return Err(());
    }
    Ok(true)
}

/// `_timestamp(value)`: `Null` means "now".
pub fn timestamp(v: &Value) -> R<String> {
    if v.is_null() {
        return Ok(now_iso());
    }
    let s = text(v, "timestamp", 64)?;
    match iso_has_tz(&s.replace('Z', "+00:00")) {
        Err(()) => Err(se("timestamp must be ISO 8601 with timezone")),
        Ok(false) => Err(se("timestamp must include a timezone")),
        Ok(true) => Ok(s),
    }
}

pub fn canonical(v: &Value, limit: usize, depth: usize) -> R<String> {
    pyjson::canonical(v, limit, depth).map_err(se)
}
pub fn canonical_d(v: &Value, limit: usize) -> R<String> {
    canonical(v, limit, 32)
}
pub fn digest(c: &str) -> String {
    sha256_hex(c.as_bytes())
}
fn object<'a>(v: &'a Value, name: &str) -> R<&'a Map<String, Value>> {
    match v {
        Value::Object(m) => Ok(m),
        _ => Err(se(format!("{} must be a JSON object", name))),
    }
}
/// Python `d[key]` on a decoded export: any shape problem is KeyError/TypeError.
fn field<'a>(v: &'a Value, key: &str) -> R<&'a Value> {
    match v {
        Value::Object(m) => m.get(key).ok_or_else(|| other(format!("KeyError: '{}'", key))),
        _ => Err(other("TypeError")),
    }
}
fn is_int(v: &Value) -> bool {
    matches!(v, Value::Number(n) if n.is_i64() || n.is_u64())
}
fn str_of<'a>(v: Option<&'a Value>) -> Option<&'a str> {
    match v {
        Some(Value::String(s)) => Some(s),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------------------------
// review overlay folding

const OVERRIDE_GQ: f64 = 1.0;
const OVERRIDE_CGQ: f64 = 0.5;

fn edit_policy() -> Value {
    json!({
        "add_allele": "user_call_on_marker_linked_peak_clears_pullup_and_spike",
        "add_allele_undo": "full_event_reversal_restores_all_machine_flags",
        "original_add_undo_restores_spike": false,
        "omr_flag_modeled": false,
        "delete_artifact_label": "hide_label_keep_flags",
        "delete_artifact_label_undoable": false,
        "override_gq": "marker_gq_forced_to_1.0_marker_stale_sample_freshness_unchanged",
        "override_cgq": "sample_cgq_forced_to_0.5_override_pass_category",
        "ladder_override_run_effects_modeled": false,
        "numeric_quality_recomputed": false,
    })
}

fn get_arr<'a>(result: &'a Value, key: &str) -> &'a [Value] {
    match result.get(key) {
        Some(Value::Array(a)) => a,
        _ => &[],
    }
}

fn gq_rows(result: &Value) -> HashMap<String, &Map<String, Value>> {
    let mut out = HashMap::new();
    let rows = result.get("quality").and_then(|q| q.get("marker_gq")).and_then(|g| g.get("markers"));
    if let (Some(Value::Object(_)), Some(Value::Array(rows))) = (result.get("quality").and_then(|q| q.get("marker_gq")), rows) {
        for r in rows {
            if let Value::Object(m) = r {
                if let Some(Value::String(s)) = m.get("marker") {
                    out.insert(s.clone(), m); // later duplicates win, like the dict comprehension
                }
            }
        }
    }
    out
}

/// `_targets(result)`: ordered `(kind, id, item)`; calls first, then peaks.
fn targets(result: &Value) -> R<Vec<(&'static str, String, &Map<String, Value>)>> {
    let mut out = Vec::new();
    let mut seen: HashSet<(&'static str, String)> = HashSet::new();
    for (kind, plural) in [("call", "calls"), ("peak", "peaks")] {
        let records: &[Value] = match result.get(plural) {
            None => &[],
            Some(Value::Array(a)) => a,
            Some(_) => return Err(se(format!("result.{} must be an array", plural))),
        };
        for item in records {
            let m = object(item, &format!("result.{} item", plural))?;
            let id = text(m.get("id").unwrap_or(&Value::Null), &format!("{} id", kind), 256)?;
            if !seen.insert((kind, id.clone())) {
                return Err(se(format!("duplicate {} id: {}", kind, id)));
            }
            out.push((kind, id, m));
        }
    }
    Ok(out)
}

fn peak_flag(peak: &Map<String, Value>, name: &str) -> bool {
    let listed = match peak.get("flags") {
        None => false,
        Some(Value::Object(m)) => matches!(m.get(name), Some(Value::Bool(true))),
        Some(Value::Array(a)) => a.iter().any(|x| matches!(x, Value::String(s) if s == name)),
        Some(_) => false,
    };
    matches!(peak.get(name), Some(Value::Bool(true))) || listed
}

#[derive(Clone, Debug)]
struct Ctx {
    marker: Option<String>,
    pullup: bool,
    spike: bool,
    label: Option<String>,
    calls: Vec<String>,
}

fn peak_contexts(result: &Value) -> HashMap<String, Ctx> {
    let peaks = get_arr(result, "peaks");
    let mut ctxs: HashMap<String, Ctx> = HashMap::new();
    for p in peaks {
        let Value::Object(peak) = p else { continue };
        let state = peak.get("pullup_classification").and_then(|c| c.get("state"));
        let mut label = match state.and_then(|s| s.get("artifact_label")) {
            Some(Value::String(s)) => Some(s.clone()),
            _ => None,
        };
        let pullup = peak_flag(peak, "pullup");
        let spike = peak_flag(peak, "spike");
        if !pullup {
            label = None;
        } else if !matches!(&label, Some(s) if !strip(s).is_empty()) {
            label = Some("SPU".into());
        }
        let id = str_of(peak.get("id")).unwrap_or("").to_string();
        ctxs.insert(id, Ctx { marker: None, pullup, spike, label, calls: Vec::new() });
    }
    if let Some(Value::Array(markers)) = result.get("allele_calling").and_then(|c| c.get("markers")) {
        for marker in markers {
            let Some(name) = str_of(marker.get("name")) else { continue };
            if let Some(Value::Array(links)) = marker.get("peak_review") {
                for e in links {
                    if let Some(Value::Number(n)) = e.get("peak_index") {
                        if let Some(i) = n.as_i64() {
                            if n.is_i64() && 0 <= i && (i as usize) < peaks.len() {
                                let pid = str_of(peaks[i as usize].get("id")).unwrap_or("");
                                if let Some(c) = ctxs.get_mut(pid) {
                                    c.marker = Some(name.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    for call in get_arr(result, "calls") {
        if let Some(src) = str_of(call.get("source_peak_id")) {
            if let Some(c) = ctxs.get_mut(src) {
                c.calls.push(str_of(call.get("id")).unwrap_or("").to_string());
            }
        }
    }
    ctxs
}

#[derive(Clone, Debug)]
struct Ev {
    event_id: String,
    action: String,
    target_kind: String,
    target_id: Option<String>,
    actor: String,
    value: Value,
    reverses: Option<String>,
}

struct Overlay {
    kind: &'static str,
    id: String,
    accepted: Option<bool>,
    label: Value,
    notes: Vec<Value>,
    event_ids: Vec<String>,
    peak: Option<PeakOverlay>,
}
struct PeakOverlay {
    marker: Option<String>,
    user_allele: Option<Value>,
    machine_flags: (bool, bool),
    flags: (bool, bool),
    machine_label: Option<String>,
    label: Option<String>,
}

fn opt_s(s: &Option<String>) -> Value {
    s.as_ref().map_or(Value::Null, |x| Value::String(x.clone()))
}
fn flags_v(f: (bool, bool)) -> Value {
    json!({"pullup": f.0, "spike": f.1})
}

impl Overlay {
    fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("target_kind".into(), json!(self.kind));
        m.insert("target_id".into(), json!(self.id));
        m.insert("accepted".into(), self.accepted.map_or(Value::Null, Value::Bool));
        m.insert("label".into(), self.label.clone());
        m.insert("notes".into(), Value::Array(self.notes.clone()));
        m.insert("event_ids".into(), json!(self.event_ids));
        if let Some(p) = &self.peak {
            m.insert("marker".into(), opt_s(&p.marker));
            m.insert("user_allele".into(), p.user_allele.clone().unwrap_or(Value::Null));
            m.insert("machine_artifact_flags".into(), flags_v(p.machine_flags));
            m.insert("artifact_flags".into(), flags_v(p.flags));
            m.insert("machine_artifact_label".into(), opt_s(&p.machine_label));
            m.insert("artifact_label".into(), opt_s(&p.label));
        }
        Value::Object(m)
    }
}

fn review_quality(result: &Value, overlays: &HashMap<(String, String), Overlay>, events: &[Ev]) -> Value {
    use std::collections::{BTreeMap, BTreeSet};
    let mut latest: HashMap<(String, &str), String> = HashMap::new();
    for e in events {
        if e.target_kind == "call" {
            let field = match e.action.as_str() {
                "accept" | "exclude" => Some("accepted"),
                "change-label" => Some("label"),
                _ => None,
            };
            if let Some(f) = field {
                latest.insert((e.target_id.clone().unwrap_or_default(), f), e.event_id.clone());
            }
        }
    }
    let mut affected: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut active_ids: HashSet<String> = HashSet::new();
    let mut call_markers: BTreeSet<String> = BTreeSet::new();
    for call in get_arr(result, "calls") {
        let marker = match call.get("marker") {
            Some(Value::String(s)) if !strip(s).is_empty() => Some(s.clone()),
            _ => None,
        };
        if let Some(m) = &marker {
            call_markers.insert(m.clone());
        }
        let id = str_of(call.get("id")).unwrap_or("").to_string();
        let ov = &overlays[&("call".to_string(), id.clone())];
        let mut event_id = None;
        if ov.accepted == Some(false) {
            event_id = latest.get(&(id.clone(), "accepted")).cloned();
        } else if !py_eq(&ov.label, call.get("label").unwrap_or(&Value::Null)) {
            event_id = latest.get(&(id.clone(), "label")).cloned();
        }
        if let Some(eid) = event_id {
            active_ids.insert(eid.clone());
            if let Some(m) = marker {
                affected.entry(m).or_default().insert(eid);
            }
        }
    }
    let contexts = peak_contexts(result);
    let mut pullup_cleared: Vec<String> = Vec::new();
    let mut gq_overrides: HashMap<String, String> = HashMap::new();
    let mut cgq_override: Option<String> = None;
    for e in events {
        if e.action == "override-gq" {
            let t = e.target_id.clone().unwrap_or_default();
            gq_overrides.insert(t.clone(), e.event_id.clone());
            call_markers.insert(t.clone());
            affected.entry(t).or_default().insert(e.event_id.clone());
            continue;
        }
        if e.action == "override-cgq" {
            cgq_override = Some(e.event_id.clone());
            continue;
        }
        if e.target_kind != "peak" || !PEAK_EDITS.contains(&e.action.as_str()) {
            continue;
        }
        let ctx = &contexts[e.target_id.as_deref().unwrap_or("")];
        active_ids.insert(e.event_id.clone());
        if let Some(m) = &ctx.marker {
            call_markers.insert(m.clone());
            affected.entry(m.clone()).or_default().insert(e.event_id.clone());
        }
        if e.action == "add-allele" && ctx.pullup {
            pullup_cleared.push(e.event_id.clone());
        }
    }
    let quality = result.get("quality");
    let gq_val = quality.and_then(|q| q.get("marker_gq"));
    let empty = Map::new();
    let gq: &Map<String, Value> = match gq_val {
        Some(Value::Object(m)) => m,
        _ => &empty,
    };
    let machine_status: Option<String> = str_of(gq.get("status")).map(String::from);
    let mut marker_status: HashMap<String, Option<String>> = HashMap::new();
    if let Some(Value::Array(rows)) = gq.get("markers") {
        for r in rows {
            if let Some(Value::String(m)) = r.get("marker") {
                if !strip(m).is_empty() {
                    marker_status.insert(m.clone(), str_of(r.get("status")).map(String::from));
                }
            }
        }
    }
    let override_ids: HashSet<&String> = gq_overrides.values().collect();
    let ordered_ids: Vec<String> = events
        .iter()
        .filter(|e| active_ids.contains(&e.event_id) || override_ids.contains(&e.event_id))
        .map(|e| e.event_id.clone())
        .collect();
    let any_stale_marker = marker_status.values().any(|s| s.as_deref() == Some("stale"));
    let ms_v = opt_s(&machine_status);
    let mut all_markers: BTreeSet<String> = call_markers.clone();
    all_markers.extend(marker_status.keys().cloned());
    let rows_by_marker = gq_rows(result);
    let empty_set = BTreeSet::new();
    let mut marker_rows: Vec<Value> = Vec::new();
    for marker in &all_markers {
        let stale = affected.contains_key(marker)
            || machine_status.as_deref() == Some("stale")
            || marker_status.get(marker).map_or(false, |s| s.as_deref() == Some("stale"));
        let mstat = match marker_status.get(marker) {
            Some(s) => opt_s(s),
            None => ms_v.clone(),
        };
        let aff = affected.get(marker).unwrap_or(&empty_set);
        let mut row = Map::new();
        row.insert("marker".into(), json!(marker));
        row.insert("freshness".into(), json!(if stale { "stale" } else { "unchanged" }));
        row.insert("machine_status".into(), mstat);
        row.insert("active_event_ids".into(), json!(ordered_ids.iter().filter(|i| aff.contains(*i)).collect::<Vec<_>>()));
        if let Some(eid) = gq_overrides.get(marker) {
            let score = rows_by_marker.get(marker).and_then(|r| r.get("score"));
            let machine_gq = match score {
                Some(Value::Object(s)) => s.get("gq").cloned().unwrap_or(Value::Null),
                _ => Value::Null,
            };
            row.insert(
                "gq_override".into(),
                json!({"gq": OVERRIDE_GQ, "override": true, "event_id": eid, "machine_gq": machine_gq}),
            );
        }
        marker_rows.push(Value::Object(row));
    }
    let sample_stale = !active_ids.is_empty() || machine_status.as_deref() == Some("stale") || any_stale_marker;
    let mut summary = Map::new();
    summary.insert("freshness".into(), json!(if sample_stale { "stale" } else { "unchanged" }));
    summary.insert("machine_status".into(), ms_v);
    if let Some(eid) = cgq_override {
        summary.insert(
            "cgq_override".into(),
            json!({"cgq": OVERRIDE_CGQ, "category": "override_pass", "override": true, "event_id": eid,
                   "machine_category": gq.get("sample_category").cloned().unwrap_or(Value::Null)}),
        );
    }
    let mut out = Map::new();
    out.insert("policy".into(), json!("net_call_interpretation"));
    out.insert("affected_markers".into(), json!(affected.keys().collect::<Vec<_>>()));
    out.insert("active_event_ids".into(), json!(ordered_ids));
    out.insert("markers".into(), Value::Array(marker_rows));
    out.insert("sample_summary".into(), Value::Object(summary));
    if let Some(Value::Object(q)) = quality {
        if matches!(q.get("pullup"), Some(Value::Object(_))) {
            out.insert(
                "pullup_profile".into(),
                json!({"freshness": if pullup_cleared.is_empty() { "unchanged" } else { "stale" },
                       "active_event_ids": pullup_cleared}),
            );
        }
    }
    Value::Object(out)
}

// ---------------------------------------------------------------------------------------------------------------
// the store

const DDL: &str = "
CREATE TABLE store_info (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    schema_version INTEGER NOT NULL, project_id TEXT NOT NULL,
    name TEXT NOT NULL, created_at TEXT NOT NULL
);
CREATE TABLE sample_records (
    sample_id TEXT PRIMARY KEY, source_sha256 TEXT NOT NULL,
    metadata_json TEXT NOT NULL, created_at TEXT NOT NULL
);
CREATE TABLE analysis_records (
    analysis_id TEXT PRIMARY KEY,
    sample_id TEXT NOT NULL REFERENCES sample_records(sample_id),
    input_sha256 TEXT NOT NULL, algorithm_json TEXT NOT NULL,
    algorithm_sha256 TEXT NOT NULL, settings_json TEXT NOT NULL,
    settings_sha256 TEXT NOT NULL, result_json TEXT NOT NULL,
    result_sha256 TEXT NOT NULL, status TEXT NOT NULL, created_at TEXT NOT NULL
);
CREATE TABLE review_log (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id TEXT UNIQUE NOT NULL,
    analysis_id TEXT NOT NULL REFERENCES analysis_records(analysis_id),
    action TEXT NOT NULL, target_kind TEXT NOT NULL, target_id TEXT,
    actor TEXT NOT NULL, reason TEXT NOT NULL, created_at TEXT NOT NULL,
    value_json TEXT NOT NULL,
    reverses_event_id TEXT REFERENCES review_log(event_id)
);
CREATE TRIGGER analysis_source_binding BEFORE INSERT ON analysis_records
WHEN NEW.input_sha256 != (SELECT source_sha256 FROM sample_records WHERE sample_id=NEW.sample_id)
BEGIN SELECT RAISE(ABORT, 'analysis input hash differs from sample'); END;
";

const RUN_DDL: &str = "
CREATE TABLE runs (
    run_id TEXT PRIMARY KEY, name TEXT NOT NULL, created_at TEXT NOT NULL
);
CREATE TABLE run_revisions (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    revision_id TEXT UNIQUE NOT NULL,
    run_id TEXT NOT NULL REFERENCES runs(run_id),
    parent_revision_id TEXT,
    actor TEXT NOT NULL, reason TEXT NOT NULL, created_at TEXT NOT NULL,
    inputs_json TEXT NOT NULL, inputs_sha256 TEXT NOT NULL,
    outputs_json TEXT NOT NULL, outputs_sha256 TEXT NOT NULL,
    UNIQUE(run_id, revision_id),
    FOREIGN KEY(run_id, parent_revision_id) REFERENCES run_revisions(run_id, revision_id)
);
CREATE UNIQUE INDEX run_single_root ON run_revisions(run_id) WHERE parent_revision_id IS NULL;
CREATE UNIQUE INDEX run_single_child ON run_revisions(parent_revision_id) WHERE parent_revision_id IS NOT NULL;
";

pub struct ProjectStore {
    conn: Connection,
    pub path: Option<PathBuf>,
    depth: Cell<usize>,
    sp: Cell<usize>,
}

fn load(s: &str) -> R<Value> {
    pyjson::loads(s, false).map_err(|e| other(e.message()))
}

/// Arguments of `append_review` (Python keyword arguments); `Null` means `None`.
pub struct Review<'a> {
    pub analysis_id: &'a Value,
    pub action: &'a Value,
    pub actor: &'a Value,
    pub reason: &'a Value,
    pub target_kind: Value,
    pub target_id: &'a Value,
    pub value: &'a Value,
    pub event_id: &'a Value,
    pub created_at: &'a Value,
    pub reverses_event_id: &'a Value,
}

impl<'a> Review<'a> {
    pub fn new(analysis_id: &'a Value, action: &'a Value, actor: &'a Value, reason: &'a Value) -> Self {
        static NULL: Value = Value::Null;
        Review {
            analysis_id,
            action,
            actor,
            reason,
            target_kind: json!("analysis"),
            target_id: &NULL,
            value: &NULL,
            event_id: &NULL,
            created_at: &NULL,
            reverses_event_id: &NULL,
        }
    }
}

impl ProjectStore {
    fn new(conn: Connection, path: Option<PathBuf>) -> R<Self> {
        conn.execute_batch("PRAGMA foreign_keys=ON").map_err(db)?;
        Ok(ProjectStore { conn, path, depth: Cell::new(0), sp: Cell::new(0) })
    }

    /// `ProjectStore.memory(name)`.
    pub fn memory(name: &Value) -> R<Self> {
        let id = new_uuid();
        let created = now_iso();
        Self::memory_with(name, &json!(id), &json!(created), &json!(SCHEMA_VERSION))
    }

    pub fn from_json(document: &Value) -> R<Self> {
        Self::replay_json(document)
    }

    fn memory_with(name: &Value, project_id: &Value, created_at: &Value, version: &Value) -> R<Self> {
        let v = match version {
            Value::Number(n) if n.is_i64() && matches!(n.as_i64(), Some(1) | Some(2)) => n.as_i64().unwrap(),
            _ => return Err(se("unsupported project schema version")),
        };
        let store = Self::new(Connection::open_in_memory().map_err(db)?, None)?;
        store.conn.execute_batch(DDL).map_err(db)?;
        if v == 2 {
            store.conn.execute_batch(RUN_DDL).map_err(db)?;
        }
        let pid = text(project_id, "project_id", 256)?;
        let nm = text(name, "name", 1024)?;
        let ts = timestamp(created_at)?;
        store
            .conn
            .execute("INSERT INTO store_info VALUES (1, ?, ?, ?, ?)", params![v, pid, nm, ts])
            .map_err(db)?;
        let mut tables = vec!["store_info", "sample_records", "analysis_records", "review_log"];
        if v == 2 {
            tables.extend(["runs", "run_revisions"]);
        }
        for t in tables {
            for op in ["UPDATE", "DELETE"] {
                store
                    .conn
                    .execute_batch(&format!(
                        "CREATE TRIGGER immutable_{}_{} BEFORE {} ON {} BEGIN SELECT RAISE(ABORT, 'immutable project record'); END",
                        t,
                        op.to_lowercase(),
                        op,
                        t
                    ))
                    .map_err(db)?;
            }
        }
        store.conn.execute_batch(&format!("PRAGMA user_version={}", v)).map_err(db)?;
        Ok(store)
    }

    /// `ProjectStore.create(path, name)`: an existing destination is always refused.
    pub fn create(path: &Path, name: &Value) -> R<Self> {
        let id = new_uuid();
        let created = now_iso();
        let mem = Self::memory_with(name, &json!(id), &json!(created), &json!(SCHEMA_VERSION))?;
        mem.save_new(path)
    }

    fn save_new(&self, path: &Path) -> R<Self> {
        let dest = std::path::absolute(path).map_err(|e| other(e.to_string()))?;
        if path.as_os_str() == ":memory:" {
            return Err(se("an explicit filesystem destination is required"));
        }
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&dest) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(other(format!("[Errno 17] File exists: '{}'", dest.display())))
            }
            Err(e) => return Err(other(e.to_string())),
        }
        let copy = || -> R<Self> {
            let mut target = Connection::open(&dest).map_err(db)?;
            {
                let backup = rusqlite::backup::Backup::new(&self.conn, &mut target).map_err(db)?;
                backup.run_to_completion(1000, std::time::Duration::from_millis(0), None).map_err(db)?;
            }
            drop(target);
            Self::open(&dest)
        };
        match copy() {
            Ok(s) => Ok(s),
            Err(e) => {
                let _ = std::fs::remove_file(&dest);
                Err(e)
            }
        }
    }

    /// `ProjectStore.open(path)`: exact schema only.
    pub fn open(path: &Path) -> R<Self> {
        let dest = std::path::absolute(path).map_err(|e| other(e.to_string()))?;
        if !dest.is_file() {
            return Err(se("project file does not exist"));
        }
        let conn = Connection::open_with_flags(&dest, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_URI)
            .map_err(db)?;
        let store = Self::new(conn, Some(dest))?;
        let r = (|| -> R<()> {
            let version: i64 = store.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(db)?;
            let info: Vec<i64> = {
                let mut st = store.conn.prepare("SELECT schema_version FROM store_info").map_err(db)?;
                let rows = st.query_map([], |r| r.get::<_, i64>(0)).map_err(db)?;
                rows.collect::<Result<_, _>>().map_err(db)?
            };
            if (version != 1 && version != 2) || info.len() != 1 || info[0] != version {
                return Err(se("unsupported project schema version"));
            }
            let qc: String = store.conn.query_row("PRAGMA quick_check", [], |r| r.get(0)).map_err(db)?;
            if qc != "ok" {
                return Err(se("invalid SQLite project"));
            }
            let fk: Option<i64> = store
                .conn
                .query_row("PRAGMA foreign_key_check", [], |r| r.get::<_, i64>(1).or(Ok(0)))
                .ok();
            if fk.is_some() {
                return Err(se("project contains broken references"));
            }
            let doc = store.export_json()?;
            Self::replay_json(&Value::String(doc))?;
            Ok(())
        })();
        match r {
            Ok(()) => Ok(store),
            Err(e) if e.kind == Kind::Store => Err(e),
            Err(_) => Err(se("not a supported SQLite project").tap_other()),
        }
    }

    pub fn atomic<T>(&self, f: impl FnOnce() -> R<T>) -> R<T> {
        let outermost = self.depth.get() == 0;
        self.sp.set(self.sp.get() + 1);
        let name = format!("project_atomic_{}", self.sp.get());
        self.conn
            .execute_batch(&if outermost { "BEGIN IMMEDIATE".to_string() } else { format!("SAVEPOINT {}", name) })
            .map_err(db)?;
        self.depth.set(self.depth.get() + 1);
        let rollback = |s: &Self| {
            if outermost {
                let _ = s.conn.execute_batch("ROLLBACK");
            } else {
                let _ = s.conn.execute_batch(&format!("ROLLBACK TO {}; RELEASE {}", name, name));
            }
        };
        let r = match f() {
            Ok(v) => {
                let done = if outermost { self.conn.execute_batch("COMMIT") } else { self.conn.execute_batch(&format!("RELEASE {}", name)) };
                match done {
                    Ok(()) => Ok(v),
                    Err(e) => {
                        rollback(self);
                        Err(db(e))
                    }
                }
            }
            Err(e) => {
                rollback(self);
                Err(e)
            }
        };
        self.depth.set(self.depth.get() - 1);
        r
    }

    /// `upgraded()`: schema-2 in-memory copy.
    pub fn upgraded(&self) -> R<Self> {
        let mut doc = load(&self.export_json()?)?;
        let m = doc.as_object_mut().unwrap();
        m.insert("schema_version".into(), json!(SCHEMA_VERSION));
        if let Some(Value::Object(p)) = m.get_mut("project") {
            p.insert("schema_version".into(), json!(SCHEMA_VERSION));
        }
        m.entry("runs").or_insert_with(|| json!([]));
        m.entry("run_revisions").or_insert_with(|| json!([]));
        let c = canonical(&doc, MAX_EXPORT_BYTES, 40)?;
        Self::from_json(&Value::String(c))
    }

    pub fn project(&self) -> R<Map<String, Value>> {
        let r = self
            .conn
            .query_row("SELECT schema_version, project_id, name, created_at FROM store_info WHERE singleton=1", [], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?))
            })
            .map_err(db)?;
        let mut m = Map::new();
        m.insert("schema_version".into(), json!(r.0));
        m.insert("project_id".into(), json!(r.1));
        m.insert("name".into(), json!(r.2));
        m.insert("created_at".into(), json!(r.3));
        Ok(m)
    }
    fn version(&self) -> R<i64> {
        self.conn.query_row("SELECT schema_version FROM store_info WHERE singleton=1", [], |r| r.get(0)).map_err(db)
    }

    pub fn add_sample(&self, source_sha256: &Value, metadata: &Value, sample_id: &Value, created_at: &Value) -> R<String> {
        let identifier = text(&if sample_id.is_null() { json!(new_uuid()) } else { sample_id.clone() }, "sample_id", 256)?;
        let meta = object(metadata, "metadata")?;
        if meta.values().any(|v| v.is_object() || v.is_array()) {
            return Err(se("sample metadata contains scalar fields only; traces are not stored"));
        }
        let payload = canonical_d(metadata, 65536)?;
        self.atomic(|| {
            let s = sha(source_sha256)?;
            let ts = timestamp(created_at)?;
            self.conn
                .execute("INSERT INTO sample_records VALUES (?, ?, ?, ?)", params![identifier, s, payload, ts])
                .map_err(db)?;
            Ok(())
        })?;
        Ok(identifier)
    }

    pub fn list_samples(&self) -> R<Vec<Value>> {
        let mut st = self
            .conn
            .prepare("SELECT sample_id, source_sha256, metadata_json, created_at FROM sample_records ORDER BY sample_id")
            .map_err(db)?;
        let rows: Vec<(String, String, String, String)> = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .map_err(db)?
            .collect::<Result<_, _>>()
            .map_err(db)?;
        rows.into_iter()
            .map(|(a, b, c, d)| Ok(json!({"sample_id": a, "source_sha256": b, "metadata": load(&c)?, "created_at": d})))
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_analysis(
        &self,
        sample_id: &Value,
        input_sha256: &Value,
        algorithm: &Value,
        settings: &Value,
        result: &Value,
        status: &Value,
        analysis_id: &Value,
        created_at: &Value,
    ) -> R<String> {
        let identifier = text(&if analysis_id.is_null() { json!(new_uuid()) } else { analysis_id.clone() }, "analysis_id", 256)?;
        let sid = text(sample_id, "sample_id", 256)?;
        let input = sha(input_sha256)?;
        let alg = object(algorithm, "algorithm")?;
        text(alg.get("name").unwrap_or(&Value::Null), "algorithm.name", 256)?;
        text(alg.get("version").unwrap_or(&Value::Null), "algorithm.version", 256)?;
        let algorithm_json = canonical_d(algorithm, 65536)?;
        object(settings, "settings")?;
        let settings_json = canonical_d(settings, 262144)?;
        object(result, "result")?;
        let result_json = canonical_d(result, MAX_JSON_BYTES)?;
        targets(result)?;
        let st = text(status, "status", 128)?;
        let source: Option<String> = self
            .conn
            .query_row("SELECT source_sha256 FROM sample_records WHERE sample_id=?", params![sid], |r| r.get(0))
            .ok();
        if source.as_deref() != Some(input.as_str()) {
            return Err(se("analysis input SHA-256 must match the registered sample"));
        }
        self.atomic(|| {
            let ts = timestamp(created_at)?;
            self.conn
                .execute(
                    "INSERT INTO analysis_records VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    params![
                        identifier,
                        sid,
                        input,
                        algorithm_json,
                        digest(&algorithm_json),
                        settings_json,
                        digest(&settings_json),
                        result_json,
                        digest(&result_json),
                        st,
                        ts
                    ],
                )
                .map_err(db)?;
            Ok(())
        })?;
        Ok(identifier)
    }

    pub fn get_analysis(&self, analysis_id: &Value) -> R<Value> {
        let id = text(analysis_id, "analysis_id", 256)?;
        type Row = (String, String, String, String, String, String, String, String, String, String, String);
        let row: Option<Row> = self
            .conn
            .query_row("SELECT * FROM analysis_records WHERE analysis_id=?", params![id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?))
            })
            .ok();
        let Some(r) = row else { return Err(se("unknown analysis_id")) };
        let mut m = Map::new();
        m.insert("analysis_id".into(), json!(r.0));
        m.insert("sample_id".into(), json!(r.1));
        m.insert("input_sha256".into(), json!(r.2));
        m.insert("algorithm_sha256".into(), json!(r.4));
        m.insert("settings_sha256".into(), json!(r.6));
        m.insert("result_sha256".into(), json!(r.8));
        m.insert("status".into(), json!(r.9));
        m.insert("created_at".into(), json!(r.10));
        for (name, js, want) in [("algorithm", &r.3, &r.4), ("settings", &r.5, &r.6), ("result", &r.7, &r.8)] {
            let v = load(js)?;
            if &digest(&canonical_d(&v, MAX_JSON_BYTES)?) != want {
                return Err(se(format!("stored {} digest mismatch", name)));
            }
            m.insert(name.into(), v);
        }
        Ok(Value::Object(m))
    }

    pub fn list_analyses(&self) -> R<Vec<Value>> {
        let ids: Vec<String> = {
            let mut st = self.conn.prepare("SELECT analysis_id FROM analysis_records ORDER BY analysis_id").map_err(db)?;
            let rows = st.query_map([], |r| r.get(0)).map_err(db)?;
            rows.collect::<Result<_, _>>().map_err(db)?
        };
        ids.iter().map(|i| self.get_analysis(&json!(i))).collect()
    }

    fn require_runs(&self) -> R<()> {
        if self.version()? != 2 {
            return Err(se("run history requires an explicit schema2 upgrade"));
        }
        Ok(())
    }

    pub fn add_run(&self, name: &Value, run_id: &Value, created_at: &Value) -> R<String> {
        self.require_runs()?;
        let identifier = text(&if run_id.is_null() { json!(new_uuid()) } else { run_id.clone() }, "run_id", 256)?;
        self.atomic(|| {
            let nm = text(name, "run name", 1024)?;
            let ts = timestamp(created_at)?;
            self.conn.execute("INSERT INTO runs VALUES (?, ?, ?)", params![identifier, nm, ts]).map_err(db)?;
            Ok(())
        })?;
        Ok(identifier)
    }

    pub fn list_runs(&self) -> R<Vec<Value>> {
        if self.version()? == 1 {
            return Ok(vec![]);
        }
        let mut st = self.conn.prepare("SELECT run_id, name, created_at FROM runs ORDER BY run_id").map_err(db)?;
        let rows: Vec<(String, String, String)> = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(db)?
            .collect::<Result<_, _>>()
            .map_err(db)?;
        Ok(rows.into_iter().map(|(a, b, c)| json!({"run_id": a, "name": b, "created_at": c})).collect())
    }

    pub fn get_run(&self, run_id: &Value) -> R<Value> {
        self.require_runs()?;
        let id = text(run_id, "run_id", 256)?;
        let row: Option<(String, String, String)> = self
            .conn
            .query_row("SELECT run_id, name, created_at FROM runs WHERE run_id=?", params![id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .ok();
        match row {
            Some((a, b, c)) => Ok(json!({"run_id": a, "name": b, "created_at": c})),
            None => Err(se("unknown run_id")),
        }
    }

    pub fn get_run_revision(&self, revision_id: &Value) -> R<Value> {
        self.require_runs()?;
        let id = text(revision_id, "revision_id", 256)?;
        type Row = (i64, String, String, Option<String>, String, String, String, String, String, String, String);
        let row: Option<Row> = self
            .conn
            .query_row("SELECT * FROM run_revisions WHERE revision_id=?", params![id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?))
            })
            .ok();
        let Some(r) = row else { return Err(se("unknown revision_id")) };
        let mut m = Map::new();
        m.insert("sequence".into(), json!(r.0));
        m.insert("revision_id".into(), json!(r.1));
        m.insert("run_id".into(), json!(r.2));
        m.insert("parent_revision_id".into(), r.3.as_ref().map_or(Value::Null, |s| json!(s)));
        m.insert("actor".into(), json!(r.4));
        m.insert("reason".into(), json!(r.5));
        m.insert("created_at".into(), json!(r.6));
        m.insert("inputs_sha256".into(), json!(r.8));
        m.insert("outputs_sha256".into(), json!(r.10));
        for (name, js, want, limit) in [("inputs", &r.7, &r.8, MAX_RUN_INPUT_BYTES), ("outputs", &r.9, &r.10, MAX_JSON_BYTES)] {
            let v = load(js)?;
            if &digest(&canonical_d(&v, limit)?) != want {
                return Err(se(format!("stored run {} digest mismatch", name)));
            }
            m.insert(name.into(), v);
        }
        Ok(Value::Object(m))
    }

    /// `list_run_revisions(run_id=None)`.
    pub fn list_run_revisions(&self, run_id: &Value) -> R<Vec<Value>> {
        if self.version()? == 1 {
            if !run_id.is_null() {
                self.require_runs()?;
            }
            return Ok(vec![]);
        }
        let ids: Vec<String> = if run_id.is_null() {
            let mut st = self.conn.prepare("SELECT revision_id FROM run_revisions ORDER BY sequence").map_err(db)?;
            let rows = st.query_map([], |r| r.get(0)).map_err(db)?;
            rows.collect::<Result<_, _>>().map_err(db)?
        } else {
            self.get_run(run_id)?;
            let mut st = self.conn.prepare("SELECT revision_id FROM run_revisions WHERE run_id=? ORDER BY sequence").map_err(db)?;
            let rows = st.query_map(params![run_id.as_str().unwrap_or("")], |r| r.get(0)).map_err(db)?;
            rows.collect::<Result<_, _>>().map_err(db)?
        };
        ids.iter().map(|i| self.get_run_revision(&json!(i))).collect()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn append_run_revision(
        &self,
        run_id: &Value,
        inputs: &Value,
        outputs: &Value,
        parent_revision_id: &Value,
        actor: &Value,
        reason: &Value,
        revision_id: &Value,
        created_at: &Value,
    ) -> R<String> {
        self.require_runs()?;
        let identifier = text(&if revision_id.is_null() { json!(new_uuid()) } else { revision_id.clone() }, "revision_id", 256)?;
        let rid = text(run_id, "run_id", 256)?;
        let actor_s = text(actor, "actor", 256)?;
        let reason_s = text(reason, "reason", 4096)?;
        let timestamp_s = timestamp(created_at)?;
        let parent: Option<String> = if parent_revision_id.is_null() {
            None
        } else {
            Some(text(parent_revision_id, "parent_revision_id", 256)?)
        };
        let im = object(inputs, "run inputs")?;
        let inputs_json = canonical_d(inputs, MAX_RUN_INPUT_BYTES)?;
        let (Some(Value::Array(samples)), Value::Array(outs)) = (im.get("samples"), outputs) else {
            return Err(se("run inputs.samples and outputs must be arrays"));
        };
        let mut sources: HashMap<String, String> = HashMap::new();
        for sample in samples {
            let sm = object(sample, "run input sample")?;
            let key = text(sm.get("sample_key").unwrap_or(&Value::Null), "sample_key", 256)?;
            if sources.contains_key(&key) {
                return Err(se("duplicate input sample_key"));
            }
            let analysis = object(sm.get("analysis").unwrap_or(&Value::Null), "run input sample.analysis")?;
            sources.insert(key, sha(analysis.get("source_sha256").unwrap_or(&Value::Null))?);
        }
        let outputs_json = canonical_d(outputs, MAX_JSON_BYTES)?;
        let mut output_keys: HashSet<String> = HashSet::new();
        let mut output_ids: HashSet<String> = HashSet::new();
        for output in outs {
            let om = object(output, "run output")?;
            let want = ["sample_key", "analysis_id", "previous_analysis_id"];
            if om.len() != 3 || !want.iter().all(|k| om.contains_key(*k)) {
                return Err(se("run output requires sample_key, analysis_id and previous_analysis_id"));
            }
            let key = text(&om["sample_key"], "sample_key", 256)?;
            let aid = text(&om["analysis_id"], "analysis_id", 256)?;
            if !om["previous_analysis_id"].is_null() {
                text(&om["previous_analysis_id"], "previous_analysis_id", 256)?;
            }
            if output_keys.contains(&key) || output_ids.contains(&aid) {
                return Err(se("duplicate output sample_key or analysis_id"));
            }
            output_keys.insert(key);
            output_ids.insert(aid);
        }
        let src_keys: HashSet<String> = sources.keys().cloned().collect();
        if output_keys != src_keys {
            return Err(se("run outputs must cover exactly the input sample keys"));
        }
        self.atomic(|| {
            self.get_run(&json!(rid))?;
            let latest: Option<String> = self
                .conn
                .query_row("SELECT revision_id FROM run_revisions WHERE run_id=? ORDER BY sequence DESC LIMIT 1", params![rid], |r| r.get(0))
                .ok();
            if parent != latest {
                return Err(se("parent_revision_id must match the latest run revision"));
            }
            let mut previous_outputs: HashMap<String, String> = HashMap::new();
            if let Some(p) = &parent {
                let rev = self.get_run_revision(&json!(p))?;
                for item in rev["outputs"].as_array().unwrap() {
                    previous_outputs.insert(
                        item["sample_key"].as_str().unwrap_or("").to_string(),
                        item["analysis_id"].as_str().unwrap_or("").to_string(),
                    );
                }
            }
            let mut referenced: HashSet<String> = HashSet::new();
            for rev in self.list_run_revisions(&Value::Null)? {
                for item in rev["outputs"].as_array().unwrap() {
                    referenced.insert(item["analysis_id"].as_str().unwrap_or("").to_string());
                }
            }
            if output_ids.iter().any(|i| referenced.contains(i)) {
                return Err(se("run outputs require new analysis records"));
            }
            for output in outs {
                let key = output["sample_key"].as_str().unwrap().to_string();
                let prev = match &output["previous_analysis_id"] {
                    Value::Null => None,
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                };
                if prev != previous_outputs.get(&key).cloned() {
                    return Err(se("previous_analysis_id must match the parent sample output"));
                }
                let analysis = self.get_analysis(&output["analysis_id"])?;
                if analysis["input_sha256"].as_str() != sources.get(&key).map(String::as_str) {
                    return Err(se("run input source SHA-256 must match its output analysis"));
                }
            }
            self.conn
                .execute(
                    "INSERT INTO run_revisions (revision_id,run_id,parent_revision_id,actor,reason,created_at,\
                     inputs_json,inputs_sha256,outputs_json,outputs_sha256) VALUES (?,?,?,?,?,?,?,?,?,?)",
                    params![identifier, rid, parent, actor_s, reason_s, timestamp_s, inputs_json, digest(&inputs_json), outputs_json, digest(&outputs_json)],
                )
                .map_err(db)?;
            Ok(())
        })?;
        Ok(identifier)
    }

    pub fn analysis_run_contexts(&self, analysis_id: &Value) -> R<Vec<Value>> {
        self.get_analysis(analysis_id)?;
        let revisions = self.list_run_revisions(&Value::Null)?;
        let mut latest: HashMap<String, &Value> = HashMap::new();
        for r in &revisions {
            latest.insert(r["run_id"].as_str().unwrap().to_string(), r);
        }
        let aid = analysis_id.as_str().unwrap_or("");
        let mut out = Vec::new();
        for rev in &revisions {
            for output in rev["outputs"].as_array().unwrap() {
                if output["analysis_id"].as_str() != Some(aid) {
                    continue;
                }
                let current = latest[rev["run_id"].as_str().unwrap()];
                let current_id = current["outputs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|i| i["sample_key"] == output["sample_key"])
                    .map(|i| i["analysis_id"].clone());
                let fresh = match &current_id {
                    None => "removed",
                    Some(c) if c.as_str() == Some(aid) => "current",
                    _ => "superseded",
                };
                out.push(json!({
                    "run_id": rev["run_id"], "revision_id": rev["revision_id"],
                    "sample_key": output["sample_key"], "latest_revision_id": current["revision_id"],
                    "latest_analysis_id": current_id.unwrap_or(Value::Null),
                    "freshness": fresh,
                }));
            }
        }
        Ok(out)
    }

    fn events(&self, analysis_id: Option<&str>) -> R<Vec<Value>> {
        let sql = "SELECT sequence, event_id, analysis_id, action, target_kind, target_id, actor, reason, created_at, value_json, reverses_event_id FROM review_log";
        let mut st = if analysis_id.is_some() {
            self.conn.prepare(&format!("{} WHERE analysis_id=? ORDER BY sequence", sql)).map_err(db)?
        } else {
            self.conn.prepare(&format!("{} ORDER BY sequence", sql)).map_err(db)?
        };
        type Row = (i64, String, String, String, String, Option<String>, String, String, String, String, Option<String>);
        let map = |r: &rusqlite::Row| -> rusqlite::Result<Row> {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?))
        };
        let rows: Vec<Row> = match analysis_id {
            Some(a) => st.query_map(params![a], map).map_err(db)?.collect::<Result<_, _>>().map_err(db)?,
            None => st.query_map([], map).map_err(db)?.collect::<Result<_, _>>().map_err(db)?,
        };
        rows.into_iter()
            .map(|r| {
                let on = |o: &Option<String>| o.as_ref().map_or(Value::Null, |s| json!(s));
                let mut m = Map::new();
                m.insert("sequence".into(), json!(r.0));
                m.insert("event_id".into(), json!(r.1));
                m.insert("analysis_id".into(), json!(r.2));
                m.insert("action".into(), json!(r.3));
                m.insert("target_kind".into(), json!(r.4));
                m.insert("target_id".into(), on(&r.5));
                m.insert("actor".into(), json!(r.6));
                m.insert("reason".into(), json!(r.7));
                m.insert("created_at".into(), json!(r.8));
                m.insert("reverses_event_id".into(), on(&r.10));
                m.insert("value".into(), load(&r.9)?);
                Ok(Value::Object(m))
            })
            .collect()
    }

    /// `review_events(analysis_id=None)`.
    pub fn review_events(&self, analysis_id: &Value) -> R<Vec<Value>> {
        if analysis_id.is_null() {
            self.events(None)
        } else {
            self.get_analysis(analysis_id)?;
            self.events(analysis_id.as_str())
        }
    }

    pub fn append_review(&self, a: &Review) -> R<String> {
        let identifier = text(&if a.event_id.is_null() { json!(new_uuid()) } else { a.event_id.clone() }, "event_id", 256)?;
        let actor = text(a.actor, "actor", 256)?;
        let reason = text(a.reason, "reason", 4096)?;
        let action = text(a.action, "action", 128)?;
        let mut target_kind = text(&a.target_kind, "target_kind", 128)?;
        let timestamp_s = timestamp(a.created_at)?;
        if !ACTIONS.contains(&action.as_str()) {
            return Err(se("unsupported review action"));
        }
        let analysis = self.get_analysis(a.analysis_id)?;
        let result = &analysis["result"];
        let tg = targets(result)?;
        let mut target_id: Value = a.target_id.clone();
        if target_kind == "analysis" {
            if !target_id.is_null() || !["note", "undo", "override-cgq"].contains(&action.as_str()) {
                return Err(se("only notes and CGQ overrides may directly target an analysis"));
            }
        } else if target_kind == "marker" {
            let t = text(&target_id, "target_id", 256)?;
            if !["override-gq", "undo"].contains(&action.as_str()) {
                return Err(se("marker targets accept only override-gq"));
            }
            if !gq_rows(result).contains_key(&t) {
                return Err(se("override-gq requires a marker with a machine GQ row"));
            }
        } else {
            let t = text(&target_id, "target_id", 256)?;
            if !["call", "peak"].contains(&target_kind.as_str()) || !tg.iter().any(|(k, i, _)| *k == target_kind && *i == t) {
                return Err(se("unknown review target"));
            }
        }
        if action == "override-gq" && target_kind != "marker" {
            return Err(se("override-gq requires a marker target"));
        }
        if action == "override-cgq" {
            if target_kind != "analysis" {
                return Err(se("override-cgq targets the whole analysis"));
            }
            let ok = matches!(result.get("quality").and_then(|q| q.get("marker_gq")), Some(Value::Object(_)));
            if !ok {
                return Err(se("override-cgq requires machine marker GQ results"));
            }
        }
        if action == "change-label" || action == "note" {
            text(a.value, "review value", 4096)?;
        } else if action == "add-allele" {
            text(a.value, "allele label", 256)?;
        } else if !a.value.is_null() {
            return Err(se("this review action does not take a value"));
        }
        if PEAK_EDITS.contains(&action.as_str()) {
            if target_kind != "peak" {
                return Err(se(format!("{} requires a peak target", action)));
            }
            let ctxs = peak_contexts(result);
            let ctx = &ctxs[target_id.as_str().unwrap()];
            if action == "add-allele" {
                if ctx.marker.is_none() {
                    return Err(se("add-allele requires a peak linked to a marker window"));
                }
                if !ctx.calls.is_empty() {
                    return Err(se("peak already has a machine call; review that call instead"));
                }
            } else if ctx.label.is_none() {
                return Err(se("delete-artifact-label requires a peak with a machine artifact label"));
            }
        }
        let aid = a.analysis_id.as_str().unwrap_or("").to_string();
        let reverses = a.reverses_event_id;
        self.atomic(|| {
            if OVERRIDES.contains(&action.as_str()) {
                let rows = self.log_rows(&aid)?;
                let reversed: HashSet<&str> = rows.iter().filter(|r| r.1 == "undo").filter_map(|r| r.4.as_deref()).collect();
                if rows.iter().any(|r| {
                    r.1 == action && r.2 == target_kind && r.3.as_deref() == target_id.as_str() && !reversed.contains(r.0.as_str())
                }) {
                    return Err(se(format!("{} is already in effect for this target", action)));
                }
            }
            if PEAK_EDITS.contains(&action.as_str()) {
                let rows = self.log_rows(&aid)?;
                let reversed: HashSet<&str> = rows.iter().filter(|r| r.1 == "undo").filter_map(|r| r.4.as_deref()).collect();
                let active: HashSet<&str> = rows
                    .iter()
                    .filter(|r| {
                        PEAK_EDITS.contains(&r.1.as_str()) && r.2 == "peak" && r.3.as_deref() == target_id.as_str() && !reversed.contains(r.0.as_str())
                    })
                    .map(|r| r.1.as_str())
                    .collect();
                if active.contains(action.as_str()) || (action == "delete-artifact-label" && active.contains("add-allele")) {
                    return Err(se(format!("{} is already in effect for this peak", action)));
                }
            }
            let mut rev_id: Option<String> = None;
            if action == "undo" {
                let rid = match reverses {
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                };
                type Row = (String, String, String, Option<String>);
                let event: Option<Row> = rid.as_ref().and_then(|r| {
                    self.conn
                        .query_row("SELECT analysis_id, action, target_kind, target_id FROM review_log WHERE event_id=?", params![r], |x| {
                            Ok((x.get(0)?, x.get(1)?, x.get(2)?, x.get(3)?))
                        })
                        .ok()
                });
                let Some(ev) = event else {
                    return Err(se("undo requires an earlier non-undo event in this analysis"));
                };
                if ev.0 != aid || ev.1 == "undo" {
                    return Err(se("undo requires an earlier non-undo event in this analysis"));
                }
                if ev.1 == "delete-artifact-label" {
                    return Err(se("artifact label deletion is not undoable, as in the original command"));
                }
                let already: Option<i64> = self
                    .conn
                    .query_row("SELECT 1 FROM review_log WHERE reverses_event_id=?", params![rid], |r| r.get(0))
                    .ok();
                if already.is_some() {
                    return Err(se("event is already reversed"));
                }
                target_kind = ev.2.clone();
                target_id = ev.3.as_ref().map_or(Value::Null, |s| json!(s));
                rev_id = rid;
            } else if !reverses.is_null() {
                return Err(se("only undo may reference a reversed event"));
            }
            let vj = canonical_d(a.value, 65536)?;
            self.conn
                .execute(
                    "INSERT INTO review_log (event_id, analysis_id, action, target_kind, target_id, \
                     actor, reason, created_at, value_json, reverses_event_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    params![identifier, aid, action, target_kind, target_id.as_str(), actor, reason, timestamp_s, vj, rev_id],
                )
                .map_err(db)?;
            Ok(())
        })?;
        Ok(identifier)
    }

    /// (event_id, action, target_kind, target_id, reverses_event_id) of an analysis' log.
    #[allow(clippy::type_complexity)]
    fn log_rows(&self, aid: &str) -> R<Vec<(String, String, String, Option<String>, Option<String>)>> {
        let mut st = self
            .conn
            .prepare("SELECT event_id, action, target_kind, target_id, reverses_event_id FROM review_log WHERE analysis_id=?")
            .map_err(db)?;
        let rows = st
            .query_map(params![aid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
            .map_err(db)?
            .collect::<Result<_, _>>()
            .map_err(db)?;
        Ok(rows)
    }

    pub fn undo_review(&self, analysis_id: &Value, event_id: &Value, actor: &Value, reason: &Value) -> R<String> {
        let action = json!("undo");
        let mut r = Review::new(analysis_id, &action, actor, reason);
        r.reverses_event_id = event_id;
        self.append_review(&r)
    }

    pub fn effective_analysis(&self, analysis_id: &Value) -> R<Value> {
        let analysis = self.get_analysis(analysis_id)?;
        let events_v = self.review_events(analysis_id)?;
        let events: Vec<Ev> = events_v
            .iter()
            .map(|e| Ev {
                event_id: e["event_id"].as_str().unwrap().to_string(),
                action: e["action"].as_str().unwrap().to_string(),
                target_kind: e["target_kind"].as_str().unwrap().to_string(),
                target_id: e["target_id"].as_str().map(String::from),
                actor: e["actor"].as_str().unwrap().to_string(),
                value: e["value"].clone(),
                reverses: e["reverses_event_id"].as_str().map(String::from),
            })
            .collect();
        let result = &analysis["result"];
        let reversed: HashSet<&str> = events.iter().filter(|e| e.action == "undo").filter_map(|e| e.reverses.as_deref()).collect();
        let contexts = peak_contexts(result);
        let mut order: Vec<(String, String)> = Vec::new();
        let mut overlays: HashMap<(String, String), Overlay> = HashMap::new();
        for (kind, id, item) in targets(result)? {
            let key = (kind.to_string(), id.clone());
            let peak = if kind == "peak" {
                let c = &contexts[&id];
                Some(PeakOverlay {
                    marker: c.marker.clone(),
                    user_allele: None,
                    machine_flags: (c.pullup, c.spike),
                    flags: (c.pullup, c.spike),
                    machine_label: c.label.clone(),
                    label: c.label.clone(),
                })
            } else {
                None
            };
            order.push(key.clone());
            overlays.insert(
                key,
                Overlay { kind, id, accepted: None, label: item.get("label").cloned().unwrap_or(Value::Null), notes: vec![], event_ids: vec![], peak },
            );
        }
        let mut notes: Vec<Value> = Vec::new();
        let mut active: Vec<Ev> = Vec::new();
        for e in &events {
            if e.action == "undo" || reversed.contains(e.event_id.as_str()) {
                continue;
            }
            active.push(e.clone());
            if OVERRIDES.contains(&e.action.as_str()) {
                continue;
            }
            if e.target_kind == "analysis" {
                notes.push(json!({"event_id": e.event_id, "text": e.value}));
                continue;
            }
            let ov = overlays
                .get_mut(&(e.target_kind.clone(), e.target_id.clone().unwrap_or_default()))
                .ok_or_else(|| other("KeyError"))?;
            ov.event_ids.push(e.event_id.clone());
            match e.action.as_str() {
                "accept" | "exclude" => ov.accepted = Some(e.action == "accept"),
                "change-label" => ov.label = e.value.clone(),
                "note" => ov.notes.push(json!({"event_id": e.event_id, "text": e.value})),
                "add-allele" => {
                    let p = ov.peak.as_mut().ok_or_else(|| other("KeyError"))?;
                    p.user_allele = Some(json!({"label": e.value, "call_type": "USER", "marker": opt_s(&p.marker),
                        "event_id": e.event_id, "actor": e.actor}));
                    p.flags = (false, false);
                    p.label = None;
                }
                "delete-artifact-label" => {
                    let p = ov.peak.as_mut().ok_or_else(|| other("KeyError"))?;
                    p.label = Some(String::new());
                }
                _ => {}
            }
        }
        let mut user_calls: Vec<Value> = Vec::new();
        for k in &order {
            let ov = &overlays[k];
            if let Some(Value::Object(ua)) = ov.peak.as_ref().and_then(|p| p.user_allele.as_ref()) {
                let mut m = Map::new();
                m.insert("peak_id".into(), json!(ov.id));
                for (kk, vv) in ua {
                    m.insert(kk.clone(), vv.clone());
                }
                user_calls.push(Value::Object(m));
            }
        }
        let mut eff = Map::new();
        eff.insert("analysis_id".into(), a_id(analysis_id));
        eff.insert("machine_status".into(), analysis["status"].clone());
        eff.insert("machine_result".into(), analysis["result"].clone());
        eff.insert("review_targets".into(), Value::Array(order.iter().map(|k| overlays[k].to_value()).collect()));
        eff.insert("review_notes".into(), Value::Array(notes));
        eff.insert("review_event_count".into(), json!(events.len()));
        eff.insert("user_calls".into(), Value::Array(user_calls));
        eff.insert("edit_policy".into(), edit_policy());
        eff.insert("review_quality".into(), review_quality(result, &overlays, &active));
        if self.version()? == 2 {
            eff.insert("run_contexts".into(), Value::Array(self.analysis_run_contexts(analysis_id)?));
        }
        Ok(Value::Object(eff))
    }

    pub fn export_json(&self) -> R<String> {
        let version = self.version()?;
        let mut doc = Map::new();
        doc.insert("format".into(), json!(EXPORT_FORMAT));
        doc.insert("schema_version".into(), json!(version));
        doc.insert("project".into(), Value::Object(self.project()?));
        doc.insert("samples".into(), Value::Array(self.list_samples()?));
        doc.insert("analyses".into(), Value::Array(self.list_analyses()?));
        doc.insert("review_events".into(), Value::Array(self.review_events(&Value::Null)?));
        if version == 2 {
            doc.insert("runs".into(), Value::Array(self.list_runs()?));
            doc.insert("run_revisions".into(), Value::Array(self.list_run_revisions(&Value::Null)?));
        }
        canonical(&Value::Object(doc), MAX_EXPORT_BYTES, 40)
    }

    pub fn import_json(path: &Path, document: &Value) -> R<Self> {
        let store = Self::replay_json(document)?;
        store.save_new(path)
    }

    fn replay_json(document: &Value) -> R<Self> {
        let r = Self::replay_inner(document);
        match r {
            Err(e) if e.kind == Kind::Other => Err(se("malformed project export")),
            other => other,
        }
    }

    fn replay_inner(document: &Value) -> R<Self> {
        let doc = match document {
            Value::String(s) if s.len() <= MAX_EXPORT_BYTES => s,
            _ => return Err(se("invalid or oversized project export")),
        };
        let payload = match pyjson::loads(doc, true) {
            Ok(v) => v,
            Err(LoadErr::Dup(_)) => return Err(se("duplicate JSON object key")),
            Err(e) => return Err(other(e.message())),
        };
        object(&payload, "export")?;
        let canon = canonical(&payload, MAX_EXPORT_BYTES, 40)?;
        let version = payload.get("schema_version").cloned().unwrap_or(Value::Null);
        let version_ok = is_int(&version) && matches!(version.as_i64(), Some(1) | Some(2));
        if payload.get("format") != Some(&json!(EXPORT_FORMAT)) || !version_ok {
            return Err(se("unsupported export format or schema version"));
        }
        let v = version.as_i64().unwrap();
        let project = field(&payload, "project")?;
        object(project, "project")?;
        let ps = project.get("schema_version");
        if !ps.map_or(false, is_int) || ps.and_then(|x| x.as_i64()) != Some(v) {
            return Err(se("unsupported project schema version"));
        }
        let mut names = vec!["samples", "analyses", "review_events"];
        if v == 2 {
            names.extend(["runs", "run_revisions"]);
        }
        for f in &names {
            if !matches!(field(&payload, f)?, Value::Array(_)) {
                return Err(se(format!("{} must be an array", f)));
            }
        }
        let store = Self::memory_with(field(project, "name")?, field(project, "project_id")?, field(project, "created_at")?, &version)?;
        let arr = |k: &str| payload[k].as_array().unwrap();
        for s in arr("samples") {
            let (a, b, c, d) = (field(s, "source_sha256")?, field(s, "metadata")?, field(s, "sample_id")?, field(s, "created_at")?);
            store.add_sample(a, b, c, d)?;
        }
        for s in arr("analyses") {
            let f = |k: &str| field(s, k);
            let args = (f("sample_id")?, f("input_sha256")?, f("algorithm")?, f("settings")?, f("result")?, f("status")?, f("analysis_id")?, f("created_at")?);
            store.add_analysis(args.0, args.1, args.2, args.3, args.4, args.5, args.6, args.7)?;
        }
        for e in arr("review_events") {
            let f = |k: &str| field(e, k);
            let (aid, action, actor, reason, tk, tid, value, eid, cat, rev) = (
                f("analysis_id")?, f("action")?, f("actor")?, f("reason")?, f("target_kind")?,
                f("target_id")?, f("value")?, f("event_id")?, f("created_at")?, f("reverses_event_id")?,
            );
            let mut r = Review::new(aid, action, actor, reason);
            r.target_kind = tk.clone();
            r.target_id = tid;
            r.value = value;
            r.event_id = eid;
            r.created_at = cat;
            r.reverses_event_id = rev;
            store.append_review(&r)?;
        }
        if v == 2 {
            for run in arr("runs") {
                let (n, i, c) = (field(run, "name")?, field(run, "run_id")?, field(run, "created_at")?);
                store.add_run(n, i, c)?;
            }
            for rv in arr("run_revisions") {
                let f = |k: &str| field(rv, k);
                let (rid, inputs, outputs, parent, actor, reason, revid, cat) = (
                    f("run_id")?, f("inputs")?, f("outputs")?, f("parent_revision_id")?, f("actor")?, f("reason")?, f("revision_id")?, f("created_at")?,
                );
                store.append_run_revision(rid, inputs, outputs, parent, actor, reason, revid, cat)?;
            }
        }
        if store.export_json()? != canon {
            return Err(se("export round-trip mismatch (fields, order, hashes or sequence)"));
        }
        Ok(store)
    }
}

fn a_id(v: &Value) -> Value {
    v.clone()
}

trait TapOther {
    fn tap_other(self) -> SErr;
}
impl TapOther for SErr {
    /// `ProjectStoreError("not a supported SQLite project")` raised from a non-store exception.
    fn tap_other(self) -> SErr {
        self
    }
}
