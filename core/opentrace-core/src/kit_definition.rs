//! Port of `genemapper_open/kit_definition.py` for parsed mapping input (`_load_kit_definition`).
//! Not ported: the JSON-text/bytes path (duplicate-key and nonfinite-constant checks on text) and the memo cache,
//! which returns equal copies and therefore does not change results.
use serde_json::{Map, Value};

pub type Res<T> = Result<T, String>;

const MAX_JSON_BYTES: usize = 1_048_576;
const MAX_MARKERS: usize = 128;
const MAX_BINS_PER_MARKER: usize = 128;
const MAX_TOTAL_BINS: usize = 4096;
const MAX_LADDER_LABELS: usize = 64;
const MARKER_OPTIONAL: [&str; 4] = ["dye", "control_alleles", "y_marker", "internal_qc"];
const DYE_COLORS: [&str; 6] = ["blue", "green", "yellow", "red", "purple", "orange"];
const MAX_RANGES: usize = 64;
const MAX_TEXT_LENGTH: usize = 128;
const MAX_BP: f64 = 1_000_000.0;

fn fv(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

/// Python `float.__repr__` (shortest round-trip digits, Python's fixed/scientific switch at 1e-4 and 1e16).
pub fn py_float_repr(x: f64) -> String {
    if x == 0.0 {
        return if x.is_sign_negative() { "-0.0".into() } else { "0.0".into() };
    }
    let sign = if x < 0.0 { "-" } else { "" };
    let sci = format!("{:e}", x.abs());
    let (mant, exp) = sci.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let body = if (-4..16).contains(&exp) {
        if exp >= 0 {
            let int_len = (exp + 1) as usize;
            if digits.len() <= int_len {
                format!("{}{}.0", digits, "0".repeat(int_len - digits.len()))
            } else {
                format!("{}.{}", &digits[..int_len], &digits[int_len..])
            }
        } else {
            format!("0.{}{}", "0".repeat((-exp - 1) as usize), digits)
        }
    } else {
        let frac = if digits.len() > 1 { format!(".{}", &digits[1..]) } else { String::new() };
        let e_sign = if exp < 0 { "-" } else { "+" };
        format!("{}{}e{}{:02}", &digits[..1], frac, e_sign, exp.abs())
    };
    format!("{}{}", sign, body)
}

fn py_json_string(s: &str, ensure_ascii: bool, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if ensure_ascii && (c as u32) > 0x7e => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{:04x}", unit));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Python `json.dumps(value, ensure_ascii=False, allow_nan=False)` text for JSON-compatible data.
pub fn py_json_dumps(v: &Value) -> String {
    let mut out = String::new();
    write_py_json(v, false, &mut out);
    out
}

/// Python `json.dumps(value, allow_nan=False)` with the default `ensure_ascii=True`.
pub fn py_json_dumps_ascii(v: &Value) -> String {
    let mut out = String::new();
    write_py_json(v, true, &mut out);
    out
}

fn write_py_json(v: &Value, ensure_ascii: bool, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) if n.is_f64() => out.push_str(&py_float_repr(n.as_f64().unwrap())),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => py_json_string(s, ensure_ascii, out),
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_py_json(x, ensure_ascii, out);
            }
            out.push(']');
        }
        Value::Object(m) => {
            out.push('{');
            for (i, (k, x)) in m.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                py_json_string(k, ensure_ascii, out);
                out.push_str(": ");
                write_py_json(x, ensure_ascii, out);
            }
            out.push('}');
        }
    }
}

