//! Port of `genemapper_open/profiles.py` (Profile Manager import/export and Profile Comparison).
//!
//! Known gaps: `\d` in the Allele column regex is ASCII-only; Python `float()` of non-ASCII digits
//! is not accepted; csv.Error / TypeError paths return error strings that differ from Python's
//! exception types; generators avoid those inputs.
use crate::kit_definition::py_float_repr;
use crate::pullup_quality::{py_eq, truthy};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};

pub type Res<T> = Result<T, String>;

const MAX_PROFILES: usize = 5000;
const MAX_MARKERS: usize = 128;
const MAX_ALLELES: usize = 8;
const FIELD_LIMIT: usize = 131072;

fn fv(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

fn py_isspace(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r' | '\x1c' | '\x1d' | '\x1e' | '\x1f' | '\u{85}' | '\u{a0}' | '\u{1680}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}')
        || ('\u{2000}'..='\u{200a}').contains(&c)
}

fn strip(s: &str) -> &str {
    s.trim_matches(py_isspace)
}

fn str_repr(s: &str) -> String {
    let q = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::new();
    out.push(q);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == q => {
                out.push('\\');
                out.push(c)
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push(q);
    out
}

/// Python `str(value)` of a JSON-decoded value.
fn py_str(v: &Value) -> String {
    match v {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::String(s) => s.clone(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(u) = n.as_u64() {
                u.to_string()
            } else {
                py_float_repr(n.as_f64().unwrap())
            }
        }
        Value::Array(a) => format!("[{}]", a.iter().map(py_repr).collect::<Vec<_>>().join(", ")),
        Value::Object(m) => format!("{{{}}}", m.iter().map(|(k, x)| format!("{}: {}", str_repr(k), py_repr(x))).collect::<Vec<_>>().join(", ")),
    }
}

fn py_repr(v: &Value) -> String {
    match v {
        Value::String(s) => str_repr(s),
        other => py_str(other),
    }
}

/// Python `float(str)`.
fn py_float_parse(s: &str) -> Option<f64> {
    let t = strip(s);
    if t.is_empty() {
        return None;
    }
    let chars: Vec<char> = t.chars().collect();
    let mut cleaned = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if c == '_' {
            let ok = i > 0 && i + 1 < chars.len() && chars[i - 1].is_ascii_digit() && chars[i + 1].is_ascii_digit();
            if !ok {
                return None;
            }
        } else {
            cleaned.push(c);
        }
    }
    let lower = cleaned.to_ascii_lowercase();
    let body = lower.trim_start_matches(['+', '-']);
    if lower.len() - body.len() > 1 {
        return None;
    }
    if matches!(body, "inf" | "infinity" | "nan") {
        return lower.parse::<f64>().ok();
    }
    if !body.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | '+' | '-')) {
        return None;
    }
    lower.parse::<f64>().ok()
}

fn round1(x: f64) -> f64 {
    format!("{:.1}", x).parse::<f64>().unwrap()
}

#[derive(Clone)]
struct Profile {
    id: String,
    markers: Vec<(String, Vec<String>)>,
    #[allow(dead_code)]
    kind: Option<String>,
}

fn profile(raw: &Value, wh: &str) -> Res<Profile> {
    let raw = raw.as_object().ok_or_else(|| format!("{}: object required", wh))?;
    let pid = strip(&py_str(raw.get("id").unwrap_or(&json!("")))).to_string();
    if pid.is_empty() || pid.chars().count() > 120 {
        return Err(format!("{}: id of 1..120 characters required", wh));
    }
    let markers = match raw.get("markers") {
        Some(Value::Object(m)) if m.len() <= MAX_MARKERS => m,
        _ => return Err(format!("{}: markers object (at most {}) required", pid, MAX_MARKERS)),
    };
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for (name, alleles) in markers {
        let a = match alleles {
            Value::Array(a) if a.len() <= 64 => a,
            _ => return Err(format!("{}/{}: allele list required", pid, name)),
        };
        let labels: Vec<String> = a.iter().map(|x| strip(&py_str(x)).to_string()).filter(|s| !s.is_empty()).collect();
        out.push((name.clone(), labels));
    }
    let kind = match raw.get("type") {
        Some(Value::String(t)) if t == "Lab Reference" || t == "Custom Control" => Some(t.clone()),
        _ => None,
    };
    Ok(Profile { id: pid, markers: out, kind })
}

