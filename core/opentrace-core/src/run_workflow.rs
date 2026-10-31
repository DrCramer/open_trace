//! Port of `genemapper_open/run_workflow.py` (explicit fresh run: analyze ladders, aggregate admitted offsets, call sample).
//!
//! `analyze_str_run_files` (needs `server.analyze`) and the in-process cohort cache are not ported; the
//! cohort is a pure function of its inputs, so results are identical.
use crate::kit_definition::load_kit_definition;
use crate::ladder_quality_workflow::load_ladder_quality_definition;
use crate::pullup_quality::truthy;
use crate::run_offsets::{aggregate_run_offsets, remove_run_ladder, upsert_run_offsets};
use crate::spike::load_spike_definition;
use crate::str_workflow::{analysis, digest, ladder_stages, pullup_provenance, sample_stage, VERSION};
use serde_json::{json, Map, Value};
use std::collections::{BTreeSet, HashSet};

pub type Res<T> = Result<T, String>;

const POLICIES: [&str; 3] = ["reject", "forward_fill", "reference_bins"];

pub(crate) fn ladder_entries<'a>(ladders: &'a Value, field: &str) -> Res<&'a Vec<Value>> {
    let list = match ladders {
        Value::Array(l) if (1..=128).contains(&l.len()) => l,
        _ => return Err("Explicit1..128 run ladders required".into()),
    };
    for entry in list {
        match entry {
            Value::Object(m) if m.len() == 2 && m.contains_key("ladder_id") && m.contains_key(field) => {
                let ok = matches!(&m["ladder_id"], Value::String(s)
                    if (1..=256).contains(&s.chars().count()) && s.is_ascii() && !s.chars().any(|c| (c as u32) < 32 || c as u32 == 127));
                if !ok {
                    return Err("Bounded printable ASCII run ladder_id required".into());
                }
            }
            _ => return Err(format!("Each run ladder requires ladder_id and {}", field)),
        }
    }
    Ok(list)
}

fn ladder_policy(policy: &str) -> &str {
    if policy == "reference_bins" {
        "forward_fill"
    } else {
        policy
    }
}

fn pick(m: &Map<String, Value>, keys: &[&str]) -> Value {
    Value::Object(keys.iter().map(|k| (k.to_string(), m[*k].clone())).collect())
}

const IDENTITY_KEYS: [&str; 4] = ["source_sha256", "analysis_version", "sizing_status", "standard_definition_context"];

fn base_run_report(sample_analysis: &Value, sample: &Map<String, Value>, definition: &Value, quality_definition: &Value, policy: &str) -> Res<Map<String, Value>> {
    let mut sizing = Map::new();
    for key in ["status", "mode", "anchors", "calibration_model", "reference_channel", "base_quality", "post_quality", "expected_bp"] {
        sizing.insert(key.into(), sample_analysis["sizing"].get(key).cloned().unwrap_or(Value::Null));
    }
    let mut r = Map::new();
    r.insert("schema_version".into(), json!(1));
    r.insert("source_sha256".into(), sample["source_sha256"].clone());
    r.insert("analysis_version".into(), json!(format!("OpenTrace/{}", VERSION)));
    r.insert("status".into(), json!("rejected"));
    r.insert("reasons".into(), json!([]));
    r.insert("calls".into(), json!([]));
    r.insert("peaks".into(), json!([]));
    r.insert("allele_calling".into(), Value::Null);
    r.insert("ladder".into(), Value::Null);
    r.insert("research_only".into(), json!(true));
    r.insert("kit_identity".into(), json!({"name": definition["name"], "sha256": digest(definition)?}));
    r.insert("sample_identity".into(), pick(sample, &IDENTITY_KEYS));
    r.insert("ladder_identity".into(), json!({"scope": "explicit_run", "ladders": []}));
    r.insert(
        "settings".into(),
        json!({"analysis": sample["signature"], "missing_offset_policy": policy, "ladder_quality_definition_sha256": digest(quality_definition)?}),
    );
    r.insert("sizing".into(), Value::Object(sizing));
    r.insert("quality".into(), json!({"run_ladders": []}));
    r.insert(
        "provenance".into(),
        json!({"scope": "fresh_explicit_run", "input_reports_are_caller_supplied": true, "sample_settings_sha256": digest(&sample["signature"])?,
               "external_validation": false, "original_runtime_validated": false, "full_original_pqv_applied": false,
               "run_quality_code_source": "fresh_ladder_al_subset", "ladder_quality_applied": true}),
    );
    Ok(r)
}

