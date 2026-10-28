//! Port of `genemapper_open/pullup_quality.py`: bleed-through triggers and profile PQV.
//! SHA-256 is implemented locally (no external crate) and checked against `hashlib`.
use crate::pullup::{flag, load_pullup_definition};
use crate::quality_control::{number, profile_pullup_category};
use serde_json::{json, Map, Value};
use std::collections::HashSet;

pub type Res<T> = Result<T, String>;

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// SHA-256 digest of `data`, lowercase hex.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ ((!v[4]) & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    h.iter().map(|x| format!("{:08x}", x)).collect()
}

/// Python `json.dumps` string body with ensure_ascii=False.
fn write_str(s: &str, out: &mut String) {
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
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Python `json.dumps(v, sort_keys=True, separators=(',',':'), ensure_ascii=False, allow_nan=False)`.
fn write_canonical(v: &Value, out: &mut String) -> Res<()> {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if n.is_f64() {
                let x = n.as_f64().unwrap();
                if !x.is_finite() {
                    return Err("Out of range float values are not JSON compliant".into());
                }
                out.push_str(&crate::kit_definition::py_float_repr(x));
            } else {
                out.push_str(&n.to_string());
            }
        }
        Value::String(s) => write_str(s, out),
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(x, out)?;
            }
            out.push(']');
        }
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_str(k, out);
                out.push(':');
                write_canonical(&m[k.as_str()], out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

pub(crate) fn digest(value: &Value) -> Res<String> {
    let mut s = String::new();
    write_canonical(value, &mut s)?;
    Ok(sha256_hex(s.as_bytes()))
}

fn int_value(v: &Value) -> Option<i64> {
    match v {
        Value::Bool(b) => Some(*b as i64),
        Value::Number(n) => n.as_i64(),
        _ => None,
    }
}

fn float_value(v: &Value) -> f64 {
    match v {
        Value::Bool(b) => *b as i64 as f64,
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        _ => f64::NAN,
    }
}

/// Python `==` on JSON-shaped values (True == 1, 1 == 1.0, dict equality order-free).
pub(crate) fn py_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| py_eq(p, q)),
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).map_or(false, |w| py_eq(v, w)))
        }
        (Value::Bool(_) | Value::Number(_), Value::Bool(_) | Value::Number(_)) => match (int_value(a), int_value(b)) {
            (Some(x), Some(y)) => x == y,
            _ => float_value(a) == float_value(b),
        },
        _ => false,
    }
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map_or(false, |x| x != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(m) => !m.is_empty(),
    }
}

/// Python `value or {}`.
fn or_empty(v: Option<&Value>) -> Value {
    match v {
        Some(x) if truthy(x) => x.clone(),
        _ => json!({}),
    }
}

