//! Port of `genemapper_open/post_sizing_quality.py` (parsed-mapping definition path only).
use crate::calibration::Calibration;
use crate::kit_definition::py_json_dumps_ascii;
use crate::pullup_quality::digest;
use crate::quality_control::{broad_standard_penalty, number, quality_category};
use serde_json::{json, Map, Value};

pub type Res<T> = Result<T, String>;

fn fv(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

pub fn load_sizing_quality_definition(source: &Value) -> Res<Value> {
    if py_json_dumps_ascii(source).len() > 16384 {
        return Err("Sizing quality definition exceeds16KiB".into());
    }
    let map = source.as_object();
    let mut keys: Vec<&str> = map.map(|m| m.keys().map(|k| k.as_str()).collect()).unwrap_or_default();
    keys.sort();
    let mut expected = vec!["schema_version", "context", "width_conversion", "warning_width_bp", "broad_weight", "thresholds"];
    expected.sort();
    let map = match map {
        Some(m) if keys == expected => m,
        _ => return Err("All explicit post-sizing quality fields required".into()),
    };
    match map.get("schema_version") {
        Some(Value::Number(n)) if n.as_i64() == Some(1) => {}
        _ => return Err("Sizing quality schema_version must be1".into()),
    }
    if map["context"] != json!("fresh_hid_no_override") {
        return Err("Only explicit fresh_hid_no_override context supported".into());
    }
    if map["width_conversion"] != json!("unrounded_standard_map_span") {
        return Err("Explicit unrounded_standard_map_span policy required".into());
    }
    number(&map["warning_width_bp"], "warning_width_bp", 0.0, 1e6)?;
    number(&map["broad_weight"], "broad_weight", 0.0, 1.0)?;
    let th = match &map["thresholds"] {
        Value::Object(t) if t.len() == 2 && t.contains_key("good") && t.contains_key("bad") => t,
        _ => return Err("Explicit good/bad SQ thresholds required".into()),
    };
    quality_category(&json!(0.5), &th["good"], &th["bad"])?;
    Ok(source.clone())
}

fn pyf(v: &Value) -> f64 {
    v.as_f64().unwrap_or(f64::NAN)
}

pub fn standard_dimensions(peak: &Map<String, Value>, calibration: &Calibration) -> Res<Map<String, Value>> {
    let g = |k: &str| peak.get(k).cloned().unwrap_or(Value::Null);
    let begin = number(&g("begin"), "begin", 0.0, 1e8)?;
    let end = number(&g("end"), "end", 0.0, 1e8)?;
    let width = number(&g("fwhm_scan"), "fwhm_scan", 0.0, 1e8)?;
    let (lo, hi) = (begin as i64, end as i64);
    let mut r = Map::new();
    r.insert("status".into(), json!("unavailable"));
    r.insert("reason".into(), Value::Null);
    r.insert("begin_scan".into(), json!(lo));
    r.insert("end_scan".into(), json!(hi));
    r.insert("fwhm_scan".into(), fv(width));
    for k in ["begin_bp", "end_bp", "bp_per_scan_span", "fwhm_bp"] {
        r.insert(k.into(), Value::Null);
    }
    r.insert("position_rounding_applied".into(), json!(false));
    r.insert("outside_anchor_bounds".into(), Value::Null);
    if hi <= lo {
        r.insert("reason".into(), json!("nonpositive_integer_peak_span"));
        return Ok(r);
    }
    let anchors = calibration.anchors();
    let a = anchors.as_array().unwrap();
    let first = pyf(&a[0][0]);
    let last = pyf(&a[a.len() - 1][0]);
    r.insert("outside_anchor_bounds".into(), json!((lo as f64) < first || (hi as f64) > last));
    let left = calibration.standard_map_size(&json!(lo))?;
    let right = calibration.standard_map_size(&json!(hi))?;
    r.insert("begin_bp".into(), fv(left));
    r.insert("end_bp".into(), fv(right));
    if left < 0.0 || right < 0.0 {
        r.insert("reason".into(), json!("negative_or_sentinel_standard_map_endpoint"));
        return Ok(r);
    }
    let scale = (right - left) / ((hi - lo) as f64);
    if scale <= 0.0 {
        r.insert("reason".into(), json!("nonpositive_standard_map_span"));
        return Ok(r);
    }
    let value = scale * width;
    number(&fv(value), "calibrated standard FWHM", 0.0, 1e6)?;
    r.insert("status".into(), json!("evaluated"));
    r.insert("bp_per_scan_span".into(), fv(scale));
    r.insert("fwhm_bp".into(), fv(value));
    Ok(r)
}

pub fn evaluate_post_sizing_quality(
    base: &Value,
    selected_peaks: &Value,
    calibration: Option<&Calibration>,
    definition: &Value,
    reference_channel: &Value,
) -> Res<Value> {
    let definition = load_sizing_quality_definition(definition)?;
    let mut result = Map::new();
    result.insert("schema_version".into(), json!(1));
    result.insert("status".into(), json!("unavailable"));
    result.insert("reason".into(), Value::Null);
    result.insert("definition_sha256".into(), json!(digest(&definition)?));
    for k in ["base_score", "score", "broad_metric", "category", "penalty"] {
        result.insert(k.into(), Value::Null);
    }
    result.insert("measurements".into(), json!([]));
    result.insert("context".into(), definition["context"].clone());
    result.insert("width_conversion".into(), definition["width_conversion"].clone());
    result.insert("scope".into(), json!("independent_base_score_plus_matched_standard_broad_penalty"));
    for k in ["full_original_sq", "calibration_rejection_requested", "external_validation", "original_runtime_validated"] {
        result.insert(k.into(), json!(false));
    }
    let unavailable = |mut r: Map<String, Value>, why: &str| -> Res<Value> {
        r.insert("reason".into(), json!(why));
        Ok(Value::Object(r))
    };
    match base {
        Value::Object(b) if b.get("status") == Some(&json!("evaluated")) => {
            let s = number(b.get("score").unwrap_or(&Value::Null), "base sizing score", 0.0, 1.0)?;
            result.insert("base_score".into(), fv(s));
        }
        _ => return unavailable(result, "base_sizing_quality_unavailable"),
    }
    let calibration = match calibration {
        Some(c) => c,
        None => return unavailable(result, "accepted_calibration_unavailable"),
    };
    let model = calibration.model();
    result.insert("calibration_model".into(), json!(model));
    let rule = if model == "lsq2" || model == "lsq3" {
        "fitted_polynomial"
    } else if model == "linear" {
        "independent_piecewise_linear_secant"
    } else {
        "endpoint_pair_secant_negative_sentinel"
    };
    result.insert("exterior_rule".into(), json!(rule));
    let peaks = match selected_peaks {
        Value::Array(p) if p.len() <= 4096 => p,
        _ => return Err("At most4096 selected standard peaks required".into()),
    };
    let mut measurements: Vec<Value> = Vec::new();
    let mut matched: Vec<Value> = Vec::new();
    for (index, peak) in peaks.iter().enumerate() {
        let pm = peak.as_object().ok_or("Selected standard peak object required")?;
        if pm.get("matched_bp").map_or(true, |v| v.is_null()) {
            continue;
        }
        let mut m = standard_dimensions(pm, calibration)?;
        m.insert("selected_peak_index".into(), json!(index));
        m.insert("scan".into(), pm.get("scan").cloned().ok_or("KeyError: 'scan'")?);
        m.insert("matched_bp".into(), pm["matched_bp"].clone());
        if m["status"] == json!("evaluated") {
            matched.push(json!({"channel": reference_channel, "size_matched": true, "fwhm_bp": m["fwhm_bp"]}));
        }
        measurements.push(Value::Object(m));
    }
    let count = measurements.len();
    result.insert("measurements".into(), Value::Array(measurements));
    if count == 0 {
        return unavailable(result, "matched_standard_measurements_missing");
    }
    if matched.len() != count {
        return unavailable(result, "matched_standard_dimensions_unavailable");
    }
    let penalty = broad_standard_penalty(
        &result["base_score"].clone(),
        &Value::Array(matched),
        reference_channel,
        &definition["warning_width_bp"],
        &definition["broad_weight"],
        &json!(true),
        &json!(false),
    )?;
    let category = quality_category(&penalty["score"], &definition["thresholds"]["good"], &definition["thresholds"]["bad"])?;
    result.insert("status".into(), json!("evaluated"));
    result.insert("score".into(), penalty["score"].clone());
    result.insert("broad_metric".into(), penalty["broad_metric"].clone());
    result.insert("penalty".into(), penalty);
    result.insert("category".into(), category);
    Ok(Value::Object(result))
}