fn sample_failures(sample: &Map<String, Value>) -> Vec<String> {
    let mut reasons = Vec::new();
    if sample["accepted"] != json!(true) {
        reasons.push("sample: accepted automatic morphology sizing is required".to_string());
    }
    if !sample["missing_channels"].as_array().unwrap().is_empty() {
        reasons.push("sample: kit channels absent from selected data group".to_string());
    }
    if !sample["spike_unavailable"].as_array().unwrap().is_empty() {
        reasons.push("sample: requested spike classification unavailable".to_string());
    }
    if !sample["pullup_unavailable"].as_array().unwrap().is_empty() {
        reasons.push("sample: requested pull-up processing unavailable".to_string());
    }
    reasons
}

fn artifact_options(sample: &Map<String, Value>) -> Res<(Option<Value>, Option<Value>)> {
    let sd = sample["signature"]["peak_settings"].get("spike_definition").filter(|v| !v.is_null()).cloned();
    let adjustments = match &sd {
        Some(d) => load_spike_definition(d)?.get("adjustments").filter(|v| !v.is_null()).cloned(),
        None => None,
    };
    Ok((sd, adjustments))
}

fn marker_quality_codes(quality: &Value) -> Value {
    let gq = quality.get("ladder_gq").filter(|v| truthy(v)).cloned().unwrap_or_else(|| json!({}));
    let sq = gq.get("sq_category").filter(|v| truthy(v)).and_then(|c| c.get("code")).cloned().unwrap_or(Value::Null);
    let mut codes = Map::new();
    if let Some(rows) = gq.get("markers").and_then(|m| m.as_array()) {
        for row in rows {
            if row.get("status") != Some(&json!("evaluated_subset")) {
                continue;
            }
            let cat = row.get("category").filter(|v| truthy(v)).and_then(|c| c.get("code")).cloned().unwrap_or(Value::Null);
            let passed = cat == json!(0) && sq == json!(0);
            codes.insert(row["marker"].as_str().unwrap().to_string(), json!(if passed { 0 } else { 2 }));
        }
    }
    Value::Object(codes)
}

fn ladder_cohort(
    sample: &Map<String, Value>,
    ladders: &[Value],
    definition: &Value,
    quality_definition: &Value,
    policy: &str,
    adjustments: Option<&Value>,
    marker_gate: bool,
) -> Res<Map<String, Value>> {
    let channels: BTreeSet<String> = definition["markers"].as_array().unwrap().iter().map(|m| m["channel"].as_str().unwrap().to_string()).collect();
    let mut identities: Vec<Value> = Vec::new();
    let mut run_ladders: Vec<Value> = Vec::new();
    let mut entries: Vec<Value> = Vec::new();
    let mut state = Value::Null;
    for entry in ladders {
        let identifier = entry["ladder_id"].as_str().unwrap().to_string();
        let analysis_v = &entry["analysis"];
        let mut ladder = analysis(analysis_v, &format!("ladder {}", identifier), &channels)?.as_object().unwrap().clone();
        let identity = pick(&ladder, &IDENTITY_KEYS);
        let mut row = Map::new();
        row.insert("ladder_id".into(), json!(identifier));
        row.extend(identity.as_object().unwrap().clone());
        identities.push(Value::Object(row));
        let mut reasons: Vec<&str> = Vec::new();
        if !crate::pullup_quality::py_eq(&sample["signature"], &ladder["signature"]) {
            reasons.push("Analysis/standard settings differ from sample");
        }
        if sample["analysis_version"] != ladder["analysis_version"] {
            reasons.push("Analysis version differs from sample");
        }
        if ladder["accepted"] != json!(true) {
            reasons.push("Accepted automatic morphology sizing required");
        }
        if !ladder["missing_channels"].as_array().unwrap().is_empty() {
            reasons.push("Kit channels absent from selected data group");
        }
        if !ladder["spike_unavailable"].as_array().unwrap().is_empty() {
            reasons.push("Requested spike classification unavailable");
        }
        let mut stage = Map::new();
        stage.insert("ladder_id".into(), json!(identifier));
        stage.insert("identity".into(), identity);
        stage.insert("status".into(), json!("rejected"));
        stage.insert("reasons".into(), json!(reasons));
        stage.insert("quality_code".into(), Value::Null);
        stage.insert("quality_code_source".into(), json!("fresh_ladder_al_subset"));
        stage.insert("matching".into(), json!({"status": "rejected", "markers": {}, "reasons": reasons}));
        stage.insert("effective".into(), Value::Null);
        stage.insert("quality".into(), json!({}));
        stage.insert("provenance".into(), json!({}));
        let prior = remove_run_ladder(&state, &json!(identifier))?;
        if reasons.is_empty() {
            let st = ladder_stages(analysis_v, &mut ladder, definition, Some(quality_definition), &json!(policy), adjustments, Some(&prior))?;
            let code = st.quality["ladder_gq"]["al_subset_code"].clone();
            stage.insert("status".into(), json!("evaluated_subset"));
            stage.insert("matching".into(), st.matching);
            stage.insert("effective".into(), st.effective);
            stage.insert("quality".into(), Value::Object(st.quality));
            stage.insert("provenance".into(), Value::Object(st.provenance));
            stage.insert("quality_code".into(), code);
        }
        let stage = Value::Object(stage);
        let mut admitted = stage["quality_code"].clone();
        if marker_gate && stage["status"] == json!("evaluated_subset") {
            admitted = marker_quality_codes(&stage["quality"]);
        }
        entries.push(json!({"ladder_id": identifier, "quality_code": admitted, "result": stage["matching"]}));
        state = upsert_run_offsets(&prior, &json!(identifier), &admitted, &stage["matching"])?;
        run_ladders.push(stage);
    }
    let offsets = if !entries.is_empty() {
        aggregate_run_offsets(&Value::Array(entries), definition)?
    } else {
        json!({"status": "rejected", "reasons": ["No ladders remain in this run"], "markers": {},
               "diagnostics": {"method": "empty_explicit_run", "external_validation": false}})
    };
    let mut result = Map::new();
    result.insert("ladder_identity".into(), json!({"scope": "explicit_run", "ladders": identities}));
    result.insert("quality".into(), json!({"run_ladders": run_ladders}));
    result.insert("ladder".into(), offsets);
    let _ = quality_definition;
    Ok(result)
}

