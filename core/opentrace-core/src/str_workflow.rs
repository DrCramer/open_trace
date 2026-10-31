//! Port of `genemapper_open/str_workflow.py` (explicit-kit, single-ladder research workflow).
//!
//! `analyze_str_files` (which needs `server.analyze`) is not part of this module.
use crate::allele_calling::call_sample;
use crate::calibration::build_calibration;
use crate::kit_definition::load_kit_definition;
use crate::ladder::match_ladder;
use crate::ladder_calling::ladder_call_observations;
use crate::ladder_observation_binding::bind;
use crate::ladder_quality_workflow::{gate_ladder_offsets, load_ladder_quality_definition};
use crate::pullup::load_pullup_definition;
use crate::pullup_quality::{digest as digest_impl, evaluate_pullup_quality, py_eq, sha256_hex, truthy};
use crate::quality_control::dimensions_in_bp;
use crate::spike::load_spike_definition;
use crate::spike_adjustment::{adjust_ladder_spikes, apply_peak_states};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

pub type Res<T> = Result<T, String>;

pub const VERSION: &str = "0.57.0";

pub fn digest(v: &Value) -> Res<String> {
    digest_impl(v)
}

/// Python `(value or {}).get(key)`.
fn or_empty_get(v: Option<&Value>, key: &str) -> Res<Value> {
    match v {
        Some(x) if truthy(x) => match x {
            Value::Object(m) => Ok(m.get(key).cloned().unwrap_or(Value::Null)),
            _ => Err("AttributeError: object has no attribute 'get'".into()),
        },
        _ => Ok(Value::Null),
    }
}

pub fn compact_analysis(report: &Value) -> Value {
    let mut out = Map::new();
    for key in ["source_sha256", "analysis_version", "peak_settings", "sizing", "standard_definition_context", "pullup_detection"] {
        if let Some(v) = report.get(key) {
            out.insert(key.into(), v.clone());
        }
    }
    let channels: Vec<Value> = report
        .get("channels")
        .and_then(|c| c.as_array())
        .map(|a| {
            a.iter()
                .map(|ch| {
                    let mut m = Map::new();
                    for key in ["tag", "group", "peaks"] {
                        if let Some(v) = ch.get(key) {
                            m.insert(key.into(), v.clone());
                        }
                    }
                    Value::Object(m)
                })
                .collect()
        })
        .unwrap_or_default();
    out.insert("channels".into(), Value::Array(channels));
    Value::Object(out)
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn real(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Number(n)) => n.as_f64(),
        _ => None,
    }
}

