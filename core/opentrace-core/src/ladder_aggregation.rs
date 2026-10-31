//! Port of `genemapper_open/ladder_aggregation.py` (kit given as parsed mapping).
use crate::fsum::fsum;
use crate::kit_definition::load_kit_definition;
use serde_json::{json, Map, Value};
use std::collections::HashSet;

pub type Res<T> = Result<T, String>;

fn fv(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

/// JSON number that is a Python int or float (bool excluded).
fn plain(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Number(n)) => n.as_f64(),
        _ => None,
    }
}

pub fn aggregate_ladder_offsets(entries: &Value, kit: &Value) -> Res<Value> {
    let definition = load_kit_definition(kit)?;
    let list = match entries {
        Value::Array(a) if (1..=128).contains(&a.len()) => a,
        _ => return Err("Explicit list of1..128 ladder entries required".into()),
    };
    let mut ids: HashSet<String> = HashSet::new();
    for entry in list {
        let em = match entry {
            Value::Object(m) if m.len() == 3 && ["ladder_id", "quality_code", "result"].iter().all(|k| m.contains_key(*k)) => m,
            _ => return Err("ladder_id/quality_code/result required".into()),
        };
        match &em["ladder_id"] {
            Value::String(s) if (1..=256).contains(&s.chars().count()) && !ids.contains(s) => {
                ids.insert(s.clone());
            }
            _ => return Err("Bounded unique ladder_id required".into()),
        }
        if plain(Some(&em["quality_code"])).is_none() {
            return Err("Explicit finite quality_code required".into());
        }
        match &em["result"] {
            Value::Object(r) if matches!(r.get("markers"), Some(Value::Object(_))) => {}
            _ => return Err("Explicit ladder result with markers required".into()),
        }
    }
    let allowed = [0.0f64, -2.0, 0.5];
    let mut out_markers = Map::new();
    for marker in definition["markers"].as_array().unwrap() {
        let name = marker["name"].as_str().unwrap();
        let needed: Vec<&str> = marker["ladder_labels"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        let mut accepted: Vec<(&str, &Map<String, Value>)> = Vec::new();
        let mut excluded: Vec<Value> = Vec::new();
        for entry in list {
            let id = entry["ladder_id"].as_str().unwrap();
            let result = entry["result"].as_object().unwrap();
            let record = result["markers"].as_object().unwrap().get(name);
            let code = plain(Some(&entry["quality_code"])).unwrap();
            if !allowed.contains(&code) {
                excluded.push(json!({"ladder_id": id, "reason": "quality_code_not_admitted"}));
                continue;
            }
            let status_ok = matches!(result.get("status"), Some(Value::String(s)) if s == "accepted" || s == "partial");
            let rec = match record {
                Some(Value::Object(r)) if status_ok && r.get("status") == Some(&json!("accepted")) => r,
                _ => {
                    excluded.push(json!({"ladder_id": id, "reason": "marker_offsets_not_accepted"}));
                    continue;
                }
            };
            let offsets = match rec.get("bin_offsets") {
                Some(Value::Object(o)) if !needed.is_empty() && needed.iter().all(|l| o.contains_key(*l)) => o,
                _ => return Err("Accepted ladder needs all explicit ladder-label offsets".into()),
            };
            let mut values: Vec<Option<f64>> = needed.iter().map(|l| plain(offsets.get(*l))).collect();
            values.push(plain(rec.get("min_offset_bp")));
            values.push(plain(rec.get("max_offset_bp")));
            if values.iter().any(|v| v.map_or(true, |x| !x.is_finite() || x.abs() > 1_000_000.0)) {
                return Err("Finite bounded offsets required".into());
            }
            accepted.push((id, rec));
        }
        let mut record = Map::new();
        record.insert("status".into(), json!("rejected"));
        record.insert("bin_offsets".into(), json!({}));
        record.insert("min_offset_bp".into(), Value::Null);
        record.insert("max_offset_bp".into(), Value::Null);
        record.insert("source_ladders".into(), Value::Array(accepted.iter().map(|(i, _)| json!(i)).collect()));
        record.insert("excluded".into(), Value::Array(excluded));
        if !accepted.is_empty() {
            let n = accepted.len() as f64;
            let mean = |f: &dyn Fn(&Map<String, Value>) -> f64| fsum(accepted.iter().map(|(_, r)| f(r))) / n;
            let mut offs = Map::new();
            for label in &needed {
                offs.insert((*label).to_string(), fv(mean(&|r| r["bin_offsets"][*label].as_f64().unwrap())));
            }
            record.insert("status".into(), json!("accepted"));
            record.insert("bin_offsets".into(), Value::Object(offs));
            record.insert("min_offset_bp".into(), fv(mean(&|r| r["min_offset_bp"].as_f64().unwrap())));
            record.insert("max_offset_bp".into(), fv(mean(&|r| r["max_offset_bp"].as_f64().unwrap())));
        }
        out_markers.insert(name.to_string(), Value::Object(record));
    }
    let statuses: Vec<&str> = out_markers.values().map(|r| r["status"].as_str().unwrap()).collect();
    let status = if statuses.iter().all(|s| *s == "accepted") {
        "accepted"
    } else if statuses.contains(&"accepted") {
        "partial"
    } else {
        "rejected"
    };
    let reasons: Vec<Value> = if status != "accepted" {
        vec![json!("Some markers have no admitted complete ladder offsets")]
    } else {
        vec![]
    };
    Ok(json!({
        "status": status,
        "reasons": reasons,
        "markers": Value::Object(out_markers),
        "diagnostics": {
            "method": "unweighted_per_bin_mean_with_explicit_numeric_quality_gate",
            "admitted_quality_codes": [fv(0.0), fv(-2.0), fv(0.5)],
            "source_contract": "IDX16_ALLELE_CALLING AL-05",
            "quality_codes_computed_here": false,
            "external_validation": false,
            "missing_offset_policy": "withhold_marker_if_no_admitted_complete_ladder",
        },
    }))
}
