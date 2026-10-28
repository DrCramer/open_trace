//! Port of `genemapper_open/marker_quality_workflow.py` (definition given as parsed mapping).
use crate::amel_cross_check::evaluate_amel_cross_check;
use crate::kit_definition::py_json_dumps_ascii;
use crate::ladder_quality::object;
use crate::marker_quality::{score_negative_marker, score_ordinary_marker, score_y_marker};
use crate::pullup_quality::{digest, py_eq, truthy};
use crate::quality_control::{number, quality_category};
use crate::spike_adjustment::marker_has_spike;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};

pub type Res<T> = Result<T, String>;

fn fv(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

/// Python `_definition`: JSON text is parsed (duplicate keys and non-finite constants rejected), then validated.
pub fn load_marker_quality_definition(source: &Value) -> Res<Value> {
    if let Value::String(text) = source {
        if text.len() > 1_048_576 {
            return Err("Marker quality definition exceeds1MiB".into());
        }
        use crate::pyjson::{loads_opts, LoadErr};
        let parsed = match loads_opts(text, true, true) {
            Ok(v) => v,
            Err(LoadErr::Dup(k)) => return Err(format!("Duplicate marker quality JSON field: {}", k)),
            Err(LoadErr::Const(c)) => return Err(format!("Nonfinite JSON constant: {}", c)),
            Err(LoadErr::Decode(m)) => return Err(m),
            Err(LoadErr::Recursion(_)) => return Err("Bounded JSON marker quality definition required".into()),
        };
        return load_definition_value(&parsed);
    }
    load_definition_value(source)
}

fn load_definition_value(source: &Value) -> Res<Value> {
    if py_json_dumps_ascii(source).len() > 1_048_576 {
        return Err("Marker quality definition exceeds1MiB".into());
    }
    let version = match source.get("schema_version") {
        Some(Value::Number(n)) if matches!(n.as_i64(), Some(1) | Some(2)) => n.as_i64().unwrap(),
        _ => return Err("schema_version must be1 or2".into()),
    };
    let base = ["schema_version", "sample_sq", "gq_thresholds", "amel_marker", "control_mode", "markers"];
    let keys: Vec<&str> = if version == 1 { base.to_vec() } else { [&base[..], &["sample_type", "amel_cross_check"]].concat() };
    let s = object(source, &keys, "definition")?;
    let modes: &[&str] = if version == 1 { &["none", "positive_attached"] } else { &["none", "positive_attached", "negative_attached"] };
    if !matches!(s["control_mode"].as_str(), Some(m) if modes.contains(&m)) {
        return Err("Unsupported explicit control_mode".into());
    }
    if version == 2 {
        let required = |t: &str| match t {
            "Sample" => Some("none"),
            "Positive Control" => Some("positive_attached"),
            "Negative Control" => Some("negative_attached"),
            _ => None,
        };
        let need = s["sample_type"].as_str().and_then(required);
        let need = need.ok_or("Explicit Sample/Positive Control/Negative Control required")?;
        if s["control_mode"] != json!(need) {
            return Err("sample_type and control_mode must agree".into());
        }
        if !s["amel_cross_check"].is_null() {
            let a = object(&s["amel_cross_check"], &["weight"], "amel_cross_check")?;
            number(&a["weight"], "AMELCC sample weight", 0.0, 1.0)?;
        }
    }
    match &s["amel_marker"] {
        Value::Null => {}
        Value::String(a) if (1..=128).contains(&a.chars().count()) => {}
        _ => return Err("Explicit bounded AMEL marker or null required".into()),
    }
    let t = object(&s["gq_thresholds"], &["good", "bad"], "gq_thresholds")?;
    quality_category(&json!(0.5), &t["good"], &t["bad"])?;
    let sq = s["sample_sq"].as_object().ok_or("Explicit sample_sq source required")?;
    match sq.get("source") {
        Some(Value::String(x)) if x == "base_quality" || x == "post_sizing_quality" => {
            object(&s["sample_sq"], &["source"], "sample_sq")?;
        }
        Some(Value::String(x)) if x == "explicit" => {
            object(&s["sample_sq"], &["source", "score"], "sample_sq")?;
            number(&sq["score"], "sample SQ", 0.0, 1.0)?;
        }
        _ => return Err("sample_sq source must be base_quality, post_sizing_quality or explicit".into()),
    }
    let markers = match &s["markers"] {
        Value::Object(m) if (1..=128).contains(&m.len()) => m,
        _ => return Err("Explicit1..128 marker recipes required".into()),
    };
    for (name, recipe) in markers {
        if !(1..=128).contains(&name.chars().count()) {
            return Err("Bounded marker name required".into());
        }
        let negative = version == 2 && s["sample_type"] == json!("Negative Control");
        let r = if negative {
            object(recipe, &["kind", "spike_metric", "spike_weight", "confidence_source"], name)?
        } else {
            object(recipe, &["kind", "thresholds", "weights", "flags", "confidence_source"], name)?
        };
        if r["confidence_source"] != json!("bin_confidence_weight") {
            return Err("Explicit bin_confidence_weight source required".into());
        }
        if negative {
            if !matches!(r["kind"].as_str(), Some("autosomal" | "amel" | "y")) {
                return Err("Explicit autosomal/amel/y kind required".into());
            }
            score_negative_marker(&json!([]), &json!(1), &r["spike_metric"], &r["spike_weight"], &json!(1))?;
            continue;
        }
        if r["kind"] == json!("y") {
            score_y_marker(&json!([]), &r["thresholds"], &r["weights"], &r["flags"], &json!(1), &Value::Null, &json!(false))?;
        } else {
            score_ordinary_marker(&json!([]), &r["thresholds"], &r["weights"], &r["flags"], &json!(1), &r["kind"], &json!(false))?;
        }
    }
    Ok(source.clone())
}

fn get<'a>(v: &'a Value, k: &str) -> &'a Value {
    v.get(k).unwrap_or(&Value::Null)
}

