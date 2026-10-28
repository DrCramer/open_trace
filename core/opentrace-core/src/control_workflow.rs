//! Port of `genemapper_open/control_workflow.py` (definition given as parsed mapping).
use crate::pullup_quality::{digest, py_eq, truthy};
use crate::quality_control::{composite_control_category, negative_control, positive_control, zero_peak_metric};
use serde_json::{json, Map, Value};

pub type Res<T> = Result<T, String>;

fn same_keys(m: &Map<String, Value>, keys: &[&str]) -> bool {
    m.len() == keys.len() && keys.iter().all(|k| m.contains_key(*k))
}

fn is_int(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) if n.is_i64() || n.is_u64() => n.as_i64(),
        _ => None,
    }
}

fn update(row: &mut Map<String, Value>, add: &Value) {
    if let Value::Object(m) = add {
        for (k, v) in m {
            row.insert(k.clone(), v.clone());
        }
    }
}

pub fn evaluate_controls(report: &Value, definition: &Value) -> Res<Value> {
    let d = match definition {
        Value::Object(m) if same_keys(m, &["schema_version", "sample_type", "markers"]) => m,
        _ => return Err("Explicit schema_version/sample_type/markers control definition required".into()),
    };
    if is_int(&d["schema_version"]) != Some(1) {
        return Err("Control definition schema_version must be1".into());
    }
    let kind = match d["sample_type"].as_str() {
        Some(k @ ("Positive Control" | "Negative Control" | "Sample")) => k,
        _ => return Err("Choose exact Positive Control, Negative Control or Sample".into()),
    };
    let definitions = match &d["markers"] {
        Value::Object(m) if m.len() <= 128 => m,
        _ => return Err("At most128 explicit marker definitions required".into()),
    };
    let calling = match report.get("allele_calling") {
        Some(c) if report.is_object() && c.is_object() => c,
        _ => return Err("A completed STR workflow report is required".into()),
    };
    let markers = match calling.get("markers") {
        Some(Value::Array(m)) if m.len() <= 128 => m,
        _ => return Err("Bounded marker list required".into()),
    };
    let mut outputs: Vec<Value> = Vec::new();
    for marker in markers {
        let name = marker.get("name").ok_or("KeyError: 'name'")?;
        let name_s = name.as_str().ok_or("unhashable or non-string marker name")?;
        let settings = definitions.get(name_s);
        let mut out = Map::new();
        out.insert("marker".into(), name.clone());
        out.insert("cc".into(), json!(-2));
        out.insert("gq_forced_zero".into(), json!(false));
        out.insert("comparison_performed".into(), json!(false));
        out.insert("reason".into(), Value::Null);
        if marker.get("status") != Some(&json!("tentative")) {
            out.insert("reason".into(), json!("marker_analysis_rejected"));
        } else if kind == "Sample" {
            out.insert("reason".into(), json!("not_a_control"));
        } else if settings.is_none() {
            out.insert("reason".into(), json!("explicit_marker_control_definition_missing"));
        } else {
            let s = match settings.unwrap() {
                Value::Object(m) if same_keys(m, &["expected_alleles", "quality_sensor", "is_y_marker"]) => m,
                _ => return Err(format!("{}: expected_alleles/quality_sensor/is_y_marker required", name_s)),
            };
            if !s["quality_sensor"].is_boolean() || !s["is_y_marker"].is_boolean() {
                return Err(format!("{}: explicit sensor/Y booleans required", name_s));
            }
            let calls = marker.get("calls").and_then(|c| c.as_array()).ok_or("TypeError: calls")?;
            let called = Value::Array(calls.iter().map(|c| c.get("label").cloned().ok_or("KeyError: 'label'".to_string())).collect::<Res<Vec<_>>>()?);
            if kind == "Positive Control" {
                let r = positive_control(&called, &s["expected_alleles"], &s["is_y_marker"])?;
                update(&mut out, &r);
            } else {
                let reviews = match marker.get("peak_review") {
                    Some(Value::Array(r)) => r,
                    _ => return Err(format!("{}: pre-cutoff peak review required", name_s)),
                };
                let mut candidates = 0i64;
                for peak in reviews {
                    let flags = peak.get("flags").cloned().unwrap_or_else(|| json!([]));
                    let has = |f: &str| match &flags {
                        Value::Array(a) => a.iter().any(|x| x == f),
                        Value::String(s) => s.contains(f),
                        _ => false,
                    };
                    if !(has("spike") || has("pullup")) {
                        candidates += 1;
                    }
                }
                let zero = zero_peak_metric(&json!(candidates), &called, &s["quality_sensor"], &s["expected_alleles"])?;
                let neg = negative_control(&zero["value"])?;
                update(&mut out, &neg);
                out.insert("zero_peak".into(), zero);
                out.insert("comparison_performed".into(), json!(true));
                out.insert("reason".into(), json!("negative_zero_peak_rule"));
            }
        }
        outputs.push(Value::Object(out));
    }
    let ccs = Value::Array(outputs.iter().map(|o| o["cc"].clone()).collect());
    let composite = composite_control_category(&ccs)?;
    let all = !outputs.is_empty() && outputs.iter().all(|r| truthy(&r["comparison_performed"]));
    Ok(json!({
        "schema_version": 1,
        "sample_type": kind,
        "definition_sha256": digest(definition)?,
        "input_observations_sha256": digest(calling)?,
        "markers": outputs,
        "composite_control": composite,
        "all_control_comparisons_performed": all,
        "external_validation": false,
        "original_runtime_validated": false,
        "not_applied": ["original full GQ weights", "spike penalty", "custom profile selection/UI validation",
                        "stale results or overrides", "original case-sensitive sample type entry routes"],
    }))
}

pub fn attach_control_result(report: &Value, definition: &Value) -> Res<Value> {
    let quality = evaluate_controls(report, definition)?;
    let mut result = report.as_object().cloned().ok_or("A completed STR workflow report is required")?;
    let prior = result.get("quality").filter(|q| truthy(q)).and_then(|q| q.get("controls")).cloned().unwrap_or(Value::Null);
    let q = result.entry("quality").or_insert_with(|| json!({})).as_object_mut().ok_or("TypeError: quality")?;
    q.insert("controls".into(), quality.clone());
    let prior_or_empty = if truthy(&prior) { prior.clone() } else { json!({}) };
    let prior_type = prior_or_empty.get("sample_type").cloned().unwrap_or(Value::Null);
    let new_type = quality["sample_type"].clone();
    let route_changed = prior_type != new_type && (new_type == json!("Negative Control") || prior_type == json!("Negative Control"));
    if let Some(mq) = q.get_mut("marker_gq") {
        if truthy(mq) && !py_eq(&prior, &quality) {
            let mode = mq.get("control_mode").cloned().unwrap_or(Value::Null);
            if mode == json!("positive_attached") || mode == json!("negative_attached") || route_changed {
                let m = mq.as_object_mut().ok_or("AttributeError: marker_gq")?;
                m.insert("status".into(), json!("stale"));
                m.insert("stale_reason".into(), json!("control_observations_changed"));
                m.insert("sample_category".into(), Value::Null);
                m.insert("complete_for_requested_subset".into(), json!(false));
                m.insert("all_markers_evaluated".into(), json!(false));
            }
        }
    }
    let mut settings = match result.get("settings") {
        None => Map::new(),
        Some(Value::Object(m)) => m.clone(),
        _ => return Err("AttributeError: settings".into()),
    };
    settings.insert("control_definition_sha256".into(), quality["definition_sha256"].clone());
    result.insert("settings".into(), Value::Object(settings));
    Ok(Value::Object(result))
}