fn markers_json(m: &[(String, Vec<String>)]) -> Value {
    let mut o = Map::new();
    for (k, v) in m {
        o.insert(k.clone(), json!(v));
    }
    Value::Object(o)
}

type PerMarker = Vec<(String, Vec<String>, Vec<String>, Vec<String>)>;
struct Match {
    pct: f64,
    found: i64,
    total: i64,
    per_marker: PerMarker,
}

fn clean(markers: &[(String, Vec<String>)], ignore: &HashSet<String>) -> Vec<(String, Vec<String>)> {
    markers.iter().map(|(m, a)| (m.clone(), a.iter().filter(|x| !ignore.contains(*x)).cloned().collect())).collect()
}

fn percent_match(reference: &[(String, Vec<String>)], comparison: &[(String, Vec<String>)], ignore: &HashSet<String>) -> Match {
    let ref_ = clean(reference, ignore);
    let comp = clean(comparison, ignore);
    let comp_map: HashMap<&String, &Vec<String>> = comp.iter().map(|(m, a)| (m, a)).collect();
    let (mut total, mut found) = (0i64, 0i64);
    let mut per = PerMarker::new();
    for (marker, alleles) in &ref_ {
        let empty = Vec::new();
        let other_v = comp_map.get(marker).copied().unwrap_or(&empty);
        let other: HashSet<&String> = other_v.iter().collect();
        let hits: Vec<String> = alleles.iter().filter(|a| other.contains(a)).cloned().collect();
        total += alleles.len() as i64;
        found += hits.len() as i64;
        per.push((marker.clone(), alleles.clone(), other_v.clone(), hits));
    }
    let pct = if total != 0 { 100.0 * found as f64 / total as f64 } else { 0.0 };
    Match { pct, found, total, per_marker: per }
}

fn key(p: &Profile) -> Vec<(String, Vec<String>)> {
    let mut k: Vec<(String, Vec<String>)> = p
        .markers
        .iter()
        .filter(|(_, a)| !a.is_empty())
        .map(|(m, a)| {
            let mut s = a.clone();
            s.sort();
            (m.clone(), s)
        })
        .collect();
    k.sort();
    k
}

struct Group {
    id: String,
    kind: &'static str,
    members: Vec<String>,
    markers: Vec<(String, Vec<String>)>,
}

fn concordance_groups(samples: &[Profile]) -> Vec<Group> {
    let mut order: Vec<Vec<usize>> = Vec::new();
    let mut index: HashMap<Vec<(String, Vec<String>)>, usize> = HashMap::new();
    for (i, s) in samples.iter().enumerate() {
        let k = key(s);
        if !k.is_empty() {
            let slot = *index.entry(k).or_insert_with(|| {
                order.push(Vec::new());
                order.len() - 1
            });
            order[slot].push(i);
        }
    }
    let mut groups: Vec<Group> = Vec::new();
    for members in order {
        if members.len() < 2 {
            continue;
        }
        let first = &samples[members[0]];
        let mixed = first.markers.iter().any(|(_, a)| a.len() > 2);
        groups.push(Group {
            id: format!("{}-source group {}", if mixed { "Mixed" } else { "Single" }, groups.len() + 1),
            kind: if mixed { "mixed" } else { "single" },
            members: members.iter().map(|&i| samples[i].id.clone()).collect(),
            markers: first.markers.clone(),
        });
    }
    groups
}

fn group_json(g: &Group) -> Value {
    json!({"id": g.id, "kind": g.kind, "members": g.members, "markers": markers_json(&g.markers)})
}

#[derive(Clone)]
struct Unit {
    id: String,
    markers: Vec<(String, Vec<String>)>,
    members: Vec<String>,
}

