//! Port of `genemapper_open/ladder_quality_workflow.py` (definition given as parsed mapping).
use crate::allele_calling::call_sample;
use crate::fsum::fsum;
use crate::kit_definition::{load_kit_definition, py_json_dumps_ascii};
use crate::ladder_calling::ladder_call_observations;
use crate::ladder_observation_binding::{bind, validate};
use crate::ladder_quality::{object, score_ladder_marker, validate_ladder_recipe};
use crate::pullup_quality::digest;
use crate::quality_control::{number, quality_category};
use crate::spike_adjustment::marker_has_spike;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};

pub type Res<T> = Result<T, String>;

fn fv(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

pub fn load_ladder_quality_definition(source: &Value) -> Res<Value> {
    if let Value::String(text) = source {
        if text.len() > 1_048_576 {
            return Err("Ladder quality definition exceeds1MiB".into());
        }
        let parsed = crate::pyjson::loads_opts(text, true, true).map_err(|e| match e {
            crate::pyjson::LoadErr::Dup(k) => format!("Duplicate ladder quality JSON field: {}", k),
            other => other.message(),
        })?;
        return load_ladder_quality_definition(&parsed);
    }
    if py_json_dumps_ascii(source).len() > 1_048_576 {
        return Err("Ladder quality definition exceeds1MiB".into());
    }
    let s = object(
        source,
        &["schema_version", "calling_parameters_source", "ladder_sq", "sq_thresholds", "gq_thresholds", "markers"],
        "ladder quality definition",
    )?;
    if !matches!(&s["schema_version"], Value::Number(n) if n.as_i64() == Some(1)) {
        return Err("Ladder quality schema_version must be1".into());
    }
    if s["calling_parameters_source"] != json!("kit_sample_parameters") && s["calling_parameters_source"] != json!("ladder_matching_calls") {
        return Err("Explicit ladder_matching_calls or legacy kit_sample_parameters policy required".into());
    }
    for key in ["sq_thresholds", "gq_thresholds"] {
        let t = object(&s[key], &["good", "bad"], key)?;
        quality_category(&json!(0.5), &t["good"], &t["bad"])?;
    }
    let sq = s["ladder_sq"].as_object().ok_or("Explicit ladder_sq source required")?;
    match sq.get("source") {
        Some(Value::String(x)) if x == "base_quality" || x == "post_sizing_quality" => {
            object(&s["ladder_sq"], &["source"], "ladder_sq")?;
        }
        Some(Value::String(x)) if x == "explicit" => {
            object(&s["ladder_sq"], &["source", "score"], "ladder_sq")?;
            number(&sq["score"], "explicit ladder SQ", 0.0, 1.0)?;
        }
        _ => return Err("ladder_sq source must be base_quality, post_sizing_quality or explicit".into()),
    }
    let markers = match &s["markers"] {
        Value::Object(m) if (1..=128).contains(&m.len()) => m,
        _ => return Err("Explicit1..128 marker recipes required".into()),
    };
    for (name, recipe) in markers {
        if !(1..=128).contains(&name.chars().count()) {
            return Err("Bounded marker name required".into());
        }
        validate_ladder_recipe(recipe)?;
    }
    Ok(source.clone())
}

fn dig(v: &Value) -> Res<String> {
    digest(v)
}

/// Python `gate_ladder_offsets`: returns (effective offsets, quality).
pub fn gate_ladder_offsets(
    peaks: &Value,
    kit: &Value,
    matching: &Value,
    definition: &Value,
    sizing: &Value,
    missing_offset_policy: &Value,
    call_observations: Option<&Value>,
) -> Res<(Value, Value)> {
    let definition = load_ladder_quality_definition(definition)?;
    let kit = load_kit_definition(kit)?;
    let known: HashSet<&str> = kit["markers"].as_array().unwrap().iter().map(|m| m["name"].as_str().unwrap()).collect();
    if definition["markers"].as_object().unwrap().keys().any(|k| !known.contains(k.as_str())) {
        return Err("Ladder quality recipe references unknown kit marker".into());
    }
    let policy = definition["calling_parameters_source"].as_str().unwrap().to_string();
    let observations = match call_observations {
        None => {
            if policy == "ladder_matching_calls" {
                ladder_call_observations(peaks, &kit, matching)?
            } else {
                let called = call_sample(peaks, &kit, matching, &json!(true), None, missing_offset_policy, None)?;
                bind(peaks, &kit, matching, &called)?
            }
        }
        Some(o) if o.is_object() => o.clone(),
        Some(_) => return Err("Ladder call observations must be an object".into()),
    };
    let expected_method = if policy == "ladder_matching_calls" { "ladder_matching_calls" } else { "explicit_kit_ordered_sample_filters" };
    let method = observations.get("diagnostics").and_then(|d| d.get("method")).cloned().unwrap_or(Value::Null);
    if method != json!(expected_method) {
        return Err("Ladder call observation source differs from requested policy".into());
    }
    let observations = validate(peaks, &kit, matching, &observations)?;
    let by_marker: HashMap<&str, &Value> =
        observations["markers"].as_array().unwrap().iter().map(|r| (r["name"].as_str().unwrap(), r)).collect();
    let mut sq: Option<f64> = None;
    let mut global_reason: Option<&str> = None;
    let sq_source = definition["ladder_sq"]["source"].as_str().unwrap();
    if sq_source == "explicit" {
        sq = Some(definition["ladder_sq"]["score"].as_f64().unwrap());
    } else {
        let key = if sq_source == "post_sizing_quality" { "post_quality" } else { "base_quality" };
        let value = sizing.get(key).filter(|v| !v.is_null() && *v != &json!({})).cloned().unwrap_or_else(|| json!({}));
        if sizing.is_object() && sizing.get("status") == Some(&json!("accepted")) && value.get("status") == Some(&json!("evaluated")) {
            sq = Some(number(value.get("score").unwrap_or(&Value::Null), "ladder sizing quality", 0.0, 1.0)?);
        } else {
            global_reason = Some(if key == "post_quality" { "accepted_ladder_post_sizing_quality_missing" } else { "accepted_ladder_base_quality_missing" });
        }
    }
    let sq_thresholds = &definition["sq_thresholds"];
    let gq_thresholds = &definition["gq_thresholds"];
    let sq_category = match sq {
        Some(s) => quality_category(&fv(s), &sq_thresholds["good"], &sq_thresholds["bad"])?,
        None => Value::Null,
    };
    let mut result = Map::new();
    result.insert("schema_version".into(), json!(1));
    result.insert("status".into(), json!("unavailable"));
    result.insert("definition_sha256".into(), json!(dig(&definition)?));
    result.insert("scope".into(), json!("fresh_single_ladder_gq_and_sq_subset_with_explicit_call_observation_policy"));
    result.insert("calling_parameters_source".into(), definition["calling_parameters_source"].clone());
    result.insert("ladder_sq_source".into(), json!(sq_source));
    result.insert("ladder_sq".into(), sq.map_or(Value::Null, fv));
    result.insert("sq_category".into(), sq_category.clone());
    result.insert("markers".into(), json!([]));
    result.insert("all_markers_evaluated".into(), json!(false));
    result.insert("gq_category".into(), Value::Null);
    result.insert("al_subset_code".into(), Value::Null);
    result.insert("gate_passed".into(), json!(false));
    result.insert("full_original_al".into(), json!(false));
    result.insert("external_validation".into(), json!(false));
    result.insert("original_runtime_validated".into(), json!(false));
    result.insert(
        "not_applied".into(),
        json!(["automatic spike/pullup detectors", "original full SQ/PQV composition", "original ladder call selection equivalence", "overrides, null-peak states and original rounding"]),
    );
    let mut rows: Vec<Value> = Vec::new();
    let plist = peaks.as_array().unwrap();
    for marker in kit["markers"].as_array().unwrap() {
        let name = marker["name"].as_str().unwrap();
        let recipe = definition["markers"].get(name);
        let observed = by_marker.get(name).copied();
        let mut row = Map::new();
        row.insert("marker".into(), json!(name));
        row.insert("status".into(), json!("unavailable"));
        row.insert("reason".into(), Value::Null);
        row.insert("score".into(), Value::Null);
        row.insert("category".into(), Value::Null);
        row.insert("calls".into(), json!([]));
        let mut reason: Option<&str> = global_reason;
        let matched_status = matching.get("markers").and_then(|m| m.get(name)).and_then(|m| m.get("status"));
        if matched_status != Some(&json!("accepted")) {
            reason = Some("ladder_matching_not_accepted");
        } else if observed.map_or(true, |o| o["status"] != json!("tentative")) {
            reason = Some("ladder_call_observations_unavailable");
        } else if recipe.is_none() {
            reason = Some("explicit_ladder_quality_recipe_missing");
        }
        let mut effective = recipe.cloned().unwrap_or(Value::Null);
        if reason.is_none() {
            let observed = observed.unwrap();
            let reviews = observed["peak_review"].as_array().unwrap();
            let indices: Vec<usize> = reviews.iter().map(|p| p["peak_index"].as_u64().unwrap() as usize).collect();
            if indices.iter().any(|i| !matches!(plist[*i].get("offscaled"), Some(Value::Bool(_)))) {
                reason = Some("ladder_offscale_state_missing");
            } else {
                let seen_spike = marker_has_spike(observed)?;
                let seen_offscale = indices.iter().any(|i| plist[*i]["offscaled"] == json!(true))
                    || reviews.iter().any(|p| p.get("flags").and_then(|f| f.as_array()).map_or(false, |f| f.iter().any(|x| x == "offscale")));
                let em = effective.as_object_mut().unwrap();
                if seen_spike {
                    em.insert("spike_metric".into(), json!(1));
                }
                if seen_offscale {
                    em.insert("offscale_metric".into(), json!(1));
                }
                row.insert("observed_flags".into(), json!({"spike": seen_spike, "offscale": seen_offscale}));
            }
        }
        if let Some(r) = reason {
            row.insert("reason".into(), json!(r));
            rows.push(Value::Object(row));
            continue;
        }
        let observed = observed.unwrap();
        let bins: HashMap<&str, &Value> = marker["bins"].as_array().unwrap().iter().map(|b| (b["label"].as_str().unwrap(), b)).collect();
        let mut calls: Vec<Value> = Vec::new();
        for call in observed["calls"].as_array().unwrap() {
            let label = call["label"].clone();
            let bin = if call["off_ladder"] == json!(true) { None } else { label.as_str().and_then(|l| bins.get(l)) };
            calls.push(json!({
                "name": label.clone(), "size_bp": call["size_bp"].clone(), "peak_bp": call["size_bp"].clone(),
                "height": call["height"].clone(), "off_ladder": call["off_ladder"].clone(),
                "bin": match bin { Some(b) => json!({"name": label.clone(), "center_bp": b["center_bp"].clone()}), None => Value::Null },
            }));
        }
        row.insert("calls".into(), Value::Array(calls.clone()));
        let score = score_ladder_marker(&Value::Array(calls), &marker["ladder_labels"], &effective)?;
        let category = quality_category(&score["gq"], &gq_thresholds["good"], &gq_thresholds["bad"])?;
        row.insert("status".into(), json!("evaluated_subset"));
        row.insert("score".into(), score.clone());
        row.insert("category".into(), category);
        let source_key = if policy == "ladder_matching_calls" { "confidence" } else { "confidence_weight" };
        let mut initial: Vec<f64> = Vec::new();
        for c in observed["calls"].as_array().unwrap() {
            initial.push(number(c.get(source_key).unwrap_or(&Value::Null), "ladder initial confidence", 0.0, 1e12)?);
        }
        let mean = if initial.is_empty() { None } else { Some(fsum(initial.iter().cloned()) / initial.len() as f64) };
        row.insert("initial_confidence_mean".into(), mean.map_or(Value::Null, fv));
        row.insert(
            "confidence_source".into(),
            json!(if policy == "ladder_matching_calls" { "ladder_ratio_and_unmatched_height_penalty" } else { "legacy_bin_confidence_weight" }),
        );
        let gq = score["gq"].as_f64().unwrap();
        let scores: Vec<Value> = observed["calls"]
            .as_array()
            .unwrap()
            .iter()
            .zip(initial.iter())
            .map(|(c, v)| {
                let m = mean.unwrap();
                json!({
                    "peak_index": c["peak_index"].clone(), "label": c["label"].clone(), "initial_confidence": fv(*v),
                    "after_marker_gq": fv(m * gq), "after_sample_sq": fv(m * gq * sq.unwrap()),
                })
            })
            .collect();
        row.insert("call_scores".into(), Value::Array(scores));
        rows.push(Value::Object(row));
    }
    let evaluated: Vec<&Value> = rows.iter().filter(|r| r["status"] == json!("evaluated_subset")).collect();
    let all_eval = !rows.is_empty() && evaluated.len() == rows.len();
    result.insert("all_markers_evaluated".into(), json!(all_eval));
    if !evaluated.is_empty() {
        result.insert("status".into(), json!(if all_eval { "evaluated_subset" } else { "partial" }));
    }
    let mut gate_passed = false;
    if all_eval {
        let gq_code = evaluated.iter().map(|r| r["category"]["code"].as_i64().unwrap()).max().unwrap();
        result.insert("gq_category".into(), json!(gq_code));
        let al = if gq_code == 0 { sq_category["code"].as_i64().unwrap() } else { 2 };
        result.insert("al_subset_code".into(), json!(al));
        gate_passed = al == 0;
        result.insert("gate_passed".into(), json!(gate_passed));
    }
    result.insert("markers".into(), Value::Array(rows));
    result.insert(
        "input_observations_sha256".into(),
        json!(dig(&json!({"peaks": peaks, "kit": kit, "matching": matching, "sizing": sizing, "calling": observations}))?),
    );
    if let Some(sa) = observations.get("spike_adjustments") {
        result.insert("spike_adjustments".into(), sa.clone());
        result.get_mut("not_applied").unwrap().as_array_mut().unwrap()[0] = json!("automatic pullup detector");
    }
    let mut effective_offsets = matching.as_object().cloned().ok_or("matching must be an object")?;
    let mut gate = Map::new();
    for key in ["status", "definition_sha256", "al_subset_code", "gate_passed", "full_original_al"] {
        gate.insert(key.into(), result[key].clone());
    }
    effective_offsets.insert("quality_gate".into(), Value::Object(gate));
    let reason_text = json!("Requested whole-ladder quality subset did not admit offsets");
    if let Some(Value::Object(markers)) = effective_offsets.get_mut("markers") {
        for (_, record) in markers.iter_mut() {
            let rm = record.as_object_mut().ok_or("marker record must be an object")?;
            let before = rm.get("status").cloned().ok_or("KeyError: 'status'")?;
            rm.insert("matching_status_before_quality".into(), before);
            if !gate_passed {
                rm.insert("status".into(), json!("rejected"));
                rm.insert("bin_offsets".into(), json!({}));
                rm.insert("min_offset_bp".into(), Value::Null);
                rm.insert("max_offset_bp".into(), Value::Null);
                let reasons = rm.entry("reasons").or_insert_with(|| json!([]));
                reasons.as_array_mut().ok_or("reasons must be a list")?.push(reason_text.clone());
            }
        }
    }
    if !gate_passed {
        effective_offsets.insert("status".into(), json!("rejected"));
        let reasons = effective_offsets.entry("reasons").or_insert_with(|| json!([]));
        reasons.as_array_mut().ok_or("reasons must be a list")?.push(reason_text);
    }
    Ok((Value::Object(effective_offsets), Value::Object(result)))
}