pub fn evaluate_pullup_quality(report: &Value, definition: &Value) -> Res<Value> {
    let definition = load_pullup_definition(definition)?;
    let report = match report {
        Value::Object(m) => m,
        _ => return Err("STR observations required".into()),
    };
    let mut result = Map::new();
    result.insert("schema_version".into(), json!(1));
    result.insert("status".into(), json!("unavailable"));
    result.insert("reason".into(), json!("calling_observations_unavailable"));
    result.insert("definition_sha256".into(), json!(digest(&definition)?));
    result.insert("markers".into(), json!([]));
    result.insert("profile".into(), Value::Null);
    result.insert("scope".into(), json!("linked_peak_bleedthrough_and_profile_pullup_only"));
    result.insert("gq_or_confidence_changed".into(), json!(false));
    result.insert("ladder_admission_changed".into(), json!(false));
    result.insert("summary_pass_changed".into(), json!(false));
    result.insert("full_original_pqv".into(), json!(false));
    result.insert("original_runtime_validated".into(), json!(false));

    let calling = match report.get("allele_calling") {
        Some(Value::Object(c)) => c,
        _ => return Ok(Value::Object(result)),
    };
    let markers_v = calling.get("markers").cloned().unwrap_or(Value::Null);
    let peaks_v = report.get("peaks").cloned().unwrap_or_else(|| json!([]));
    let (markers, peaks) = match (&markers_v, &peaks_v) {
        (Value::Array(m), Value::Array(p)) if m.len() <= 128 && p.len() <= 4096 => (m, p),
        _ => return Err("Bounded marker and source peak lists required".into()),
    };
    for peak in peaks.iter() {
        match peak {
            Value::Object(pm) if matches!(pm.get("id"), Some(Value::String(_))) => {}
            _ => return Err("Source peaks with stable IDs required".into()),
        }
    }
    let mut ids: HashSet<&str> = HashSet::new();
    for peak in peaks.iter() {
        ids.insert(peak["id"].as_str().unwrap());
    }
    if ids.len() != peaks.len() {
        return Err("Unique source peak IDs required".into());
    }

    let mut names: HashSet<String> = HashSet::new();
    let mut values: Vec<Value> = Vec::new();
    let mut rows: Vec<Value> = Vec::new();
    for marker in markers.iter() {
        let mm = match marker {
            Value::Object(m) => m,
            _ => return Err("Named marker result required".into()),
        };
        let name = match mm.get("name") {
            Some(Value::String(s)) => s.clone(),
            _ => return Err("Named marker result required".into()),
        };
        if !names.insert(name.clone()) {
            return Err("Unique marker results required".into());
        }
        let mut row = Map::new();
        row.insert("marker".into(), json!(name));
        row.insert("status".into(), json!("unavailable"));
        row.insert("metric".into(), Value::Null);
        row.insert("triggers".into(), json!([]));
        row.insert("linked_peak_count".into(), Value::Null);
        row.insert("reason".into(), json!("marker_processing_unavailable"));

        if mm.get("status") == Some(&json!("tentative")) {
            let linked = match mm.get("peak_review") {
                Some(Value::Array(l)) if l.len() <= 512 => l,
                _ => return Err("Complete bounded marker-window links required".into()),
            };
            let mut seen: HashSet<i64> = HashSet::new();
            let mut triggers: Vec<Value> = Vec::new();
            for entry in linked.iter() {
                let em = match entry {
                    Value::Object(e) => e,
                    _ => return Err("Linked peak entry required".into()),
                };
                let index = match em.get("peak_index") {
                    Some(Value::Number(n)) => n.as_i64(),
                    _ => None,
                };
                let index = match index {
                    Some(i) if i >= 0 && (i as usize) < peaks.len() && !seen.contains(&i) => i,
                    _ => return Err("Unique valid linked peak indices required".into()),
                };
                seen.insert(index);
                let peak = peaks[index as usize].as_object().unwrap();
                for key in ["scan", "size_bp", "height"] {
                    let a = em.get(key).unwrap_or(&Value::Null);
                    let b = peak.get(key).unwrap_or(&Value::Null);
                    if !py_eq(a, b) {
                        return Err("Linked/source peak measurements disagree".into());
                    }
                }
                let entry_flag = flag(em, "pullup")?;
                let peak_flag = flag(peak, "pullup")?;
                if entry_flag != peak_flag {
                    return Err("Linked/source pull-up flags disagree".into());
                }
                if !entry_flag {
                    continue;
                }
                let classification = or_empty(peak.get("pullup_classification"));
                let cls = classification.as_object().ok_or("AttributeError: classification has no get")?;
                let state = or_empty(cls.get("state"));
                let st = state.as_object().ok_or("AttributeError: state has no get")?;
                let ratio_src = st.get("ratio").cloned().unwrap_or_else(|| {
                    peak.get("pullup_ratio").cloned().unwrap_or_else(|| json!(-1))
                });
                let thr_src = st.get("ratio_threshold").cloned().unwrap_or_else(|| {
                    peak.get("pullup_ratio_threshold").cloned().unwrap_or_else(|| json!(-1))
                });
                let ratio = number(&ratio_src, "retained pull-up ratio", -1e12, 1e12)?;
                let threshold = number(&thr_src, "retained pull-up threshold", -1.0, 1e12)?;
                let position = number(
                    &peak.get("size_bp").cloned().unwrap_or(Value::Null),
                    "pull-up trigger position",
                    -1e6,
                    1e6,
                )?;
                triggers.push(json!({
                    "id": "PTI_BLEED_THROUGH",
                    "ordinal": 4,
                    "source_peak_id": peak.get("id").cloned().unwrap_or(Value::Null),
                    "ratio": ratio,
                    "thresholds": [threshold],
                    "positions_bp": [position],
                    "ratio_provenance": cls
                        .get("ratio_provenance")
                        .cloned()
                        .unwrap_or_else(|| json!("stored_flag_without_detector_measurement")),
                    "ratio_available": ratio >= 0.0 && threshold >= 0.0,
                }));
            }
            let metric: i64 = if triggers.is_empty() { 0 } else { 1 };
            row.insert("status".into(), json!("evaluated"));
            row.insert("metric".into(), json!(metric));
            row.insert("triggers".into(), Value::Array(triggers));
            row.insert("linked_peak_count".into(), json!(linked.len()));
            row.insert("reason".into(), Value::Null);
        }
        values.push(row["metric"].clone());
        rows.push(Value::Object(row));
    }
    result.insert("markers".into(), Value::Array(rows));

    let profile = profile_pullup_category(&Value::Array(values.clone()), &definition["profile_threshold"])?;
    let status = if values.iter().all(|v| !v.is_null()) { "evaluated_subset" } else { "partial" };
    let input_sha = digest(&json!({"peaks": peaks_v.clone(), "allele_calling": Value::Object(calling.clone())}))?;
    result.insert("profile".into(), profile);
    result.insert("status".into(), json!(status));
    result.insert("reason".into(), Value::Null);
    result.insert("input_observations_sha256".into(), json!(input_sha));
    Ok(Value::Object(result))
}

pub fn attach_pullup_quality(report: &Value, definition: &Value) -> Res<Value> {
    let quality = evaluate_pullup_quality(report, definition)?;
    let mut result = match report {
        Value::Object(m) => m.clone(),
        _ => return Err("STR observations required".into()),
    };
    let sha = quality["definition_sha256"].clone();
    result
        .entry("quality")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("TypeError: quality is not a mapping")?
        .insert("pullup".into(), quality);
    result
        .entry("settings")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("TypeError: settings is not a mapping")?
        .insert("pullup_quality_definition_sha256".into(), sha);
    Ok(Value::Object(result))
}