/// Python `_object`: required and optional field sets; messages match the source.
fn object<'a>(value: &'a Value, required: &[&str], optional: &[&str], path: &str) -> Res<&'a Map<String, Value>> {
    let map = value.as_object().ok_or_else(|| format!("{}: object required", path))?;
    let mut missing: Vec<&str> = required.iter().copied().filter(|k| !map.contains_key(*k)).collect();
    if !missing.is_empty() {
        missing.sort();
        return Err(format!("{}: missing fields {}", path, missing.join(", ")));
    }
    let mut unknown: Vec<&str> = map
        .keys()
        .map(|k| k.as_str())
        .filter(|k| !required.contains(k) && !optional.contains(k))
        .collect();
    if !unknown.is_empty() {
        unknown.sort();
        return Err(format!("{}: unknown fields {}", path, unknown.join(", ")));
    }
    Ok(map)
}

/// Python `_text`: 1..128 code points, no edge whitespace, no control characters.
fn text(value: &Value, path: &str) -> Res<String> {
    let msg = format!(
        "{}: nonempty text of at most 128 characters without edge whitespace or controls required",
        path
    );
    let s = value.as_str().ok_or_else(|| msg.clone())?;
    let len = s.chars().count();
    if !(1..=MAX_TEXT_LENGTH).contains(&len)
        || s != s.trim()
        || s.chars().any(|c| (c as u32) < 32 || c as u32 == 127)
    {
        return Err(msg);
    }
    Ok(s.to_string())
}

/// Python `_number`: finite real (not bool) within [low, high]; `positive` excludes `low`. Returns float.
fn number(value: Option<&Value>, path: &str, low: f64, high: f64, positive: bool) -> Res<f64> {
    let err = || {
        format!(
            "{}: finite number in {}{}, {}] required",
            path,
            if positive { "(" } else { "[" },
            py_num_str(low),
            py_num_str(high)
        )
    };
    let x = match value {
        Some(Value::Number(n)) => n.as_f64().ok_or_else(err)?,
        _ => return Err(err()),
    };
    if !(low <= x && x <= high) || (positive && x == low) {
        return Err(err());
    }
    Ok(x)
}

fn py_num_str(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{}", x as i64)
    } else {
        py_float_repr(x)
    }
}

fn ranges(value: Option<&Value>, path: &str, ratio: bool) -> Res<Vec<Value>> {
    let items = match value {
        Some(Value::Array(a)) if a.len() <= MAX_RANGES => a,
        _ => return Err(format!("{}: explicit list of at most {} ranges required", path, MAX_RANGES)),
    };
    let mut result = Vec::with_capacity(items.len());
    for (index, row) in items.iter().enumerate() {
        let wher = format!("{}[{}]", path, index);
        let keys: &[&str] = if ratio { &["min_bp", "max_bp", "ratio"] } else { &["min_bp", "max_bp"] };
        let map = object(row, keys, &[], &wher)?;
        let low = number(map.get("min_bp"), &format!("{}.min_bp", wher), 0.0, MAX_BP, false)?;
        let high = number(map.get("max_bp"), &format!("{}.max_bp", wher), 0.0, MAX_BP, false)?;
        if low > high {
            return Err(format!("{}: min_bp must not exceed max_bp", wher));
        }
        let mut item = Map::new();
        item.insert("min_bp".into(), fv(low));
        item.insert("max_bp".into(), fv(high));
        if ratio {
            let r = number(map.get("ratio"), &format!("{}.ratio", wher), 0.0, 1.0, false)?;
            item.insert("ratio".into(), fv(r));
        }
        result.push(Value::Object(item));
    }
    Ok(result)
}