fn finish_run(
    result: &mut Map<String, Value>,
    sample: &mut Map<String, Value>,
    definition: &Value,
    cohort: &Map<String, Value>,
    adjustments: Option<&Value>,
    spike_definition: Option<&Value>,
    call_thresholds: Option<&Value>,
) -> Res<()> {
    {
        let prov = result.get_mut("provenance").unwrap().as_object_mut().unwrap();
        if let Some(sd) = spike_definition {
            if sd.get("schema_version").and_then(|v| v.as_f64()).unwrap_or(0.0) >= 3.0 {
                prov.insert("morphology_scope".into(), json!("companion_jag_and_ideal_gaussian_fit_only"));
                prov.insert("true_peak_restoration_applied".into(), json!(false));
                if sd["schema_version"] == json!(4) {
                    let applied = sample["peaks"].as_array().unwrap().iter().any(|p| {
                        p.get("true_peak_restoration").cloned().unwrap_or_else(|| json!({})).get("writes").map_or(false, truthy)
                    });
                    prov.insert("morphology_scope".into(), json!("side_collection_true_peak_and_fresh_restoration"));
                    prov.insert("true_peak_restoration_applied".into(), json!(applied));
                }
            }
        }
    }
    result.insert("ladder_identity".into(), cohort["ladder_identity"].clone());
    result.get_mut("quality").unwrap().as_object_mut().unwrap().insert("run_ladders".into(), cohort["quality"]["run_ladders"].clone());
    let offsets = cohort["ladder"].clone();
    result.insert("ladder".into(), offsets.clone());
    {
        let prov = result.get_mut("provenance").unwrap().as_object_mut().unwrap();
        prov.insert("artifact_detectors_applied".into(), json!(spike_definition.is_some()));
        prov.insert("automatic_pullup_applied".into(), json!(false));
        prov.insert("downstream_spike_adjustments_applied".into(), json!(adjustments.is_some()));
        for (k, v) in pullup_provenance(sample) {
            prov.insert(k, v);
        }
        let prior = prov["artifact_detectors_applied"] == json!(true);
        let routing = prov.get("experimental_pullup_routing_applied") == Some(&json!(true));
        prov.insert("artifact_detectors_applied".into(), json!(prior || routing));
    }
    let inputs = json!({"sample": sample["source_sha256"], "ladders": result["ladder_identity"]["ladders"], "stages": result["quality"]["run_ladders"]});
    let d = digest(&inputs)?;
    result.get_mut("provenance").unwrap().as_object_mut().unwrap().insert("run_inputs_sha256".into(), json!(d));
    sample_stage(result, sample, definition, &offsets, adjustments, call_thresholds)?;
    let obs = json!({"ladder": offsets, "allele_calling": result["allele_calling"], "run_ladders": result["quality"]["run_ladders"]});
    let d = digest(&obs)?;
    result.get_mut("provenance").unwrap().as_object_mut().unwrap().insert("machine_observations_sha256".into(), json!(d));
    Ok(())
}

