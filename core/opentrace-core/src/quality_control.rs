//! Port of `genemapper_open/quality_control.py`: explicit independent QC helpers (pure functions).
use crate::calibration::Calibration;
use serde_json::{json, Map, Value};

pub type Res<T> = Result<T, String>;

fn fv(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

/// Python `_number(value, name, low, high)`: finite real, not bool, within [low, high]; returns float.
pub fn number(value: &Value, name: &str, low: f64, high: f64) -> Res<f64> {
    match value {
        Value::Number(n) => match n.as_f64() {
            Some(x) if low <= x && x <= high => Ok(x),
            _ => Err(format!("{}: finite bounded number required", name)),
        },
        _ => Err(format!("{}: finite bounded number required", name)),
    }
}

/// Python `_bool`: exact bool type.
pub fn boolean(value: &Value, name: &str) -> Res<bool> {
    match value {
        Value::Bool(b) => Ok(*b),
        _ => Err(format!("{}: explicit boolean required", name)),
    }
}

/// Python `_metrics`: at most 128 entries, each None or a bounded metric in [-3, 1e6].
fn metrics(values: &Value) -> Res<Vec<Option<f64>>> {
    let items = match values {
        Value::Array(a) if a.len() <= 128 => a,
        _ => return Err("At most128 explicit marker values required".into()),
    };
    items
        .iter()
        .map(|v| if v.is_null() { Ok(None) } else { number(v, "metric", -3.0, 1e6).map(Some) })
        .collect()
}

/// Python `quality_category`: HID boundary rule (good equality passes, bad equality checks).
pub fn quality_category(score: &Value, good: &Value, bad: &Value) -> Res<Value> {
    let good = number(good, "good", 0.0, 1.0)?;
    let bad = number(bad, "bad", 0.0, 1.0)?;
    if bad > good {
        return Err("bad must not exceed good".into());
    }
    let score = number(score, "score", 0.0, 1.0)?;
    let code: i64 = if score >= good { 0 } else if score < bad { 2 } else { 1 };
    let state = ["pass", "check", "fail"][code as usize];
    Ok(json!({"code": code, "state": state, "score": fv(score), "good": fv(good), "bad": fv(bad)}))
}

/// Python `profile_pullup_category`: every marker stays in the denominator, including missing metrics.
pub fn profile_pullup_category(marker_values: &Value, threshold: &Value) -> Res<Value> {
    let values = metrics(marker_values)?;
    let threshold = number(threshold, "threshold", 0.0, 1.0)?;
    let available = values.iter().any(|v| matches!(v, Some(x) if *x >= 0.0));
    let count = values.iter().filter(|v| matches!(v, Some(x) if *x > 0.0)).count();
    let ratio = if values.is_empty() { None } else { Some(count as f64 / values.len() as f64) };
    let code: i64 = if !available {
        -2
    } else if ratio.unwrap() > threshold {
        2
    } else if count > 0 {
        1
    } else {
        0
    };
    Ok(json!({
        "code": code,
        "denominator": values.len(),
        "positive_markers": count,
        "fraction": ratio.map(fv).unwrap_or(Value::Null),
        "available": available,
        "threshold": fv(threshold),
        "scope": "profile_pullup_only",
    }))
}

/// Python `composite_control_category`.
pub fn composite_control_category(values: &Value) -> Res<Value> {
    let values = metrics(values)?;
    let available = values.iter().any(|v| matches!(v, Some(x) if *x == 0.0 || *x == 1.0));
    let has_one = values.iter().any(|v| matches!(v, Some(x) if *x == 1.0));
    let failed = values.iter().filter(|v| matches!(v, Some(x) if *x == 1.0)).count();
    let code: i64 = if has_one { 2 } else if available { 0 } else { -2 };
    Ok(json!({"code": code, "available": available, "failed_markers": failed}))
}

/// Python `_labels`: None or a bounded list of ASCII labels (no controls, 1..128 characters).
pub fn labels(value: &Value, name: &str) -> Res<Option<Vec<String>>> {
    if value.is_null() {
        return Ok(None);
    }
    let items = match value {
        Value::Array(a) if a.len() <= 128 => a,
        _ => return Err(format!("{}: bounded explicit label list or None required", name)),
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let ok = match item {
            Value::String(s) => {
                let n = s.chars().count();
                (1..=128).contains(&n) && s.is_ascii() && s.chars().all(|c| (c as u32) >= 32 && c as u32 != 127)
            }
            _ => false,
        };
        if !ok {
            return Err(format!(
                "{}: bounded ASCII allele labels required; no Unicode equality parity claimed",
                name
            ));
        }
        out.push(item.as_str().unwrap().to_string());
    }
    Ok(Some(out))
}

fn py_truthy_list(v: &Option<Vec<String>>) -> bool {
    matches!(v, Some(l) if !l.is_empty())
}

/// Python `positive_control`: membership/count rule including the duplicate asymmetry.
pub fn positive_control(called: &Value, expected: &Value, is_y_marker: &Value) -> Res<Value> {
    let called = labels(called, "called")?;
    let expected = labels(expected, "expected")?;
    let is_y = boolean(is_y_marker, "is_y_marker")?;
    let called = called.unwrap_or_default();
    let no_call_non_y = called.is_empty() && !is_y;
    let performed = py_truthy_list(&Some(expected.clone().unwrap_or_default()));
    let mismatch = performed && {
        let expected = expected.as_ref().unwrap();
        called.len() != expected.len()
            || called
                .iter()
                .any(|c| !expected.iter().any(|e| c.to_ascii_lowercase() == e.to_ascii_lowercase()))
    };
    let reason = if mismatch {
        "control_mismatch"
    } else if !performed {
        "expected_profile_missing_or_empty"
    } else {
        "labels_match"
    };
    Ok(json!({
        "cc": if mismatch { 1 } else { 0 },
        "comparison_performed": performed,
        "gq_forced_zero": no_call_non_y || mismatch,
        "reason": reason,
        "comparison": "equal_count_and_case_insensitive_membership_no_consumption",
        "external_validation": false,
    }))
}

/// Python `zero_peak_metric`: pre-cutoff candidate count and post-binning labels are separate inputs.
pub fn zero_peak_metric(candidate_count: &Value, called: &Value, quality_sensor: &Value, expected: &Value) -> Res<Value> {
    let count = match candidate_count {
        Value::Number(n) if n.is_i64() => n.as_i64().unwrap(),
        _ => return Err("candidate_count must be an integer in [0,4096]".into()),
    };
    if !(0..=4096).contains(&count) {
        return Err("candidate_count must be an integer in [0,4096]".into());
    }
    let called = labels(called, "called")?;
    let expected = labels(expected, "expected")?;
    let sensor = boolean(quality_sensor, "quality_sensor")?;
    let expected_nonempty = py_truthy_list(&expected);
    let mut metric: i64 = if count == 0 && !(sensor && expected_nonempty) { 1 } else { 0 };
    let sensor_match = match (&called, &expected) {
        (Some(c), Some(e)) if sensor && !c.is_empty() && !e.is_empty() => {
            let mut cs = c.clone();
            let mut es = e.clone();
            cs.sort();
            es.sort();
            cs == es
        }
        _ => false,
    };
    if sensor_match {
        metric = 1;
    }
    Ok(json!({
        "value": metric,
        "quality_sensor_expected_match": sensor_match,
        "candidate_count": count,
        "case_sensitive_sensor_comparison": true,
    }))
}

/// Python `negative_control`.
pub fn negative_control(zero_peak_value: &Value) -> Res<Value> {
    let value = number(zero_peak_value, "QC_ZERO_PEAK", -3.0, 1e6)?;
    Ok(json!({
        "cc": if value == 0.0 { 1 } else { 0 },
        "gq_forced_zero": value == 0.0,
        "zero_peak_value": fv(value),
        "scope": "does_not_recount_calls_or_apply_spike_weight",
    }))
}

/// Python `round(x, 5)` for bounded non-negative values: correctly rounded decimal, then the nearest double.
pub fn round5(x: f64) -> f64 {
    format!("{:.5}", x).parse::<f64>().unwrap()
}

/// Python `dimensions_in_bp`: widths from rounded endpoint calibration over the scan span.
pub fn dimensions_in_bp(peak: &Value, calibration: &Calibration) -> Res<Value> {
    let map: &Map<String, Value> = peak.as_object().ok_or("Peak object required")?;
    let begin = number(map.get("begin_scan").unwrap_or(&Value::Null), "begin_scan", 0.0, 1e8)?;
    let end = number(map.get("end_scan").unwrap_or(&Value::Null), "end_scan", 0.0, 1e8)?;
    if end <= begin {
        return Err("Positive scan span required".into());
    }
    let left = calibration.size(&fv(begin))?;
    let right = calibration.size(&fv(end))?;
    let (left, right) = match (left, right) {
        (Some(l), Some(r)) => (l, r),
        _ => {
            return Ok(json!({
                "status": "unavailable",
                "reason": "peak_bounds_outside_calibration",
                "fwhm_bp": null,
                "area_bp": null,
            }))
        }
    };
    let left = round5(number(&fv(left), "begin_bp", 0.0, 1e6)?);
    let right = round5(number(&fv(right), "end_bp", 0.0, 1e6)?);
    let scale = (right - left) / (end - begin);
    if scale <= 0.0 {
        return Err("Positive calibrated endpoint span required".into());
    }
    let fwhm = number(map.get("fwhm_scan").unwrap_or(&Value::Null), "fwhm_scan", 0.0, 1e8)?;
    let area = number(map.get("area").unwrap_or(&Value::Null), "area", 0.0, 1e20)?;
    Ok(json!({
        "status": "evaluated",
        "begin_bp": fv(left),
        "end_bp": fv(right),
        "bp_per_scan_span": fv(scale),
        "fwhm_bp": fv(scale * fwhm),
        "area_bp": fv(scale * area),
        "original_rounding_parity": false,
    }))
}

/// Python `broad_standard_penalty`: penalty when a size-matched reference peak is too wide.
pub fn broad_standard_penalty(
    score: &Value,
    peaks: &Value,
    reference_channel: &Value,
    warning_width_bp: &Value,
    weight: &Value,
    is_hid: &Value,
    sq_overridden: &Value,
) -> Res<Value> {
    let score = number(score, "score", 0.0, 1.0)?;
    let threshold = number(warning_width_bp, "warning_width_bp", 0.0, 1e6)?;
    let weight = number(weight, "weight", 0.0, 1.0)?;
    let is_hid_b = boolean(is_hid, "is_hid")?;
    let sq_b = boolean(sq_overridden, "sq_overridden")?;
    let channel = match reference_channel {
        Value::String(s) if !s.is_empty() => s.clone(),
        _ => return Err("Explicit reference channel required".into()),
    };
    let items = match peaks {
        Value::Array(a) if a.len() <= 4096 => a,
        _ => return Err("At most4096 peaks required".into()),
    };
    let mut result = Map::new();
    result.insert("score".into(), fv(score));
    result.insert("broad_metric".into(), Value::Null);
    result.insert("applied".into(), json!(false));
    result.insert("trigger_index".into(), Value::Null);
    result.insert("reason".into(), Value::Null);
    result.insert("calibration_rejection_requested".into(), json!(false));
    if !is_hid_b || sq_b {
        result.insert("reason".into(), json!(if !is_hid_b { "not_hid" } else { "sq_overridden" }));
        return Ok(Value::Object(result));
    }
    result.insert("broad_metric".into(), json!(0));
    for (index, peak) in items.iter().enumerate() {
        let map = peak.as_object().ok_or("Peak object required")?;
        let same_channel = map.get("channel") == Some(&json!(channel));
        if !same_channel || map.get("size_matched") != Some(&json!(true)) {
            continue;
        }
        let width = number(map.get("fwhm_bp").unwrap_or(&Value::Null), "matched standard FWHM bp", 0.0, 1e6)?;
        if width > threshold {
            result.insert("score".into(), fv((1.0 - weight) * score));
            result.insert("broad_metric".into(), json!(1));
            result.insert("applied".into(), json!(true));
            result.insert("trigger_index".into(), json!(index));
            break;
        }
    }
    Ok(Value::Object(result))
}