fn units(samples: &[Profile], groups: &[Group]) -> Vec<Unit> {
    let grouped: HashSet<&String> = groups.iter().flat_map(|g| g.members.iter()).collect();
    let mut u: Vec<Unit> = groups.iter().map(|g| Unit { id: g.id.clone(), markers: g.markers.clone(), members: g.members.clone() }).collect();
    u.extend(samples.iter().filter(|s| !grouped.contains(&s.id)).map(|s| Unit { id: s.id.clone(), markers: s.markers.clone(), members: vec![s.id.clone()] }));
    u
}

fn best<'a>(a: &'a Unit, b: &'a Unit, ignore: &HashSet<String>) -> (&'a Unit, &'a Unit, Match) {
    let ab = percent_match(&a.markers, &b.markers, ignore);
    let ba = percent_match(&b.markers, &a.markers, ignore);
    if ba.pct > ab.pct {
        (b, a, ba)
    } else {
        (a, b, ab)
    }
}

struct Row {
    reference: String,
    reference_members: Vec<String>,
    comparison: String,
    comparison_members: Vec<String>,
    percent: f64,
    found: i64,
    total: i64,
    markers: PerMarker,
}

fn row(r: &Unit, c: &Unit, m: Match) -> Row {
    Row {
        reference: r.id.clone(),
        reference_members: r.members.clone(),
        comparison: c.id.clone(),
        comparison_members: c.members.clone(),
        percent: round1(m.pct),
        found: m.found,
        total: m.total,
        markers: m.per_marker,
    }
}

fn row_json(r: &Row) -> Value {
    let mut markers = Map::new();
    for (m, reference, comparison, matched) in &r.markers {
        markers.insert(m.clone(), json!({"reference": reference, "comparison": comparison, "matched": matched}));
    }
    json!({"reference": r.reference, "reference_members": r.reference_members, "comparison": r.comparison,
           "comparison_members": r.comparison_members, "percent": fv(r.percent), "found": r.found, "total": r.total,
           "markers": Value::Object(markers)})
}

fn threshold(v: Option<&Value>) -> Res<(Value, f64)> {
    let (val, t) = match v {
        None | Some(Value::Null) => (json!(80), 80.0),
        Some(Value::Bool(b)) => {
            let f = if *b { 1.0 } else { 0.0 };
            (fv(f), f)
        }
        Some(Value::Number(n)) => {
            let f = n.as_f64().unwrap();
            (fv(f), f)
        }
        Some(Value::String(s)) => {
            let f = py_float_parse(s).ok_or_else(|| format!("could not convert string to float: {}", str_repr(s)))?;
            (fv(f), f)
        }
        Some(_) => return Err("TypeError: float() argument must be a string or a real number".into()),
    };
    if !(50.0 <= t && t <= 100.0) {
        return Err("Percent Match Threshold must be 50..100".into());
    }
    Ok((val, t))
}

fn iter_or_empty<'a>(v: Option<&'a Value>, wh: &str) -> Res<Vec<&'a Value>> {
    match v {
        None => Ok(vec![]),
        Some(x) if !truthy(x) => Ok(vec![]),
        Some(Value::Array(a)) => Ok(a.iter().collect()),
        Some(Value::Object(_)) | Some(Value::String(_)) => Err(format!("{}: object required", wh)),
        Some(_) => Err("TypeError: object is not iterable".into()),
    }
}