/// Python `_analysis`: returns the plain-data analysis summary.
pub fn analysis(report: &Value, name: &str, channels: &BTreeSet<String>) -> Res<Value> {
    let rep = report.as_object().ok_or_else(|| format!("{}: analysis report required", name))?;
    match rep.get("source_sha256") {
        Some(Value::String(s)) if is_hex64(s) => {}
        _ => return Err(format!("{}: input SHA-256 required", name)),
    }
    match rep.get("analysis_version") {
        Some(Value::String(s)) if s.chars().count() <= 128 => {}
        _ => return Err(format!("{}: bounded analysis version required", name)),
    }
    let (sizing, settings) = match (rep.get("sizing"), rep.get("peak_settings")) {
        (Some(Value::Object(a)), Some(Value::Object(b))) => (a, b),
        _ => return Err(format!("{}: sizing and peak_settings required", name)),
    };
    let sg = |k: &str| sizing.get(k).cloned().unwrap_or(Value::Null);
    let mut sizing_sig = Map::new();
    for key in ["mode", "reference_channel", "expected_bp", "analysis_engine", "calibration_model", "max_residual_bp", "settings"] {
        sizing_sig.insert(key.into(), sg(key));
    }
    let mut peak_settings = settings.clone();
    let anchors_len = match sizing.get("anchors") {
        None => 0,
        Some(Value::Array(a)) => a.len(),
        Some(Value::String(s)) => s.chars().count(),
        Some(_) => return Err("TypeError: object of type has no len()".into()),
    };
    let accepted = sg("status") == json!("accepted")
        && sg("mode") == json!("auto")
        && sg("calibration_applied") == json!(true)
        && anchors_len >= 3
        && sg("analysis_engine") == json!("morphology_ratio")
        && settings.get("analysis_engine") == Some(&json!("morphology_ratio"));
    let curve = if accepted {
        let anchors = sizing.get("anchors").ok_or("KeyError: 'anchors'")?;
        let model = sizing.get("calibration_model").ok_or("KeyError: 'calibration_model'")?;
        Some(build_calibration(anchors, Some(model))?)
    } else {
        None
    };
    let rows = match rep.get("channels") {
        Some(Value::Array(r)) if r.len() <= 128 => r,
        _ => return Err(format!("{}: bounded channel list required", name)),
    };
    let selected = match settings.get("selected_data_group") {
        Some(Value::String(s)) if s == "raw" || s == "analyzed" => s.clone(),
        _ => return Err(format!("{}: selected raw/analyzed group required", name)),
    };
    let mut found: BTreeSet<String> = BTreeSet::new();
    let mut peaks: Vec<Value> = Vec::new();
    let mut skipped = 0i64;
    let mut spike_unavailable: Vec<Value> = Vec::new();
    let mut pullup_unavailable: Vec<Value> = Vec::new();
    let mut pullup_definition: Option<Value> = None;
    if let Some(pd) = settings.get("pullup_definition").filter(|v| !v.is_null()) {
        let loaded = load_pullup_definition(pd)?;
        peak_settings.insert("pullup_definition".into(), loaded.clone());
        pullup_definition = Some(loaded);
    }
    let reference = sizing.get("reference_channel");
    if let Some(Value::String(r)) = reference {
        if channels.contains(r) {
            return Err("Kit marker channel must differ from the size-standard channel".into());
        }
    }
    let mut all_tags: BTreeSet<String> = BTreeSet::new();
    for channel in rows {
        let tag = match channel.as_object().and_then(|c| c.get("tag")).and_then(|t| t.as_str()) {
            Some(t) => t.to_string(),
            None => return Err(format!("{}: invalid channel", name)),
        };
        if !all_tags.insert(tag.clone()) {
            return Err(format!("{}: duplicate channel", name));
        }
        if !channels.contains(&tag) || channel.get("group") != Some(&json!(selected)) {
            continue;
        }
        found.insert(tag.clone());
        let candidates = match channel.get("peaks") {
            Some(Value::Array(c)) if c.len() <= 4096 => c,
            _ => return Err(format!("{}: at most4096 peaks per channel", name)),
        };
        for (index, peak) in candidates.iter().enumerate() {
            let p = peak.as_object().ok_or_else(|| format!("{}: peak object required", name))?;
            let restored = or_empty_get(p.get("true_peak_restoration"), "status")? == json!("restored");
            let size = p.get("size_bp").filter(|v| !v.is_null());
            if size.is_none() || (restored && matches!(size, Some(Value::Number(n)) if n.as_f64().map_or(false, |f| f <= 0.0))) {
                skipped += 1;
                continue;
            }
            for key in ["scan", "size_bp", "height"] {
                match p.get(key) {
                    Some(Value::Number(n)) if n.as_f64().map_or(false, f64::is_finite) => {}
                    _ => return Err(format!("{}: finite numeric peak coordinates/height required", name)),
                }
            }
            let (scan, size_bp, height) = (real(p.get("scan")).unwrap(), real(p.get("size_bp")).unwrap(), real(p.get("height")).unwrap());
            if !((0.0..=100_000_000.0).contains(&scan) && 0.0 < size_bp && size_bp <= 1_000_000.0 && 0.0 < height && height <= 1e12) {
                return Err(format!("{}: peak outside supported bounds", name));
            }
            if let Some(pu) = p.get("pullup") {
                if !pu.is_boolean() {
                    return Err(format!("{}: boolean pull-up flag required", name));
                }
            }
            let mut record = Map::new();
            for key in ["scan", "size_bp", "height", "flags", "spike", "pullup"] {
                if let Some(v) = p.get(key) {
                    record.insert(key.into(), v.clone());
                }
            }
            for key in [
                "begin", "end", "fwhm_scan", "area", "area_bp", "offscaled", "spike_features", "spike_classification",
                "true_peak_restoration", "pullup_classification", "pullup_ratio", "pullup_ratio_threshold",
            ] {
                if let Some(v) = p.get(key) {
                    record.insert(key.into(), v.clone());
                }
            }
            if settings.get("spike_definition").map_or(false, |v| !v.is_null())
                && or_empty_get(p.get("spike_classification"), "status")? != json!("evaluated_subset")
            {
                spike_unavailable.push(json!(format!("{}:{}", tag, index)));
            }
            if let Some(pd) = &pullup_definition {
                if !pd["detector"].is_null() {
                    let st = or_empty_get(p.get("pullup_classification"), "status")?;
                    if st != json!("not_targeted") && st != json!("evaluated_explicit_passes") {
                        pullup_unavailable.push(json!(format!("{}:{}", tag, index)));
                    }
                }
            }
            let area_neg = match p.get("area") {
                None => false,
                Some(Value::Number(n)) => n.as_f64().map_or(false, |f| f < 0.0),
                Some(_) => return Err("TypeError: '<' not supported".into()),
            };
            let have_morph = ["begin", "end", "fwhm_scan", "area"].iter().all(|k| p.contains_key(*k));
            if restored && area_neg {
                record.insert(
                    "morphology_bp".into(),
                    json!({"status": "unavailable", "reason": "restored_negative_area_sentinel", "area": p["area"], "area_bp": get_or_null(p, "area_bp")}),
                );
            } else if curve.is_some() && have_morph {
                let probe = json!({"begin_scan": p["begin"], "end_scan": p["end"], "fwhm_scan": p["fwhm_scan"], "area": p["area"]});
                let mut dims = dimensions_in_bp(&probe, curve.as_ref().unwrap())?;
                if restored {
                    let m = dims.as_object_mut().ok_or("morphology must be an object")?;
                    m.insert("area_bp".into(), p.get("area_bp").cloned().ok_or("KeyError: 'area_bp'")?);
                    m.insert("area_source".into(), json!("true_peak_restored_unrounded_map_span"));
                }
                record.insert("morphology_bp".into(), dims);
            } else {
                record.insert("morphology_bp".into(), json!({"status": "unavailable", "reason": "missing_morphology_or_calibration"}));
            }
            record.insert("channel".into(), json!(tag));
            record.insert("id".into(), json!(format!("{}:{}", tag, index)));
            peaks.push(Value::Object(record));
        }
    }
    if peaks.len() > 4096 {
        return Err(format!("{}: at most4096 total sized kit-channel peaks", name));
    }
    let missing: Vec<&String> = channels.iter().filter(|c| !found.contains(*c)).collect();
    Ok(json!({
        "accepted": accepted,
        "signature": {"peak_settings": Value::Object(peak_settings), "sizing": Value::Object(sizing_sig)},
        "peaks": peaks,
        "missing_channels": missing,
        "unsized_peaks": skipped,
        "source_sha256": rep["source_sha256"],
        "spike_unavailable": spike_unavailable,
        "pullup_unavailable": pullup_unavailable,
        "analysis_version": rep["analysis_version"],
        "sizing_status": sg("status"),
        "standard_definition_context": rep.get("standard_definition_context").cloned().unwrap_or(Value::Null),
    }))
}

