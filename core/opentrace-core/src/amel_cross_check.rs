//! Port of `genemapper_open/amel_cross_check.py`.
use crate::quality_control::{boolean, number};
use serde_json::{json, Map, Value};
use std::collections::HashSet;

pub type Res<T> = Result<T, String>;

fn fv(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

fn ascii_bounded(v: &Value) -> bool {
    matches!(v, Value::String(s) if (1..=128).contains(&s.chars().count()) && s.is_ascii())
}

pub fn evaluate_amel_cross_check(markers: &Value, sample_type: &Value, weight: &Value) -> Res<Value> {
    let st = match sample_type.as_str() {
        Some(s @ ("Sample" | "Positive Control" | "Negative Control" | "Allelic Ladder")) => s,
        _ => return Err("Explicit canonical sample type required".into()),
    };
    let weight = number(weight, "AMELCC sample weight", 0.0, 1.0)?;
    let list = match markers {
        Value::Array(m) if m.len() <= 128 => m,
        _ => return Err("At most128 explicit marker observations required".into()),
    };
    let mut prepared: Vec<&Map<String, Value>> = Vec::new();
    for marker in list {
        let m = match marker {
            Value::Object(m) if m.len() == 4 && ["name", "is_y_marker", "calls", "gq"].iter().all(|k| m.contains_key(*k)) => m,
            _ => return Err("Explicit name/is_y_marker/calls/gq required".into()),
        };
        if !ascii_bounded(&m["name"]) {
            return Err("Bounded ASCII marker name required".into());
        }
        boolean(&m["is_y_marker"], "is_y_marker")?;
        let calls = match &m["calls"] {
            Value::Array(c) if c.len() <= 4096 => c,
            _ => return Err("At most4096 calls per marker required".into()),
        };
        for call in calls {
            let c = match call {
                Value::Object(c) if c.len() == 2 && c.contains_key("label") && c.contains_key("off_ladder") => c,
                _ => return Err("Explicit label/off_ladder required".into()),
            };
            if !ascii_bounded(&c["label"]) {
                return Err("Bounded ASCII label required".into());
            }
            boolean(&c["off_ladder"], "off_ladder")?;
        }
        if !m["gq"].is_null() {
            number(&m["gq"], "marker GQ", 0.0, 1.0)?;
        }
        prepared.push(m);
    }
    let names: HashSet<&str> = prepared.iter().map(|m| m["name"].as_str().unwrap()).collect();
    if names.len() != prepared.len() {
        return Err("Unique marker names required".into());
    }
    let mut result = Map::new();
    result.insert("status".into(), json!("not_applicable"));
    result.insert("reason".into(), Value::Null);
    result.insert("weight".into(), fv(weight));
    result.insert("mismatch".into(), Value::Null);
    result.insert("updates".into(), json!([]));
    result.insert("external_validation".into(), json!(false));
    result.insert("call_confidences_recomputed".into(), json!(false));
    result.insert("scope".into(), json!("fresh_results_without_overrides_or_partial_exception_mutations"));
    let unavailable = |mut r: Map<String, Value>, why: &str| {
        r.insert("status".into(), json!("unavailable"));
        r.insert("reason".into(), json!(why));
        Ok(Value::Object(r))
    };
    if st == "Negative Control" || st == "Allelic Ladder" {
        result.insert("reason".into(), json!("sample_type_excluded"));
        return Ok(Value::Object(result));
    }
    if prepared.is_empty() {
        return unavailable(result, "panel_markers_missing");
    }
    let name_of = |m: &Map<String, Value>| m["name"].as_str().unwrap().to_string();
    let is_amel_name = |n: &str| matches!(n.to_ascii_lowercase().as_str(), "amel" | "amelogenin");
    let validator = prepared.iter().find(|m| matches!(m["name"].as_str().unwrap(), "AMEL" | "Amelogenin"));
    let validator = match validator {
        Some(v) => v,
        None => {
            result.insert("reason".into(), json!("validator_exact_amel_name_missing"));
            return Ok(Value::Object(result));
        }
    };
    if validator["gq"].is_null() {
        return unavailable(result, "validator_amel_quality_missing");
    }
    let amel = prepared.iter().find(|m| is_amel_name(&name_of(m))).unwrap();
    if amel["gq"].is_null() {
        return unavailable(result, "amel_quality_missing");
    }
    let ys: Vec<&&Map<String, Value>> = prepared.iter().filter(|m| m["is_y_marker"] == json!(true) && !is_amel_name(&name_of(m))).collect();
    if ys.is_empty() {
        result.insert("reason".into(), json!("no_y_markers"));
        return Ok(Value::Object(result));
    }
    if ys.iter().any(|m| m["gq"].is_null()) {
        return unavailable(result, "y_marker_quality_missing");
    }
    let labels: Vec<String> = amel["calls"].as_array().unwrap().iter().map(|c| c["label"].as_str().unwrap().to_ascii_lowercase()).collect();
    let has = |s: &str| labels.iter().any(|l| l == s);
    let mut count: i64 = if has("x") { 1 + has("y") as i64 } else { -1 };
    if count != labels.len() as i64 {
        count = -1;
    }
    let mut mismatch = count < 1;
    let mut reason = if mismatch { "invalid_amel_combination" } else { "consistent_amel_y_calls" };
    let mut mismatch_marker: Option<String> = None;
    for m in &ys {
        if mismatch {
            break;
        }
        let called = m["calls"].as_array().unwrap().iter().filter(|c| c["off_ladder"] != json!(true)).count();
        if (count == 1 && called > 0) || (count == 2 && called == 0) {
            mismatch = true;
            reason = "amel_y_presence_mismatch";
            mismatch_marker = Some(name_of(m));
        }
    }
    result.insert("status".into(), json!("evaluated_subset"));
    result.insert("reason".into(), json!(reason));
    result.insert("mismatch".into(), json!(mismatch));
    result.insert("amel_marker".into(), amel["name"].clone());
    result.insert("amel_call_count".into(), json!(count));
    result.insert("first_mismatch_marker".into(), mismatch_marker.map_or(Value::Null, |s| json!(s)));
    let mut updates = Vec::new();
    for m in ys.iter().map(|m| **m).chain(std::iter::once(*amel)) {
        let before = m["gq"].as_f64().unwrap();
        updates.push(json!({
            "marker": m["name"].clone(), "metric": mismatch as i64, "gq_before": fv(before),
            "gq_after": fv(if mismatch { before * (1.0 - weight) } else { before }),
        }));
    }
    result.insert("updates".into(), Value::Array(updates));
    Ok(Value::Object(result))
}