pub fn compare(request: &Value) -> Res<Value> {
    let req = request.as_object().ok_or("Profile comparison request must be an object")?;
    let mode = req.get("mode").cloned().unwrap_or_else(|| json!("concordance"));
    let samples_raw = match req.get("samples") {
        Some(s) if truthy(s) => s.clone(),
        _ => json!([]),
    };
    let samples_arr = match &samples_raw {
        Value::Array(a) if a.len() <= MAX_PROFILES => a,
        _ => return Err(format!("At most {} samples", MAX_PROFILES)),
    };
    let samples: Vec<Profile> = samples_arr.iter().map(|s| profile(s, "sample")).collect::<Res<_>>()?;
    let ignore_vals: Vec<String> = match req.get("ignore") {
        Some(x) if truthy(x) => match x {
            Value::Array(a) => a.iter().map(py_str).collect(),
            Value::String(s) => s.chars().map(|c| c.to_string()).collect(),
            Value::Object(m) => m.keys().cloned().collect(),
            _ => return Err("TypeError: object is not iterable".into()),
        },
        _ => vec![],
    };
    let ignore: HashSet<String> = ignore_vals.into_iter().take(5).collect();
    let groups = concordance_groups(&samples);
    let groups_json = Value::Array(groups.iter().map(group_json).collect());
    if mode == json!("concordance") {
        return Ok(json!({"mode": mode, "groups": groups_json}));
    }
    let (thr_val, thr) = threshold(req.get("threshold"))?;
    let mut rows: Vec<Row> = Vec::new();
    if mode == json!("samples") {
        let u = units(&samples, &groups);
        for (i, a) in u.iter().enumerate() {
            for b in &u[i + 1..] {
                let (r, c, m) = best(a, b, &ignore);
                if m.pct >= thr {
                    rows.push(row(r, c, m));
                }
            }
        }
    } else if mode == json!("lab_reference") || mode == json!("control") {
        let profiles: Vec<Profile> = iter_or_empty(req.get("profiles"), "profile")?.into_iter().map(|p| profile(p, "profile")).collect::<Res<_>>()?;
        let targets: Vec<Unit> = if mode == json!("lab_reference") {
            let chosen: Vec<Value> = match req.get("selected") {
                Some(s) if truthy(s) => match s {
                    Value::Array(a) => a.clone(),
                    _ => return Err("TypeError: unsupported selected".into()),
                },
                _ => samples.iter().map(|s| json!(s.id)).collect(),
            };
            samples
                .iter()
                .filter(|s| chosen.iter().any(|c| c == &json!(s.id)))
                .map(|s| Unit { id: s.id.clone(), markers: s.markers.clone(), members: vec![s.id.clone()] })
                .collect()
        } else {
            units(&samples, &groups)
        };
        for p in &profiles {
            let pu = Unit { id: p.id.clone(), markers: p.markers.clone(), members: vec![p.id.clone()] };
            for t in &targets {
                let (r, c, m) = best(&pu, t, &ignore);
                if m.pct >= thr {
                    rows.push(row(r, c, m));
                }
            }
        }
    } else {
        return Err("mode must be concordance, samples, lab_reference or control".into());
    }
    rows.sort_by(|a, b| {
        b.percent
            .partial_cmp(&a.percent)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.reference.cmp(&b.reference))
            .then_with(|| a.comparison.cmp(&b.comparison))
    });
    Ok(json!({"mode": mode, "threshold": thr_val, "groups": groups_json, "rows": rows.iter().map(row_json).collect::<Vec<_>>()}))
}

fn csv_cell(v: &str, out: &mut String) {
    if v.chars().any(|c| matches!(c, ',' | '"' | '\r' | '\n')) {
        out.push('"');
        out.push_str(&v.replace('"', "\"\""));
        out.push('"');
    } else {
        out.push_str(v);
    }
}