fn get_or_null(m: &Map<String, Value>, k: &str) -> Value {
    m.get(k).cloned().unwrap_or(Value::Null)
}

/// Python `max(list)` over numeric values (first maximal element wins).
fn py_max(values: &Value) -> Res<Value> {
    let list = values.as_array().ok_or("TypeError: expected_bp is not a list")?;
    let mut best: Option<&Value> = None;
    for v in list {
        let f = v.as_f64().ok_or("TypeError: '>' not supported")?;
        if best.map_or(true, |b| f > b.as_f64().unwrap()) {
            best = Some(v);
        }
    }
    best.cloned().ok_or_else(|| "max() arg is an empty sequence".to_string())
}

pub struct LadderStages {
    pub matching: Value,
    pub effective: Value,
    pub quality: Map<String, Value>,
    pub provenance: Map<String, Value>,
}

/// Python `_ladder_stages`; `ladder` is the `_analysis` summary and its `peaks` are updated in place.
pub fn ladder_stages(
    ladder_analysis: &Value,
    ladder: &mut Map<String, Value>,
    definition: &Value,
    quality_definition: Option<&Value>,
    missing_offset_policy: &Value,
    adjustments: Option<&Value>,
    run_offset_state: Option<&Value>,
) -> Res<LadderStages> {
    let max_bp = py_max(&ladder_analysis["sizing"]["expected_bp"])?;
    let matching = match_ladder(&ladder["peaks"], definition, &max_bp, &json!(true), &json!(0.01), run_offset_state)?;
    let policy = match quality_definition {
        Some(q) if truthy(q) => q["calling_parameters_source"].clone(),
        _ => json!("ladder_matching_calls"),
    };
    let mut observations = if policy == json!("ladder_matching_calls") {
        ladder_call_observations(&ladder["peaks"], definition, &matching)?
    } else {
        let called = call_sample(&ladder["peaks"], definition, &matching, &json!(true), None, missing_offset_policy, None)?;
        bind(&ladder["peaks"], definition, &matching, &called)?
    };
    let mut provenance = Map::new();
    provenance.insert("ladder_call_observation_source".into(), policy.clone());
    provenance.insert("ladder_sample_filters_applied".into(), json!(policy == json!("kit_sample_parameters")));
    let mut quality = Map::new();
    if let Some(adj) = adjustments {
        observations = adjust_ladder_spikes(&ladder["peaks"], &observations, adj)?;
        quality.insert(
            "spike_adjustments".into(),
            json!({"ladder": observations["spike_adjustments"], "ladder_call_observations": observations}),
        );
        let moved = apply_peak_states(&ladder["peaks"], &observations["spike_adjustments"])?;
        ladder.insert("peaks".into(), moved);
        provenance.insert("artifact_processing_scope".into(), json!("initial_spike_and_current_boolean_adjustment_subset"));
        provenance.insert("downstream_spike_adjustments_applied".into(), json!(true));
    }
    quality.insert("ladder_calling".into(), observations.clone());
    let pullup_definition = ladder["signature"]["peak_settings"].get("pullup_definition").filter(|v| !v.is_null()).cloned();
    if let Some(pd) = pullup_definition {
        let probe = json!({"peaks": ladder["peaks"], "allele_calling": observations});
        quality.insert("ladder_pullup".into(), evaluate_pullup_quality(&probe, &pd)?);
    }
    let mut effective = matching.clone();
    if let Some(qd) = quality_definition {
        let (eff, gq) = gate_ladder_offsets(
            &ladder["peaks"],
            definition,
            &matching,
            qd,
            &ladder_analysis["sizing"],
            missing_offset_policy,
            Some(&observations),
        )?;
        effective = eff;
        quality.insert("ladder_gq".into(), gq);
        provenance.insert("ladder_quality_applied".into(), json!(true));
    }
    Ok(LadderStages { matching, effective, quality, provenance })
}