fn parameters(value: &Value, path: &str) -> Res<Value> {
    let keys = [
        "ladder_cutoff_ratio",
        "cutoff_ratio",
        "minus_a_ratio",
        "minus_a_min_bp",
        "minus_a_max_bp",
        "backward_stutter_ranges",
        "forward_stutter_ranges",
    ];
    let map = object(value, &keys, &["clear_ranges"], path)?;
    let mut result = Map::new();
    for name in ["ladder_cutoff_ratio", "cutoff_ratio", "minus_a_ratio"] {
        let x = number(map.get(name), &format!("{}.{}", path, name), 0.0, 1.0, false)?;
        result.insert(name.into(), fv(x));
    }
    for name in ["minus_a_min_bp", "minus_a_max_bp"] {
        let x = number(map.get(name), &format!("{}.{}", path, name), 0.0, MAX_BP, false)?;
        result.insert(name.into(), fv(x));
    }
    if result["minus_a_min_bp"].as_f64().unwrap() > result["minus_a_max_bp"].as_f64().unwrap() {
        return Err(format!("{}: minus_a_min_bp must not exceed minus_a_max_bp", path));
    }
    for name in ["backward_stutter_ranges", "forward_stutter_ranges"] {
        let r = ranges(map.get(name), &format!("{}.{}", path, name), true)?;
        result.insert(name.into(), Value::Array(r));
    }
    let clear = match map.get("clear_ranges") {
        Some(v) => ranges(Some(v), &format!("{}.clear_ranges", path), false)?,
        None => ranges(Some(&Value::Array(vec![])), &format!("{}.clear_ranges", path), false)?,
    };
    result.insert("clear_ranges".into(), Value::Array(clear));
    Ok(Value::Object(result))
}

/// Python `casefold` approximated by lowercase plus the common full fold of sharp s. Exotic full foldings
/// (for example Greek final sigma variants) are not reproduced; this affects only duplicate-name detection.
fn casefold(s: &str) -> String {
    s.to_lowercase().replace('ß', "ss")
}