fn call_thresholds(homozygous: &Value, heterozygous: &Value) -> Res<Value> {
    let mut values = Map::new();
    for (name, value) in [("homozygous", homozygous), ("heterozygous", heterozygous)] {
        match value {
            Value::Number(n) if n.as_f64().map_or(false, |f| f.is_finite() && f >= 0.0) => {
                values.insert(name.into(), json!(n.as_f64().unwrap()));
            }
            _ => return Err(format!("{}_min_height must be a finite RFU value >= 0", name)),
        }
    }
    Ok(Value::Object(values))
}

pub struct RunOptions<'a> {
    pub ladder_quality_definition: &'a Value,
    pub missing_offset_policy: &'a str,
    pub marker_gate: bool,
    pub homozygous_min_height: Value,
    pub heterozygous_min_height: Value,
    /// ID-X never flags pull-up automatically (its detector `ar` is not reachable), so every detected peak is called.
    /// `true` (default) keeps the earlier behaviour: peaks classified as pull-up are excluded from calling.
    pub pullup_excludes_calls: bool,
}

pub fn run_str_run_workflow(sample_analysis: &Value, ladders: &Value, kit: &Value, opts: &RunOptions) -> Res<Value> {
    let definition = load_kit_definition(kit)?;
    let quality_definition = load_ladder_quality_definition(opts.ladder_quality_definition)?;
    let policy = opts.missing_offset_policy;
    if !POLICIES.contains(&policy) {
        return Err("missing_offset_policy must be reject, forward_fill or reference_bins".into());
    }
    let ladder_list: Vec<Value> = if truthy(ladders) || policy != "reference_bins" {
        ladder_entries(ladders, "analysis")?.clone()
    } else if let Value::Array(l) = ladders {
        l.clone()
    } else {
        return Err("Explicit run ladder list required".into());
    };
    let channels: BTreeSet<String> = definition["markers"].as_array().unwrap().iter().map(|m| m["channel"].as_str().unwrap().to_string()).collect();
    let mut sample = analysis(sample_analysis, "sample", &channels)?.as_object().unwrap().clone();
    let thresholds = call_thresholds(&opts.homozygous_min_height, &opts.heterozygous_min_height)?;
    let mut result = base_run_report(sample_analysis, &sample, &definition, &quality_definition, policy)?;
    result.get_mut("settings").unwrap().as_object_mut().unwrap().insert("call_thresholds".into(), thresholds.clone());
    if !opts.pullup_excludes_calls {
        sample.insert("ignore_pullup_for_calls".into(), json!(true));
        result.get_mut("settings").unwrap().as_object_mut().unwrap().insert("pullup_excludes_calls".into(), json!(false));
    }
    let reasons = sample_failures(&sample);
    result.insert("reasons".into(), json!(reasons));
    if !reasons.is_empty() {
        return Ok(Value::Object(result));
    }
    let (spike_definition, adjustments) = artifact_options(&sample)?;
    let cohort = ladder_cohort(&sample, &ladder_list, &definition, &quality_definition, ladder_policy(policy), adjustments.as_ref(), opts.marker_gate)?;
    finish_run(&mut result, &mut sample, &definition, &cohort, adjustments.as_ref(), spike_definition.as_ref(), Some(&thresholds))?;
    Ok(Value::Object(result))
}