fn tuple_eq(a: &[Value; 3], b: &[Value; 3]) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| py_eq(x, y))
}

pub fn evaluate_marker_quality(report: &Value, definition: &Value) -> Res<Value> {
    let definition = load_marker_quality_definition(definition)?;
    let version = definition["schema_version"].as_i64().unwrap();
    let negative = version == 2 && definition["sample_type"] == json!("Negative Control");
    let calling = match report.get("allele_calling") {
        Some(c) if report.is_object() && c.is_object() => c,
        _ => return Err("Completed STR report required".into()),
    };
    let (markers, calls, peaks) = match (calling.get("markers"), report.get("calls"), report.get("peaks")) {
        (Some(Value::Array(m)), Some(Value::Array(c)), Some(Value::Array(p))) if m.len() <= 128 && c.len() <= 4096 && p.len() <= 4096 => (m, c, p),
        _ => return Err("Bounded STR marker/call/peak observations required".into()),
    };
    let mut by_peak: HashMap<String, &Value> = HashMap::new();
    for p in peaks {
        by_peak.insert(p["id"].as_str().ok_or("KeyError: 'id'")?.to_string(), p);
    }
    if by_peak.len() != peaks.len() {
        return Err("Unique source peak IDs required".into());
    }
    let mut by_marker: HashMap<String, &Value> = HashMap::new();
    for m in markers {
        by_marker.insert(m["name"].as_str().ok_or("KeyError: 'name'")?.to_string(), m);
    }
    if by_marker.len() != markers.len() {
        return Err("Unique marker names required".into());
    }
    let mut sq: Option<f64> = None;
    let mut global_reason: Option<String> = None;
    let sq_source = definition["sample_sq"]["source"].as_str().unwrap();
    if sq_source == "explicit" {
        sq = Some(definition["sample_sq"]["score"].as_f64().unwrap());
    } else {
        let empty = json!({});
        let sizing = report.get("sizing").unwrap_or(&empty);
        let key = if sq_source == "post_sizing_quality" { "post_quality" } else { "base_quality" };
        let base = match sizing.get(key) {
            Some(b) if truthy(b) => b.clone(),
            _ => json!({}),
        };
        if sizing.get("status") == Some(&json!("accepted")) && base.get("status") == Some(&json!("evaluated")) {
            sq = Some(number(get(&base, "score"), "base sizing quality", 0.0, 1.0)?);
        } else {
            global_reason = Some(if key == "post_quality" { "accepted_post_sizing_quality_missing" } else { "accepted_base_quality_missing" }.to_string());
        }
    }
    let controls: Value = match report.get("quality") {
        Some(q) if truthy(q) => q.get("controls").cloned().unwrap_or(Value::Null),
        _ => Value::Null,
    };
    let controls_on = truthy(&controls);
    if controls_on && controls.get("sample_type") == Some(&json!("Negative Control")) && !negative {
        global_reason = Some(if version == 2 { "negative_control_gq_requires_schema2" } else { "negative_control_gq_not_implemented" }.to_string());
    }
    let mode = definition["control_mode"].as_str().unwrap();
    if mode == "positive_attached" || mode == "negative_attached" {
        let expected_type = if negative { "Negative Control" } else { "Positive Control" };
        let prefix = if negative { "negative" } else { "positive" };
        if !controls_on || controls.get("sample_type") != Some(&json!(expected_type)) {
            global_reason = Some(format!("{}_control_observations_missing", prefix));
        } else if controls.get("input_observations_sha256") != Some(&json!(digest(calling)?)) {
            global_reason = Some(format!("{}_control_observations_stale", prefix));
        }
    }
    let mut control_rows: HashMap<String, &Value> = HashMap::new();
    if controls_on {
        for row in controls["markers"].as_array().ok_or("KeyError: 'markers'")? {
            control_rows.insert(row["marker"].as_str().ok_or("KeyError: 'marker'")?.to_string(), row);
        }
    }
    let mut amel_calls: Option<Vec<Value>> = None;
    let mut amel_reason: Option<&str> = None;
    if let Some(am) = definition["amel_marker"].as_str() {
        if !negative {
            match by_marker.get(am) {
                Some(a) if a.get("status") == Some(&json!("tentative")) => {
                    amel_calls = Some(a["calls"].as_array().unwrap().iter().map(|c| c["label"].clone()).collect());
                }
                _ => amel_reason = Some("explicit_amel_marker_unavailable"),
            }
        }
    }
    let mut result = Map::new();
    result.insert("schema_version".into(), json!(1));
    result.insert("definition_schema_version".into(), json!(version));
    result.insert("status".into(), json!("unavailable"));
    result.insert("definition_sha256".into(), json!(digest(&definition)?));
    result.insert("control_mode".into(), definition["control_mode"].clone());
    result.insert("sample_sq_source".into(), json!(sq_source));
    result.insert("sample_sq".into(), sq.map_or(Value::Null, fv));
    result.insert("markers".into(), json!([]));
    result.insert("sample_category".into(), Value::Null);
    result.insert("all_markers_evaluated".into(), json!(false));
    result.insert("complete_for_requested_subset".into(), json!(false));
    result.insert("external_validation".into(), json!(false));
    result.insert("original_runtime_validated".into(), json!(false));
    result.insert("full_original_pqv".into(), json!(false));
    result.insert("not_applied".into(), json!([]));
    let mut not_applied: Vec<Value> =
        ["AMELCC plugin", "ladder GQ", "pullup processor", "overrides and stale original project states", "original floating-point rounding parity"]
            .iter()
            .map(|s| json!(s))
            .collect();
    if !negative {
        not_applied.push(json!("negative-control GQ"));
    }
    let gq_th = &definition["gq_thresholds"];
    let mut rows: Vec<Value> = Vec::new();
    for marker in markers {
        let name = marker["name"].as_str().unwrap();
        let recipe = definition["markers"].get(name);
        let mut row = Map::new();
        row.insert("marker".into(), json!(name));
        row.insert("status".into(), json!("unavailable"));
        row.insert("reason".into(), Value::Null);
        row.insert("score".into(), Value::Null);
        row.insert("category".into(), Value::Null);
        row.insert("call_scores".into(), json!([]));
        let mut reason: Option<String> = global_reason.clone();
        if marker.get("status") != Some(&json!("tentative")) {
            reason = Some("marker_analysis_rejected".into());
        } else if recipe.is_none() {
            reason = Some("explicit_marker_quality_recipe_missing".into());
        } else if recipe.unwrap()["kind"] == json!("y") && amel_reason.is_some() && !negative {
            reason = amel_reason.map(|s| s.to_string());
        }
        let selected: Vec<&Value> = calls.iter().filter(|c| c.get("marker") == Some(&json!(name))).collect();
        let mut observations: Vec<Value> = Vec::new();
        if reason.is_none() {
            let trip = |c: &Value| [get(c, "peak_index").clone(), get(c, "label").clone(), get(c, "size_bp").clone()];
            let expected: Vec<[Value; 3]> = marker["calls"].as_array().unwrap().iter().map(trip).collect();
            let actual: Vec<[Value; 3]> = selected.iter().map(|c| trip(c)).collect();
            if actual.len() != expected.len() || !actual.iter().zip(expected.iter()).all(|(a, b)| tuple_eq(a, b)) {
                return Err(format!("{}: inconsistent root and marker calls", name));
            }
            for call in &selected {
                if negative {
                    observations.push(fv(number(get(call, "confidence_weight"), "initial confidence", 0.0, 1.0)?));
                    continue;
                }
                let peak = call.get("source_peak_id").and_then(|i| i.as_str()).and_then(|i| by_peak.get(i)).copied();
                let morphology = peak.and_then(|p| p.get("morphology_bp")).cloned().unwrap_or_else(|| json!({}));
                let offscaled = peak.and_then(|p| p.get("offscaled"));
                if morphology.get("status") != Some(&json!("evaluated")) || !matches!(offscaled, Some(Value::Bool(_))) {
                    reason = Some("calibrated_morphology_or_offscale_state_missing".into());
                    break;
                }
                observations.push(json!({
                    "label": call["label"].clone(), "height": call["height"].clone(),
                    "fwhm_bp": morphology["fwhm_bp"].clone(), "offscale": offscaled.unwrap().clone(),
                    "confidence": call["confidence_weight"].clone(),
                }));
            }
        }
        let control: Option<&Value> = if mode != "none" { control_rows.get(name).copied() } else { None };
        if reason.is_none() && mode != "none" && control.is_none() {
            reason = Some("marker_control_observation_missing".into());
        }
        if reason.is_none() && negative {
            let c = control.unwrap();
            if !c.get("comparison_performed").map_or(false, truthy) || c.get("zero_peak_value").is_none() {
                reason = Some("negative_zero_peak_observation_missing".into());
            }
        }
        if let Some(r) = reason {
            row.insert("reason".into(), json!(r));
            rows.push(Value::Object(row));
            continue;
        }
        let recipe = recipe.unwrap();
        let mut effective = match recipe.get("flags") {
            Some(f) => f.clone(),
            None => json!({}),
        };
        let off_ladder = selected.iter().any(|c| c.get("off_ladder") == Some(&json!(true)));
        let overlap = selected.iter().any(|c| c.get("flags").and_then(|f| f.as_array()).map_or(false, |f| f.iter().any(|x| x == "cross_marker_overlap")));
        let spike = marker_has_spike(marker)?;
        let mut observed = Map::new();
        observed.insert("binning".into(), json!(off_ladder));
        observed.insert("overlap".into(), json!(overlap));
        observed.insert("spike".into(), json!(spike));
        let scored = if negative {
            let sm = if spike { json!(1) } else { recipe["spike_metric"].clone() };
            score_negative_marker(&Value::Array(observations.clone()), &control.unwrap()["zero_peak_value"], &sm, &recipe["spike_weight"], &fv(sq.ok_or("sample_sq missing")?))?
        } else {
            for (k, v) in &observed {
                let cur = effective[k.as_str()] == json!(true);
                effective[k.as_str()] = json!(cur || v == &json!(true));
            }
            let sqv = fv(sq.ok_or("sample_sq missing")?);
            let mismatch = control.map_or(false, |c| truthy(c) && py_eq(&c["cc"], &json!(1)));
            let obs = Value::Array(observations.clone());
            if recipe["kind"] == json!("y") {
                let amel = amel_calls.as_ref().map_or(Value::Null, |a| Value::Array(a.clone()));
                score_y_marker(&obs, &recipe["thresholds"], &recipe["weights"], &effective, &sqv, &amel, &json!(mismatch))?
            } else {
                score_ordinary_marker(&obs, &recipe["thresholds"], &recipe["weights"], &effective, &sqv, &recipe["kind"], &json!(mismatch))?
            }
        };
        let category = quality_category(&scored["gq"], &gq_th["good"], &gq_th["bad"])?;
        let before = scored["call_confidence_before_sq"].as_array().unwrap();
        let after = scored["call_confidence_after_sq"].as_array().unwrap();
        let call_scores: Vec<Value> = selected
            .iter()
            .zip(before.iter())
            .zip(after.iter())
            .map(|((c, b), a)| json!({"call_id": c["id"].clone(), "before_sample_sq": b.clone(), "after_sample_sq": a.clone()}))
            .collect();
        row.insert("status".into(), json!("evaluated_subset"));
        row.insert("score".into(), scored);
        row.insert("category".into(), category);
        row.insert("call_scores".into(), Value::Array(call_scores));
        row.insert(
            "flag_sources".into(),
            json!({"explicit": recipe.get("flags").cloned().unwrap_or(Value::Null), "observed_call_violations": Value::Object(observed), "effective": effective}),
        );
        row.insert("control_comparison_performed".into(), control.map_or(Value::Null, |c| c["comparison_performed"].clone()));
        rows.push(Value::Object(row));
    }
    let mut plugin_complete = true;
    if let Some(plugin) = definition.get("amel_cross_check").filter(|p| !p.is_null()) {
        let cross = if markers.iter().any(|m| definition["markers"].get(m["name"].as_str().unwrap()).is_none()) {
            json!({"status": "unavailable", "reason": "explicit_panel_marker_roles_missing", "updates": []})
        } else {
            let entries: Vec<Value> = markers
                .iter()
                .zip(rows.iter())
                .map(|(m, r)| {
                    json!({
                        "name": m["name"].clone(),
                        "is_y_marker": definition["markers"][m["name"].as_str().unwrap()]["kind"] == json!("y"),
                        "calls": m["calls"].as_array().unwrap().iter().map(|c| json!({"label": c["label"].clone(), "off_ladder": c["off_ladder"].clone()})).collect::<Vec<_>>(),
                        "gq": if truthy(&r["score"]) { r["score"]["gq"].clone() } else { Value::Null },
                    })
                })
                .collect();
            evaluate_amel_cross_check(&Value::Array(entries), &definition["sample_type"], &plugin["weight"])?
        };
        plugin_complete = cross["status"] != json!("unavailable");
        if plugin_complete {
            not_applied.retain(|x| x != "AMELCC plugin");
        }
        let updates: HashMap<String, &Value> = cross["updates"].as_array().unwrap().iter().map(|u| (u["marker"].as_str().unwrap().to_string(), u)).collect();
        for row in rows.iter_mut() {
            let u = match updates.get(row["marker"].as_str().unwrap()) {
                Some(u) => *u,
                None => continue,
            };
            let rm = row.as_object_mut().unwrap();
            let score = rm.get_mut("score").unwrap().as_object_mut().ok_or("score must be an object")?;
            let gq = score["gq"].clone();
            score.insert("gq_before_amel_cc".into(), gq);
            score.insert("gq".into(), u["gq_after"].clone());
            score.insert("scope".into(), json!("sample_marker_with_amel_cross_check_without_overrides_or_pullup_processor"));
            score.get_mut("metrics").unwrap().as_object_mut().unwrap().insert("amel_cross_check".into(), u["metric"].clone());
            score.get_mut("events").unwrap().as_array_mut().unwrap().push(json!({
                "criterion": "amel_cross_check", "metric": u["metric"].clone(), "gq_before": u["gq_before"].clone(),
                "gq_after": u["gq_after"].clone(), "call_confidences_recomputed": false,
            }));
            let new_gq = score["gq"].clone();
            rm.insert("category".into(), quality_category(&new_gq, &gq_th["good"], &gq_th["bad"])?);
        }
        result.insert("amel_cross_check".into(), cross);
    }
    let evaluated: Vec<&Value> = rows.iter().filter(|r| r["status"] == json!("evaluated_subset")).collect();
    let all_eval = !markers.is_empty() && evaluated.len() == markers.len();
    let complete = all_eval && plugin_complete && evaluated.iter().all(|r| r["control_comparison_performed"] != json!(false));
    let mut status = "unavailable";
    let mut category = Value::Null;
    if !evaluated.is_empty() {
        status = if all_eval && plugin_complete { "evaluated_subset" } else { "partial" };
        category = json!(evaluated.iter().map(|r| r["category"]["code"].as_i64().unwrap()).max().unwrap());
    }
    result.insert("status".into(), json!(status));
    result.insert("sample_category".into(), category);
    result.insert("all_markers_evaluated".into(), json!(all_eval));
    result.insert("complete_for_requested_subset".into(), json!(complete));
    result.insert("markers".into(), Value::Array(rows));
    result.insert("not_applied".into(), Value::Array(not_applied));
    let mut dependency = Map::new();
    for k in ["calls", "peaks", "allele_calling", "sizing"] {
        dependency.insert(k.into(), report.get(k).cloned().unwrap_or(Value::Null));
    }
    if mode != "none" {
        dependency.insert("controls".into(), controls.clone());
    }
    result.insert("input_observations_sha256".into(), json!(digest(&Value::Object(dependency))?));
    Ok(Value::Object(result))
}

pub fn attach_marker_quality(report: &Value, definition: &Value) -> Res<Value> {
    let quality = evaluate_marker_quality(report, definition)?;
    let mut result = report.as_object().cloned().ok_or("Completed STR report required")?;
    let sha = quality["definition_sha256"].clone();
    result.entry("quality").or_insert_with(|| json!({})).as_object_mut().ok_or("TypeError: quality is not a mapping")?.insert("marker_gq".into(), quality);
    result
        .entry("settings")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("TypeError: settings is not a mapping")?
        .insert("marker_quality_definition_sha256".into(), sha);
    Ok(Value::Object(result))
}

#[allow(dead_code)]
fn _unused(_: HashSet<u8>) {}
