//! Port of `genemapper_open/pullup_quality.py`: bleed-through triggers and profile PQV.
//! SHA-256 comes from the `sha2` crate (same digests as `hashlib`).
use crate::pullup::{flag, load_pullup_definition};
use crate::quality_control::{number, profile_pullup_category};
use serde_json::{json, Map, Value};
use std::collections::HashSet;

pub type Res<T> = Result<T, String>;


/// SHA-256 digest of `data`, lowercase hex.
pub fn sha256_hex(data: &[u8]) -> String {
    // sha2 (SHA-NI where the CPU has it); same digest as the earlier hand-written loop, several times faster on the
    // multi-megabyte canonical texts hashed for provenance.
    use sha2::Digest;
    let d = sha2::Sha256::digest(data);
    let mut out = String::with_capacity(64);
    for b in d.iter() {
        out.push(char::from_digit((b >> 4) as u32, 16).unwrap());
        out.push(char::from_digit((b & 15) as u32, 16).unwrap());
    }
    out
}

#[cfg(test)]
mod sha_tests {
    #[test]
    fn sha256_known_vectors() {
        assert_eq!(super::sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(super::sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
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
        Value::Object(m) => write_canonical_map(m, out)?,
    }
    Ok(())
}

/// Canonical text of a JSON object (sorted keys), as `write_canonical` writes it.
fn write_canonical_map(m: &Map<String, Value>, out: &mut String) -> Res<()> {
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
    Ok(())
}

/// The canonical text `digest` hashes (callers cache the text of large, unchanging parts).
pub(crate) fn canonical_string(value: &Value) -> Res<String> {
    let mut s = String::new();
    write_canonical(value, &mut s)?;
    Ok(s)
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

pub(crate) fn truthy(v: &Value) -> bool {
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
    let definition = match definition {
        Value::String(_) => crate::server_analyze::load_definition(definition, crate::server_analyze::DefKind::Pullup)?,
        _ => load_pullup_definition(definition)?,
    };
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
    let null = Value::Null;
    let no_peaks = json!([]);
    let markers_v = calling.get("markers").unwrap_or(&null);
    let peaks_v = report.get("peaks").unwrap_or(&no_peaks);
    let (markers, peaks) = match (markers_v, peaks_v) {
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
    // digest of {"allele_calling": ..., "peaks": ...} written without copying the two parts
    let input_sha = {
        let mut text = String::new();
        text.push('{');
        write_str("allele_calling", &mut text);
        text.push(':');
        write_canonical_map(calling, &mut text)?;
        text.push(',');
        write_str("peaks", &mut text);
        text.push(':');
        write_canonical(peaks_v, &mut text)?;
        text.push('}');
        sha256_hex(text.as_bytes())
    };
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