pub fn run_str_batch_workflow(samples: &Value, ladders: &Value, kit: &Value, opts: &RunOptions) -> Res<Value> {
    let definition = load_kit_definition(kit)?;
    let quality_definition = load_ladder_quality_definition(opts.ladder_quality_definition)?;
    let policy = opts.missing_offset_policy;
    if !POLICIES.contains(&policy) {
        return Err("Invalid missing offset policy".into());
    }
    let ladder_list: Vec<Value> = match ladders {
        Value::Array(l) => {
            if !l.is_empty() {
                ladder_entries(ladders, "analysis")?;
            }
            l.clone()
        }
        _ => return Err("Explicit run ladder list required".into()),
    };
    let sample_list = match samples {
        Value::Array(s) if s.len() <= 128 => s,
        _ => return Err("Explicit0..128 run samples required".into()),
    };
    if sample_list.is_empty() {
        return Ok(json!([]));
    }
    let mut keys: HashSet<String> = HashSet::new();
    let mut prepared: Vec<Map<String, Value>> = Vec::new();
    let channels: BTreeSet<String> = definition["markers"].as_array().unwrap().iter().map(|m| m["channel"].as_str().unwrap().to_string()).collect();
    for entry in sample_list {
        let e = match entry {
            Value::Object(e) if e.len() == 2 && e.contains_key("sample_key") && e.contains_key("analysis") => e,
            _ => return Err("Run sample needs sample_key/analysis".into()),
        };
        let key = match &e["sample_key"] {
            Value::String(k) if k.trim_matches(crate::profiles::py_isspace_pub).chars().count() > 0 && k.chars().count() <= 256 && !keys.contains(k) => k.clone(),
            _ => return Err("Bounded unique sample_key required".into()),
        };
        keys.insert(key.clone());
        prepared.push(analysis(&e["analysis"], &format!("sample {}", key), &channels)?.as_object().unwrap().clone());
    }
    let reference = prepared[0].clone();
    let (spike_definition, adjustments) = artifact_options(&reference)?;
    let thresholds = call_thresholds(&opts.homozygous_min_height, &opts.heterozygous_min_height)?;
    let cohort = ladder_cohort(&reference, &ladder_list, &definition, &quality_definition, ladder_policy(policy), adjustments.as_ref(), opts.marker_gate)?;
    let mut outputs: Vec<Value> = Vec::new();
    for (entry, mut sample) in sample_list.iter().zip(prepared.into_iter()) {
        let mut result = base_run_report(&entry["analysis"], &sample, &definition, &quality_definition, policy)?;
        let mut reasons: Vec<String> = sample_failures(&sample);
        if !crate::pullup_quality::py_eq(&sample["signature"], &reference["signature"]) {
            reasons.push("Sample analysis/standard settings differ from run context".into());
        }
        if sample["analysis_version"] != reference["analysis_version"] {
            reasons.push("Sample analysis version differs from run context".into());
        }
        result.insert("reasons".into(), json!(reasons));
        result.get_mut("settings").unwrap().as_object_mut().unwrap().insert("call_thresholds".into(), thresholds.clone());
        if reasons.is_empty() {
            finish_run(&mut result, &mut sample, &definition, &cohort, adjustments.as_ref(), spike_definition.as_ref(), Some(&thresholds))?;
        } else {
            result.insert("ladder_identity".into(), cohort["ladder_identity"].clone());
            result.get_mut("quality").unwrap().as_object_mut().unwrap().insert("run_ladders".into(), cohort["quality"]["run_ladders"].clone());
            result.insert("ladder".into(), cohort["ladder"].clone());
            let markers: Vec<Value> = definition["markers"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| json!({"name": m["name"], "channel": m["channel"], "status": "rejected", "reasons": ["Sample prerequisites unavailable"],
                                "calls": [], "stutter_calls": [], "peak_review": []}))
                .collect();
            result.insert(
                "allele_calling".into(),
                json!({"status": "rejected", "reasons": reasons, "markers": markers,
                       "diagnostics": {"method": "skipped_due_to_sample_prerequisites", "calling_executed": false}}),
            );
        }
        let batch_digest = digest(&json!({"samples": samples, "ladders": ladders, "kit": definition, "quality_definition": quality_definition, "missing_offset_policy": policy}))?;
        let prov = result.get_mut("provenance").unwrap().as_object_mut().unwrap();
        prov.insert("batch_sample_key".into(), entry["sample_key"].clone());
        prov.insert("batch_inputs_sha256".into(), json!(batch_digest));
        prov.insert("reanalysis_scope".into(), json!("sized_observations_to_calls_and_requested_quality"));
        outputs.push(json!({"sample_key": entry["sample_key"], "report": Value::Object(result)}));
    }
    Ok(Value::Array(outputs))
}