pub fn export_csv(rows: &Value) -> Res<String> {
    let mut out = String::new();
    let line = |cells: &[String], out: &mut String| {
        for (i, c) in cells.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            csv_cell(c, out);
        }
        out.push_str("\r\n");
    };
    line(&["Reference", "Comparison", "% Match", "Comment", "Marker", "Reference Alleles", "Comparison Alleles"].map(String::from), &mut out);
    let list = match rows {
        Value::Array(a) => a.iter().collect::<Vec<_>>(),
        x if !truthy(x) => vec![],
        _ => return Err("TypeError: rows".into()),
    };
    let cell = |v: &Value| if v.is_null() { String::new() } else { py_str(v) };
    for r in list {
        let r = r.as_object().ok_or("TypeError: row")?;
        let markers = r.get("markers").and_then(|m| m.as_object()).ok_or("KeyError: 'markers'")?;
        for (marker, m) in markers {
            let strs = |k: &str| -> Res<&Vec<Value>> { m.get(k).and_then(|x| x.as_array()).ok_or_else(|| format!("KeyError: '{}'", k)) };
            let matched = strs("matched")?;
            let mut parts = Vec::new();
            for a in strs("reference")? {
                if matched.iter().any(|x| py_eq(a, x)) {
                    parts.push(a.as_str().ok_or("TypeError: join")?.to_string());
                } else {
                    parts.push(format!("<{}>", py_str(a)));
                }
            }
            let comparison: Vec<&str> = strs("comparison")?.iter().map(|x| x.as_str().ok_or("TypeError: join")).collect::<Result<_, _>>()?;
            line(
                &[
                    cell(r.get("reference").ok_or("KeyError: 'reference'")?),
                    cell(r.get("comparison").ok_or("KeyError: 'comparison'")?),
                    cell(r.get("percent").ok_or("KeyError: 'percent'")?),
                    r.get("comment").map_or(String::new(), |c| cell(c)),
                    marker.clone(),
                    parts.join(", "),
                    comparison.join(", "),
                ],
                &mut out,
            );
        }
    }
    Ok(out)
}

/// `csv.reader(io.StringIO(text), delimiter=d)` with the default excel dialect.
fn csv_rows(text: &str, delim: char) -> Res<Vec<Vec<String>>> {
    #[derive(PartialEq, Clone, Copy)]
    enum St {
        StartRecord,
        StartField,
        InField,
        InQuoted,
        QuoteInQuoted,
        EatCrnl,
    }
    let mut lines: Vec<&str> = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        match rest.find('\n') {
            Some(i) => {
                lines.push(&rest[..=i]);
                rest = &rest[i + 1..];
            }
            None => {
                lines.push(rest);
                rest = "";
            }
        }
    }
    let mut records: Vec<Vec<String>> = Vec::new();
    let mut state = St::StartRecord;
    let mut fields: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut flen = 0usize;
    fn add(field: &mut String, flen: &mut usize, c: char) -> Res<()> {
        if *flen >= FIELD_LIMIT {
            return Err(format!("field larger than field limit ({})", FIELD_LIMIT));
        }
        field.push(c);
        *flen += 1;
        Ok(())
    }
    for line in &lines {
        // `None` stands for the end-of-line marker the C parser feeds after each line.
        let chars: Vec<Option<char>> = line.chars().map(Some).chain(std::iter::once(None)).collect();
        for c in chars {
            let nl = matches!(c, Some('\n') | Some('\r'));
            let eol = c.is_none();
            match state {
                St::StartRecord => {
                    if eol {
                        continue;
                    }
                    if nl {
                        state = St::EatCrnl;
                        continue;
                    }
                    // fall through to START_FIELD
                    let ch = c.unwrap();
                    if ch == '"' {
                        state = St::InQuoted;
                    } else if ch == delim {
                        fields.push(std::mem::take(&mut field));
                        flen = 0;
                        state = St::StartField;
                    } else {
                        add(&mut field, &mut flen, ch)?;
                        state = St::InField;
                    }
                }
                St::StartField => {
                    if nl || eol {
                        fields.push(std::mem::take(&mut field));
                        flen = 0;
                        state = if eol { St::StartRecord } else { St::EatCrnl };
                    } else {
                        let ch = c.unwrap();
                        if ch == '"' {
                            state = St::InQuoted;
                        } else if ch == delim {
                            fields.push(std::mem::take(&mut field));
                            flen = 0;
                        } else {
                            add(&mut field, &mut flen, ch)?;
                            state = St::InField;
                        }
                    }
                }
                St::InField => {
                    if nl || eol {
                        fields.push(std::mem::take(&mut field));
                        flen = 0;
                        state = if eol { St::StartRecord } else { St::EatCrnl };
                    } else {
                        let ch = c.unwrap();
                        if ch == delim {
                            fields.push(std::mem::take(&mut field));
                            flen = 0;
                            state = St::StartField;
                        } else {
                            add(&mut field, &mut flen, ch)?;
                        }
                    }
                }
                St::InQuoted => {
                    if let Some(ch) = c {
                        if ch == '"' {
                            state = St::QuoteInQuoted;
                        } else {
                            add(&mut field, &mut flen, ch)?;
                        }
                    }
                }
                St::QuoteInQuoted => {
                    if c == Some('"') {
                        add(&mut field, &mut flen, '"')?;
                        state = St::InQuoted;
                    } else if c == Some(delim) {
                        fields.push(std::mem::take(&mut field));
                        flen = 0;
                        state = St::StartField;
                    } else if nl || eol {
                        fields.push(std::mem::take(&mut field));
                        flen = 0;
                        state = if eol { St::StartRecord } else { St::EatCrnl };
                    } else {
                        add(&mut field, &mut flen, c.unwrap())?;
                        state = St::InField;
                    }
                }
                St::EatCrnl => {
                    if nl {
                    } else if eol {
                        state = St::StartRecord;
                    } else {
                        return Err("new-line character seen in unquoted field - do you need to open the file in universal-newline mode?".into());
                    }
                }
            }
        }
        if state == St::StartRecord {
            records.push(std::mem::take(&mut fields));
        }
    }
    if flen != 0 || state == St::InQuoted {
        fields.push(std::mem::take(&mut field));
        records.push(std::mem::take(&mut fields));
    }
    Ok(records)
}