/// Python `_sample_stage`.
pub fn sample_stage(
    result: &mut Map<String, Value>,
    sample: &mut Map<String, Value>,
    definition: &Value,
    offsets: &Value,
    adjustments: Option<&Value>,
    call_thresholds: Option<&Value>,
) -> Res<()> {
    let policy = result["settings"]["missing_offset_policy"].clone();
    let metadata = json!({"sha256": sample["source_sha256"]});
    let called = call_sample(&sample["peaks"], definition, offsets, &json!(true), Some(&metadata), &policy, adjustments)?;
    if adjustments.is_some() {
        let summary = called.get("spike_adjustments").cloned();
        let value = match &summary {
            Some(s) if truthy(s) => s.clone(),
            _ => json!({"status": "unavailable", "reason": "sample_call_observations_rejected"}),
        };
        let q = result.entry("quality").or_insert_with(|| json!({})).as_object_mut().ok_or("TypeError: quality")?;
        let sa = q.entry("spike_adjustments").or_insert_with(|| json!({})).as_object_mut().ok_or("TypeError: spike_adjustments")?;
        sa.insert("sample".into(), value);
        if let Some(s) = &summary {
            if !s.is_null() {
                let moved = apply_peak_states(&sample["peaks"], s)?;
                sample.insert("peaks".into(), moved);
            }
        }
    }
    result.insert("allele_calling".into(), called.clone());
    result.insert("status".into(), called["status"].clone());
    result.insert("reasons".into(), called["reasons"].clone());
    result.insert("peaks".into(), sample["peaks"].clone());
    let mut new_calls: Vec<Value> = Vec::new();
    for marker in called["markers"].as_array().ok_or("markers must be a list")? {
        for call in marker["calls"].as_array().ok_or("calls must be a list")? {
            if let Some(t) = call_thresholds {
                let n = marker["calls"].as_array().unwrap().len();
                let floor = if n == 1 { t["homozygous"].as_f64().unwrap() } else { t["heterozygous"].as_f64().unwrap() };
                if call["height"].as_f64().ok_or("TypeError: height")? < floor {
                    continue;
                }
            }
            let idx = call["peak_index"].as_u64().ok_or("peak_index must be an index")? as usize;
            let source = sample["peaks"].as_array().unwrap().get(idx).ok_or("IndexError: list index out of range")?;
            let ident = sha256_hex(format!("{}\0{}", marker["name"].as_str().unwrap(), source["id"].as_str().unwrap()).as_bytes());
            let mut c = call.as_object().ok_or("call must be an object")?.clone();
            c.insert("id".into(), json!(&ident[..24]));
            c.insert("marker".into(), marker["name"].clone());
            c.insert("channel".into(), marker["channel"].clone());
            c.insert("source_peak_id".into(), source["id"].clone());
            new_calls.push(Value::Object(c));
        }
    }
    result.get_mut("calls").and_then(|c| c.as_array_mut()).ok_or("calls must be a list")?.extend(new_calls);
    let pullup_definition = sample["signature"]["peak_settings"].get("pullup_definition").filter(|v| !v.is_null()).cloned();
    if let Some(pd) = pullup_definition {
        let q = evaluate_pullup_quality(&Value::Object(result.clone()), &pd)?;
        let sha = q["definition_sha256"].clone();
        result.entry("quality").or_insert_with(|| json!({})).as_object_mut().ok_or("TypeError: quality")?.insert("pullup".into(), q);
        result.get_mut("settings").and_then(|s| s.as_object_mut()).ok_or("TypeError: settings")?.insert("pullup_quality_definition_sha256".into(), sha);
    }
    Ok(())
}