/// Python `_load_kit_definition` for a parsed mapping. Returns the normalized kit object.
pub fn load_kit_definition(source: &Value) -> Res<Value> {
    if py_json_dumps(source).len() > MAX_JSON_BYTES {
        return Err("Kit JSON exceeds 1048576 bytes".into());
    }
    let map = object(source, &["schema_version", "name", "markers"], &["diagnostics"], "kit")?;
    match map.get("schema_version") {
        Some(Value::Number(n)) if n.is_i64() && n.as_i64() == Some(1) => {}
        _ => return Err("kit.schema_version must be integer 1".into()),
    }
    let name = text(&map["name"], "kit.name")?;
    let rows = match map.get("markers") {
        Some(Value::Array(a)) if (1..=MAX_MARKERS).contains(&a.len()) => a,
        _ => return Err("kit.markers: list of 1..128 markers required".into()),
    };
    let mut markers: Vec<Value> = Vec::new();
    let mut diagnostics: Vec<Value> = Vec::new();
    let mut marker_names: Vec<String> = Vec::new();
    let mut total_bins = 0usize;
    for (index, row) in rows.iter().enumerate() {
        let wher = format!("kit.markers[{}]", index);
        let row_map = object(
            row,
            &["name", "channel", "min_bp", "max_bp", "repeat", "bins", "ladder_labels", "parameters"],
            &MARKER_OPTIONAL,
            &wher,
        )?;
        let marker_name = text(&row_map["name"], &format!("{}.name", wher))?;
        let folded = casefold(&marker_name);
        if marker_names.contains(&folded) {
            return Err(format!("{}: marker names must be unique ignoring case", wher));
        }
        marker_names.push(folded);
        let channel = match row_map.get("channel") {
            Some(Value::String(c)) if canonical_channel(c) => c.clone(),
            _ => return Err(format!("{}.channel: canonical DATA number required (e.g. DATA1)", wher)),
        };
        let low = number(row_map.get("min_bp"), &format!("{}.min_bp", wher), 0.0, MAX_BP, true)?;
        let high = number(row_map.get("max_bp"), &format!("{}.max_bp", wher), 0.0, MAX_BP, true)?;
        if low >= high {
            return Err(format!("{}: min_bp must be smaller than max_bp", wher));
        }
        let repeat = match row_map.get("repeat") {
            Some(Value::Number(n)) if n.is_i64() => n.as_i64().unwrap(),
            _ => return Err(format!("{}.repeat: integer in [1, 20] required", wher)),
        };
        if !(1..=20).contains(&repeat) {
            return Err(format!("{}.repeat: integer in [1, 20] required", wher));
        }
        let bins = match row_map.get("bins") {
            Some(Value::Array(b)) if (1..=MAX_BINS_PER_MARKER).contains(&b.len()) => b,
            _ => return Err(format!("{}.bins: list of 1..128 explicit bins required", wher)),
        };
        total_bins += bins.len();
        if total_bins > MAX_TOTAL_BINS {
            return Err("Kit exceeds 4096 total bins".into());
        }
        let mut normalized_bins: Vec<Map<String, Value>> = Vec::new();
        let mut labels: Vec<(String, String, f64)> = Vec::new(); // (folded label, label, center)
        for (bin_index, item) in bins.iter().enumerate() {
            let at = format!("{}.bins[{}]", wher, bin_index);
            let item_map = object(item, &["label", "center_bp", "left_bp", "right_bp"], &["confidence_weight", "virtual"], &at)?;
            let label = text(&item_map["label"], &format!("{}.label", at))?;
            let key = casefold(&label);
            if labels.iter().any(|(k, _, _)| *k == key) {
                return Err(format!("{}: bin labels must be unique ignoring case", at));
            }
            let center = number(item_map.get("center_bp"), &format!("{}.center_bp", at), 0.0, MAX_BP, true)?;
            if !(low <= center && center <= high) {
                return Err(format!("{}: bin center must be inside marker limits", at));
            }
            let left = number(item_map.get("left_bp"), &format!("{}.left_bp", at), 0.0, MAX_BP, false)?;
            let right = number(item_map.get("right_bp"), &format!("{}.right_bp", at), 0.0, MAX_BP, false)?;
            if center - left < 0.0 || center + right > MAX_BP {
                return Err(format!("{}: bin edges must be in [0, 1000000] bp", at));
            }
            let weight = match item_map.get("confidence_weight") {
                None => 1.0,
                Some(v) => number(Some(v), &format!("{}.confidence_weight", at), 0.0, 1.0, false)?,
            };
            let mut normalized = Map::new();
            normalized.insert("label".into(), Value::String(label.clone()));
            normalized.insert("center_bp".into(), fv(center));
            normalized.insert("left_bp".into(), fv(left));
            normalized.insert("right_bp".into(), fv(right));
            normalized.insert("confidence_weight".into(), fv(weight));
            if let Some(v) = item_map.get("virtual") {
                match v {
                    Value::Bool(b) => {
                        normalized.insert("virtual".into(), Value::Bool(*b));
                    }
                    _ => return Err(format!("{}.virtual: boolean required", at)),
                }
            }
            labels.push((key, label, center));
            normalized_bins.push(normalized);
        }
        let ladder = match row_map.get("ladder_labels") {
            Some(Value::Array(l)) if l.len() <= MAX_LADDER_LABELS => l,
            _ => {
                return Err(format!(
                    "{}.ladder_labels: explicit list of at most {} labels required",
                    wher, MAX_LADDER_LABELS
                ))
            }
        };
        let mut canonical_ladder: Vec<Value> = Vec::new();
        let mut used: Vec<String> = Vec::new();
        let mut centers: Vec<f64> = Vec::new();
        for label_v in ladder {
            text(label_v, &format!("{}.ladder_labels", wher))?;
            let key = casefold(label_v.as_str().unwrap());
            let entry = labels.iter().find(|(k, _, _)| *k == key);
            match entry {
                Some((_, canonical, center)) if !used.contains(&key) => {
                    used.push(key.clone());
                    canonical_ladder.push(Value::String(canonical.clone()));
                    centers.push(*center);
                }
                _ => {
                    return Err(format!(
                        "{}: ladder labels must identify distinct defined bins ignoring case",
                        wher
                    ))
                }
            }
        }
        if centers.windows(2).any(|w| w[1] - w[0] < 1e-6) {
            return Err(format!(
                "{}: ladder label centers must increase with gaps >= 0.000001 bp",
                wher
            ));
        }
        let mut overlap_count = 0u64;
        let mut examples: Vec<Value> = Vec::new();
        for a_index in 0..normalized_bins.len() {
            for b_index in (a_index + 1)..normalized_bins.len() {
                let a = &normalized_bins[a_index];
                let b = &normalized_bins[b_index];
                let a_c = a["center_bp"].as_f64().unwrap();
                let a_l = a["left_bp"].as_f64().unwrap();
                let a_r = a["right_bp"].as_f64().unwrap();
                let b_c = b["center_bp"].as_f64().unwrap();
                let b_l = b["left_bp"].as_f64().unwrap();
                let b_r = b["right_bp"].as_f64().unwrap();
                let lo = (a_c - a_l).max(b_c - b_l);
                let hi = (a_c + a_r).min(b_c + b_r);
                if lo <= hi {
                    overlap_count += 1;
                    if examples.len() < 16 {
                        examples.push(serde_json::json!([a["label"], b["label"]]));
                    }
                }
            }
        }
        if overlap_count > 0 {
            diagnostics.push(serde_json::json!({
                "code": "overlapping_bins",
                "marker": marker_name,
                "pair_count": overlap_count,
                "examples": examples,
                "policy": "allowed; multiple inclusive bin hits are ambiguous",
            }));
        }
        let params = parameters(&row_map["parameters"], &format!("{}.parameters", wher))?;
        let mut marker = Map::new();
        marker.insert("name".into(), Value::String(marker_name.clone()));
        marker.insert("channel".into(), Value::String(channel));
        marker.insert("min_bp".into(), fv(low));
        marker.insert("max_bp".into(), fv(high));
        marker.insert("repeat".into(), serde_json::json!(repeat));
        marker.insert("bins".into(), Value::Array(normalized_bins.into_iter().map(Value::Object).collect()));
        marker.insert("ladder_labels".into(), Value::Array(canonical_ladder));
        marker.insert("parameters".into(), params);
        if let Some(dye) = row_map.get("dye") {
            match dye.as_str() {
                Some(d) if DYE_COLORS.contains(&d) => {
                    marker.insert("dye".into(), dye.clone());
                }
                _ => {
                    return Err(format!(
                        "{}.dye: one of {} required",
                        wher,
                        DYE_COLORS.join(", ")
                    ))
                }
            }
        }
        if let Some(controls) = row_map.get("control_alleles") {
            let list = match controls {
                Value::Array(c) if c.len() <= 8 => c,
                _ => return Err(format!("{}.control_alleles: list of at most 8 labels required", wher)),
            };
            let mut out = Vec::new();
            for label in list {
                out.push(Value::String(text(label, &format!("{}.control_alleles", wher))?));
            }
            marker.insert("control_alleles".into(), Value::Array(out));
        }
        for flag in ["y_marker", "internal_qc"] {
            if let Some(v) = row_map.get(flag) {
                match v {
                    Value::Bool(b) => {
                        marker.insert(flag.into(), Value::Bool(*b));
                    }
                    _ => return Err(format!("{}.{}: boolean required", wher, flag)),
                }
            }
        }
        markers.push(Value::Object(marker));
    }
    let mut out = Map::new();
    out.insert("schema_version".into(), serde_json::json!(1));
    out.insert("name".into(), Value::String(name));
    out.insert("markers".into(), Value::Array(markers));
    out.insert("diagnostics".into(), Value::Array(diagnostics));
    Ok(Value::Object(out))
}

fn canonical_channel(s: &str) -> bool {
    let rest = match s.strip_prefix("DATA") {
        Some(r) => r.as_bytes(),
        None => return false,
    };
    (1..=6).contains(&rest.len()) && (b'1'..=b'9').contains(&rest[0]) && rest[1..].iter().all(|c| c.is_ascii_digit())
}