pub fn parse_profiles(text: &Value) -> Res<Value> {
    let text = match text {
        Value::String(t) if t.chars().count() <= 16 * 1024 * 1024 => t,
        _ => return Err("Profile file text required (at most 16 MiB)".into()),
    };
    let first = text.split('\n').next().unwrap_or("");
    let delim = if first.matches('\t').count() >= first.matches(',').count() { '\t' } else { ',' };
    let mut reader = csv_rows(text, delim)?.into_iter();
    let header: Vec<String> = reader.next().unwrap_or_default().iter().map(|h| strip(h).to_string()).collect();
    for col in ["Profile ID", "Marker"] {
        if !header.iter().any(|h| h == col) {
            return Err(format!("Profile import file needs a {} column", str_repr(col)));
        }
    }
    let mut idx: HashMap<&str, usize> = HashMap::new();
    for (i, h) in header.iter().enumerate() {
        idx.insert(h.as_str(), i);
    }
    let is_allele = |h: &str| {
        h.strip_prefix("Allele ").map_or(false, |d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()))
    };
    let allele_cols: Vec<usize> = header.iter().filter(|h| is_allele(h)).map(|h| idx[h.as_str()]).take(MAX_ALLELES).collect();
    let type_col = idx.get("UD7").or_else(|| idx.get("Profile Type")).copied();
    let mut order: Vec<String> = Vec::new();
    struct P {
        kind: &'static str,
        markers: Vec<(String, Vec<String>)>,
        dye: Vec<(String, String)>,
        size: Vec<(String, f64)>,
    }
    let mut profiles: HashMap<String, P> = HashMap::new();
    for rowv in reader {
        if rowv.is_empty() || !rowv.iter().any(|c| !strip(c).is_empty()) {
            continue;
        }
        let get = |i: Option<usize>| -> String { i.filter(|&i| i < rowv.len()).map_or(String::new(), |i| strip(&rowv[i]).to_string()) };
        let pid = get(Some(idx["Profile ID"]));
        let marker = get(Some(idx["Marker"]));
        if pid.is_empty() || marker.is_empty() {
            continue;
        }
        let kind = if type_col.is_some() { get(type_col) } else { String::new() };
        let kind: &'static str = if kind.to_lowercase().starts_with("custom") { "Custom Control" } else { "Lab Reference" };
        if !profiles.contains_key(&pid) {
            order.push(pid.clone());
            profiles.insert(pid.clone(), P { kind, markers: vec![], dye: vec![], size: vec![] });
        }
        let p = profiles.get_mut(&pid).unwrap();
        let alleles: Vec<String> = allele_cols.iter().map(|&i| get(Some(i))).filter(|a| !a.is_empty()).collect();
        let pos = match p.markers.iter().position(|(m, _)| m == &marker) {
            Some(i) => i,
            None => {
                p.markers.push((marker.clone(), vec![]));
                p.markers.len() - 1
            }
        };
        let existing = p.markers[pos].1.clone();
        let add: Vec<String> = alleles.into_iter().filter(|a| !existing.contains(a)).collect();
        p.markers[pos].1.extend(add);
        if let Some(&di) = idx.get("Dye") {
            let d = get(Some(di));
            if !d.is_empty() {
                match p.dye.iter_mut().find(|(m, _)| m == &marker) {
                    Some(e) => e.1 = d,
                    None => p.dye.push((marker.clone(), d)),
                }
            }
        }
        if let Some(&si) = idx.get("Size 1") {
            let s = get(Some(si));
            if !s.is_empty() {
                if let Some(f) = py_float_parse(&s) {
                    match p.size.iter_mut().find(|(m, _)| m == &marker) {
                        Some(e) => e.1 = f,
                        None => p.size.push((marker.clone(), f)),
                    }
                }
            }
        }
        if profiles.len() > MAX_PROFILES {
            return Err(format!("At most {} profiles", MAX_PROFILES));
        }
    }
    let out: Vec<Value> = order
        .iter()
        .map(|pid| {
            let p = &profiles[pid];
            let mut dye = Map::new();
            for (m, d) in &p.dye {
                dye.insert(m.clone(), json!(d));
            }
            let mut size = Map::new();
            for (m, s) in &p.size {
                size.insert(m.clone(), fv(*s));
            }
            json!({"id": pid, "type": p.kind, "markers": markers_json(&p.markers), "dye": Value::Object(dye), "size": Value::Object(size)})
        })
        .collect();
    Ok(Value::Array(out))
}