pub fn pullup_provenance(sample: &Map<String, Value>) -> Map<String, Value> {
    let mut out = Map::new();
    if let Some(d) = sample["signature"]["peak_settings"].get("pullup_definition").filter(|v| !v.is_null()) {
        let applied = !d["detector"].is_null();
        out.insert("automatic_pullup_applied".into(), json!(applied));
        out.insert("experimental_pullup_routing_applied".into(), json!(applied));
        out.insert("original_automatic_pullup_route_confirmed".into(), json!(false));
    }
    out
}

fn restoration_applied(sample: &Map<String, Value>) -> bool {
    sample["peaks"].as_array().unwrap().iter().any(|p| {
        let t = p.get("true_peak_restoration").cloned().unwrap_or_else(|| json!({}));
        t.get("writes").map_or(false, truthy)
    })
}

fn update(target: &mut Map<String, Value>, src: Map<String, Value>) {
    for (k, v) in src {
        target.insert(k, v);
    }
}

pub fn run_str_workflow(
    sample_analysis: &Value,
    ladder_analysis: &Value,
    kit: &Value,
    missing_offset_policy: &str,
    ladder_quality_definition: Option<&Value>,
) -> Res<Value> {
    let definition = load_kit_definition(kit)?;
    if missing_offset_policy != "reject" && missing_offset_policy != "forward_fill" {
        return Err("missing_offset_policy must be reject or forward_fill".into());
    }
    let channels: BTreeSet<String> = definition["markers"].as_array().unwrap().iter().map(|m| m["channel"].as_str().unwrap().to_string()).collect();
    let mut sample = analysis(sample_analysis, "sample", &channels)?.as_object().unwrap().clone();
    let mut ladder = analysis(ladder_analysis, "ladder", &channels)?.as_object().unwrap().clone();
    let mut settings = Map::new();
    settings.insert("analysis".into(), sample["signature"].clone());
    settings.insert("missing_offset_policy".into(), json!(missing_offset_policy));
    let quality_definition = match ladder_quality_definition {
        Some(q) if !q.is_null() => {
            let loaded = load_ladder_quality_definition(q)?;
            settings.insert("ladder_quality_definition_sha256".into(), json!(digest(&loaded)?));
            Some(loaded)
        }
        _ => None,
    };
    let pick = |m: &Map<String, Value>, keys: &[&str]| -> Value {
        Value::Object(keys.iter().map(|k| (k.to_string(), m[*k].clone())).collect())
    };
    let identity_keys = ["source_sha256", "analysis_version", "sizing_status", "standard_definition_context"];
    let mut sizing_out = Map::new();
    for key in ["status", "mode", "anchors", "calibration_model", "reference_channel", "base_quality", "post_quality", "expected_bp"] {
        sizing_out.insert(key.into(), sample_analysis["sizing"].get(key).cloned().unwrap_or(Value::Null));
    }
    let mut result = Map::new();
    result.insert("schema_version".into(), json!(1));
    result.insert("source_sha256".into(), sample["source_sha256"].clone());
    result.insert("analysis_version".into(), json!(format!("OpenTrace/{}", VERSION)));
    result.insert("status".into(), json!("rejected"));
    result.insert("reasons".into(), json!([]));
    result.insert("settings".into(), Value::Object(settings));
    result.insert("kit_identity".into(), json!({"name": definition["name"], "sha256": digest(&definition)?}));
    result.insert("sizing".into(), Value::Object(sizing_out));
    result.insert("sample_identity".into(), pick(&sample, &identity_keys));
    result.insert("ladder_identity".into(), pick(&ladder, &identity_keys));
    result.insert("ladder".into(), Value::Null);
    result.insert("allele_calling".into(), Value::Null);
    result.insert("calls".into(), json!([]));
    result.insert("peaks".into(), json!([]));
    result.insert("research_only".into(), json!(true));
    let mut prov = Map::new();
    prov.insert("sample_settings_sha256".into(), json!(digest(&sample["signature"])?));
    prov.insert("ladder_settings_sha256".into(), json!(digest(&ladder["signature"])?));
    prov.insert("external_validation".into(), json!(false));
    prov.insert("original_runtime_validated".into(), json!(false));
    prov.insert("input_reports_are_caller_supplied".into(), json!(true));
    prov.insert("artifact_detectors_applied".into(), json!(false));
    prov.insert("full_original_pqv_applied".into(), json!(false));
    prov.insert("ladder_quality_applied".into(), json!(false));
    result.insert("provenance".into(), Value::Object(prov));
    let mut reasons: Vec<String> = Vec::new();
    if !py_eq(&sample["signature"], &ladder["signature"]) {
        reasons.push("Sample and ladder require identical explicit analysis/standard settings".into());
    }
    if sample["analysis_version"] != ladder["analysis_version"] {
        reasons.push("Sample and ladder require the same analysis version".into());
    }
    for (nm, item) in [("sample", &sample), ("ladder", &ladder)] {
        if item["accepted"] != json!(true) {
            reasons.push(format!("{}: accepted automatic morphology sizing is required", nm));
        }
        let missing = item["missing_channels"].as_array().unwrap();
        if !missing.is_empty() {
            let names: Vec<&str> = missing.iter().map(|m| m.as_str().unwrap()).collect();
            reasons.push(format!("{}: kit channels absent from selected data group: {}", nm, names.join(", ")));
        }
        if !item["spike_unavailable"].as_array().unwrap().is_empty() {
            reasons.push(format!("{}: requested initial spike classification unavailable for sized kit-channel peaks", nm));
        }
        if !item["pullup_unavailable"].as_array().unwrap().is_empty() {
            reasons.push(format!("{}: requested explicit pull-up processing unavailable for sized kit-channel peaks", nm));
        }
    }
    if !reasons.is_empty() {
        result.insert("reasons".into(), json!(reasons));
        return Ok(Value::Object(result));
    }
    let spike_raw = sample["signature"]["peak_settings"].get("spike_definition").filter(|v| !v.is_null()).cloned();
    {
        let prov = result.get_mut("provenance").unwrap().as_object_mut().unwrap();
        prov.insert("artifact_detectors_applied".into(), json!(spike_raw.is_some()));
        if let Some(sd) = &spike_raw {
            prov.insert("artifact_processing_scope".into(), json!("initial_non_reference_spike_subset_without_downstream_adjustments"));
            let version = sd.get("schema_version").and_then(|v| v.as_f64()).unwrap_or(0.0);
            if version >= 3.0 {
                prov.insert("morphology_scope".into(), json!("companion_jag_and_ideal_gaussian_fit_only"));
                prov.insert("true_peak_restoration_applied".into(), json!(false));
                if sd["schema_version"] == json!(4) {
                    prov.insert("morphology_scope".into(), json!("side_collection_true_peak_and_fresh_restoration"));
                    prov.insert("true_peak_restoration_applied".into(), json!(restoration_applied(&sample)));
                }
            }
            prov.insert("automatic_pullup_applied".into(), json!(false));
        }
        update(prov, pullup_provenance(&sample));
        let prior = prov["artifact_detectors_applied"] == json!(true);
        let routing = prov.get("experimental_pullup_routing_applied") == Some(&json!(true));
        prov.insert("artifact_detectors_applied".into(), json!(prior || routing));
    }
    let mut adjustments: Option<Value> = None;
    if let Some(sd) = &spike_raw {
        let loaded = load_spike_definition(sd)?;
        adjustments = loaded.get("adjustments").filter(|v| !v.is_null()).cloned();
    }
    let stages = ladder_stages(
        ladder_analysis,
        &mut ladder,
        &definition,
        quality_definition.as_ref(),
        &json!(missing_offset_policy),
        adjustments.as_ref(),
        None,
    )?;
    result.insert("quality".into(), Value::Object(stages.quality.clone()));
    update(result.get_mut("provenance").unwrap().as_object_mut().unwrap(), stages.provenance);
    result.insert("ladder".into(), stages.effective.clone());
    let ladder_observations = stages.quality["ladder_calling"].clone();
    sample_stage(&mut result, &mut sample, &definition, &stages.effective, adjustments.as_ref(), None)?;
    let called = result["allele_calling"].clone();
    let mut observations = Map::new();
    observations.insert("ladder".into(), stages.effective);
    observations.insert("allele_calling".into(), called);
    if !ladder_observations.is_null() {
        observations.insert("ladder_calling".into(), ladder_observations);
    }
    let d = digest(&Value::Object(observations))?;
    result.get_mut("provenance").unwrap().as_object_mut().unwrap().insert("machine_observations_sha256".into(), json!(d));
    Ok(Value::Object(result))
}