pub fn export_profiles(profiles: &Value) -> Res<String> {
    let list: Vec<&Value> = iter_or_empty(Some(profiles), "profile")?;
    let rows: Vec<Profile> = list.iter().map(|p| profile(p, "profile")).collect::<Res<_>>()?;
    let width = rows.iter().flat_map(|p| p.markers.iter().map(|(_, a)| a.len())).max().unwrap_or(1).max(1);
    let width = width.max(2).min(MAX_ALLELES);
    let mut head = vec!["Profile ID".to_string(), "UD7".into(), "Marker".into()];
    head.extend((0..width).map(|i| format!("Allele {}", i + 1)));
    let mut lines = vec![head.join("\t")];
    for (raw, p) in list.iter().zip(rows.iter()) {
        let kind = match raw.get("type") {
            None => "Lab Reference".to_string(),
            Some(Value::String(s)) => s.clone(),
            Some(_) => return Err("TypeError: sequence item: expected str instance".into()),
        };
        for (marker, alleles) in &p.markers {
            let mut cells: Vec<String> = alleles.iter().take(width).cloned().collect();
            let pad = width - cells.len();
            cells.extend(std::iter::repeat(String::new()).take(pad));
            let mut line = vec![p.id.clone(), kind.clone(), marker.clone()];
            line.extend(cells);
            lines.push(line.join("\t"));
        }
    }
    Ok(format!("{}\r\n", lines.join("\r\n")))
}

pub fn profile_request(request: &Value) -> Res<Value> {
    let action = request.as_object().and_then(|m| m.get("action")).and_then(|a| a.as_str());
    let get = |k: &str| request.get(k).filter(|v| truthy(v)).cloned().unwrap_or_else(|| json!([]));
    match action {
        Some("compare") => compare(request),
        Some("export_csv") => Ok(json!({"text": export_csv(&get("rows"))?})),
        Some("parse") => Ok(json!({"profiles": parse_profiles(request.get("text").unwrap_or(&Value::Null))?})),
        Some("export") => Ok(json!({"text": export_profiles(&get("profiles"))?})),
        _ => Err("action must be compare, export_csv, parse or export".into()),
    }
}
