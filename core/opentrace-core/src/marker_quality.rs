//! Port of `genemapper_open/marker_quality.py`.
use crate::fsum::fsum;
use crate::quality_control::{boolean, number};
use serde_json::{json, Map, Value};

pub type Res<T> = Result<T, String>;

const WEIGHTS: [&str; 9] =
    ["allele_number", "absolute_height", "high_height", "spike", "ratio", "broad", "offscale", "overlap", "binning"];
const THRESHOLDS: [&str; 6] = ["max_alleles", "homozygous_height", "heterozygous_height", "high_height", "ratio", "width_bp"];

fn fv(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

fn ascii_label(v: &Value) -> Option<&str> {
    match v {
        Value::String(s) if (1..=128).contains(&s.chars().count()) && s.is_ascii() => Some(s),
        _ => None,
    }
}

struct Call {
    label: String,
    height: f64,
    fwhm: f64,
    offscale: bool,
    confidence: f64,
}

#[allow(clippy::too_many_arguments)]
fn score_marker(
    calls: &Value,
    thresholds: &Value,
    weights: &Value,
    flags: &Value,
    sample_sq: &Value,
    marker_kind: &Value,
    control_mismatch: &Value,
    amel_calls: &Value,
) -> Res<Value> {
    let kind = match marker_kind.as_str() {
        Some(k @ ("autosomal" | "amel" | "y")) => k,
        _ => return Err("Explicit autosomal, amel or y marker kind required".into()),
    };
    let mut amel: Option<Vec<String>> = None;
    if !amel_calls.is_null() {
        match amel_calls {
            Value::Array(a) if a.len() <= 128 && a.iter().all(|l| ascii_label(l).is_some()) => {
                amel = Some(a.iter().map(|l| l.as_str().unwrap().to_string()).collect());
            }
            _ => return Err("Explicit bounded ASCII AMEL calls or None required".into()),
        }
    }
    let mismatch = boolean(control_mismatch, "control_mismatch")?;
    let sq = number(sample_sq, "sample_sq", 0.0, 1.0)?;
    let wm = match weights {
        Value::Object(m) if m.len() == WEIGHTS.len() && WEIGHTS.iter().all(|k| m.contains_key(*k)) => m,
        _ => return Err("All nine explicit quality weights required".into()),
    };
    let mut w: Vec<(&str, f64)> = Vec::new();
    for k in WEIGHTS {
        w.push((k, number(&wm[k], k, 0.0, 1.0)?));
    }
    let weight = |k: &str| w.iter().find(|(n, _)| *n == k).unwrap().1;
    let tm = match thresholds {
        Value::Object(m) if m.len() == THRESHOLDS.len() && THRESHOLDS.iter().all(|k| m.contains_key(*k)) => m,
        _ => return Err("All six explicit thresholds required".into()),
    };
    let maximum = match &tm["max_alleles"] {
        Value::Number(n) if n.as_i64().map_or(false, |v| (1..=128).contains(&v)) => n.as_i64().unwrap(),
        _ => return Err("max_alleles must be1..128".into()),
    };
    let mut th_out = Map::new();
    let mut th: Vec<(String, f64)> = Vec::new();
    for (k, v) in tm {
        let x = number(v, k, 0.0, 1e12)?;
        th.push((k.clone(), x));
        th_out.insert(k.clone(), fv(x));
    }
    let threshold = |k: &str| th.iter().find(|(n, _)| n == k).unwrap().1;
    if threshold("ratio") > 1.0 {
        return Err("ratio threshold must not exceed1".into());
    }
    let fm = match flags {
        Value::Object(m) if m.len() == 4 && ["spike", "offscale", "overlap", "binning"].iter().all(|k| m.contains_key(*k)) => m,
        _ => return Err("Explicit spike/offscale/overlap/binning flags required".into()),
    };
    let mut fl: Vec<(String, bool)> = Vec::new();
    for (k, v) in fm {
        fl.push((k.clone(), boolean(v, k)?));
    }
    let flag = |k: &str| fl.iter().find(|(n, _)| n == k).unwrap().1;
    let list = match calls {
        Value::Array(c) if c.len() <= 4096 => c,
        _ => return Err("At most4096 explicit calls required".into()),
    };
    let null = Value::Null;
    let mut prepared: Vec<Call> = Vec::new();
    for call in list {
        let cm = call.as_object().ok_or("Call object required")?;
        let label = ascii_label(cm.get("label").unwrap_or(&null)).ok_or("Bounded ASCII label required")?.to_string();
        prepared.push(Call {
            label,
            height: number(cm.get("height").unwrap_or(&null), "height", 0.0, 1e12)?,
            fwhm: number(cm.get("fwhm_bp").unwrap_or(&null), "fwhm_bp", 0.0, 1e6)?,
            offscale: boolean(cm.get("offscale").unwrap_or(&null), "call.offscale")?,
            confidence: number(cm.get("confidence").unwrap_or(&null), "initial confidence", 0.0, 1.0)?,
        });
    }
    let count = prepared.len();
    let mut metrics = Map::new();
    for k in WEIGHTS {
        metrics.insert(k.into(), json!(-2));
    }
    let mut result = Map::new();
    result.insert("status".into(), json!("evaluated_subset"));
    result.insert("gq".into(), fv(1.0));
    result.insert("metrics".into(), Value::Object(metrics.clone()));
    result.insert("events".into(), json!([]));
    result.insert("initial_confidence_mean".into(), Value::Null);
    result.insert("confidence_aggregate".into(), Value::Null);
    result.insert("call_confidence_before_sq".into(), json!([]));
    result.insert("call_confidence_after_sq".into(), json!([]));
    result.insert("sample_sq".into(), fv(sq));
    result.insert("external_validation".into(), json!(false));
    result.insert("confidence_is_probability".into(), json!(false));
    result.insert("thresholds".into(), Value::Object(th_out));
    result.insert("weights".into(), Value::Object(w.iter().map(|(k, v)| (k.to_string(), fv(*v))).collect()));
    result.insert("scope".into(), json!("sample_marker_without_plugins_overrides_or_pullup_processor"));
    result.insert("marker_kind".into(), json!(kind));
    let mut events: Vec<Value> = Vec::new();
    let finish = |mut result: Map<String, Value>, metrics: Map<String, Value>, events: Vec<Value>| {
        result.insert("metrics".into(), Value::Object(metrics));
        result.insert("events".into(), Value::Array(events));
        Ok(Value::Object(result))
    };
    if count == 0 && kind != "y" {
        result.insert("gq".into(), fv(0.0));
        result.insert("confidence_aggregate".into(), fv(0.0));
        metrics.insert("allele_number".into(), json!(1));
        events.push(json!({"criterion": "no_calls_non_y", "forced_gq": fv(0.0)}));
        return finish(result, metrics, events);
    }
    let initial = if count > 0 { Some(fsum(prepared.iter().map(|c| c.confidence)) / count as f64) } else { None };
    let mut aggregate = initial;
    result.insert("initial_confidence_mean".into(), initial.map_or(Value::Null, fv));
    let mut gq = 1.0f64;
    let penalize = |criterion: &str, reason: Option<&str>, metrics: &mut Map<String, Value>, events: &mut Vec<Value>, gq: &mut f64| {
        metrics.insert(criterion.into(), json!(1));
        let before = *gq;
        let factor = 1.0 - weight(criterion);
        *gq *= factor;
        events.push(json!({"criterion": criterion, "reason": reason.unwrap_or(criterion), "factor": fv(factor), "gq_before": fv(before), "gq_after": fv(*gq)}));
    };
    metrics.insert("allele_number".into(), json!(0));
    let mut count_valid = (count as i64) <= maximum;
    let mut count_reason = "called_count_above_maximum";
    if kind == "y" {
        let has = |name: &str| amel.as_ref().unwrap().iter().any(|l| l.to_ascii_lowercase() == name);
        if amel.is_none() || has("y") {
            count_valid = 1 <= count as i64 && (count as i64) <= maximum;
            count_reason = "y_count_without_amel_x_only";
        } else if has("x") {
            count_valid = count == 0;
            count_reason = "y_calls_with_amel_x_only";
        } else {
            count_valid = false;
            count_reason = "amel_has_neither_x_nor_y";
        }
        result.insert("amel_context".into(), amel.as_ref().map_or(Value::Null, |a| json!(a)));
        if count == 0 && count_valid {
            events.push(json!({"criterion": "empty_y_confidence_mean", "value": null, "original_empty_mean_would_be_nan": true}));
        }
    }
    if !count_valid {
        penalize("allele_number", Some(count_reason), &mut metrics, &mut events, &mut gq);
        aggregate = Some(0.0);
        events.push(json!({"criterion": "allele_number_confidence_aggregate", "forced_value": fv(0.0)}));
    }
    if kind == "amel" && !prepared.iter().any(|c| c.label.to_ascii_lowercase() == "x") {
        penalize("allele_number", Some("amel_without_x"), &mut metrics, &mut events, &mut gq);
    }
    let heights: Vec<f64> = prepared.iter().map(|c| c.height).collect();
    let hmin = heights.iter().cloned().fold(f64::INFINITY, f64::min);
    let hmax = heights.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let absolute = threshold(if count == 1 { "homozygous_height" } else { "heterozygous_height" });
    metrics.insert("absolute_height".into(), json!(if count > 0 { 0 } else { -2 }));
    if count > 0 && hmin < absolute {
        penalize("absolute_height", None, &mut metrics, &mut events, &mut gq);
    }
    metrics.insert("high_height".into(), json!(if count > 0 { 0 } else { -2 }));
    let non_off: Vec<f64> = prepared.iter().filter(|c| !c.offscale).map(|c| c.height).collect();
    if non_off.is_empty() {
        metrics.insert("high_height".into(), json!(-2));
    } else if non_off.iter().cloned().fold(f64::NEG_INFINITY, f64::max) > threshold("high_height") {
        penalize("high_height", None, &mut metrics, &mut events, &mut gq);
    }
    metrics.insert("spike".into(), json!(flag("spike") as i64));
    if flag("spike") {
        penalize("spike", None, &mut metrics, &mut events, &mut gq);
    }
    metrics.insert("ratio".into(), json!(if count >= 2 { 0 } else { -2 }));
    let ratio = if count > 0 && hmax > 0.0 { Some(hmin / hmax) } else { None };
    result.insert("observed_height_ratio".into(), ratio.map_or(Value::Null, fv));
    if count >= 2 && ratio.map_or(false, |r| r < threshold("ratio")) {
        penalize("ratio", None, &mut metrics, &mut events, &mut gq);
    }
    metrics.insert("broad".into(), json!(if count > 0 { 0 } else { -2 }));
    if count > 0 && prepared.iter().map(|c| c.fwhm).fold(f64::NEG_INFINITY, f64::max) > threshold("width_bp") {
        penalize("broad", None, &mut metrics, &mut events, &mut gq);
    }
    let off = flag("offscale") || prepared.iter().any(|c| c.offscale);
    metrics.insert("offscale".into(), json!(off as i64));
    if off {
        penalize("offscale", None, &mut metrics, &mut events, &mut gq);
    }
    metrics.insert("overlap".into(), json!(flag("overlap") as i64));
    if flag("overlap") {
        penalize("overlap", None, &mut metrics, &mut events, &mut gq);
    }
    metrics.insert("binning".into(), json!(flag("binning") as i64));
    if flag("binning") {
        penalize("binning", None, &mut metrics, &mut events, &mut gq);
    }
    if mismatch {
        gq = 0.0;
        events.push(json!({"criterion": "positive_control_mismatch", "forced_gq": fv(0.0)}));
    }
    result.insert("gq".into(), fv(gq));
    result.insert("confidence_aggregate".into(), aggregate.map_or(Value::Null, fv));
    let before = aggregate.map(|a| a * gq);
    result.insert("call_confidence_before_sq".into(), Value::Array((0..count).map(|_| before.map_or(Value::Null, fv)).collect()));
    result.insert(
        "call_confidence_after_sq".into(),
        match before {
            Some(b) => Value::Array((0..count).map(|_| fv(b * sq)).collect()),
            None => json!([]),
        },
    );
    finish(result, metrics, events)
}

pub fn score_ordinary_marker(
    calls: &Value,
    thresholds: &Value,
    weights: &Value,
    flags: &Value,
    sample_sq: &Value,
    marker_kind: &Value,
    control_mismatch: &Value,
) -> Res<Value> {
    if !matches!(marker_kind.as_str(), Some("autosomal" | "amel")) {
        return Err("Use the explicit Y API for y markers".into());
    }
    score_marker(calls, thresholds, weights, flags, sample_sq, marker_kind, control_mismatch, &Value::Null)
}

pub fn score_y_marker(
    calls: &Value,
    thresholds: &Value,
    weights: &Value,
    flags: &Value,
    sample_sq: &Value,
    amel_calls: &Value,
    control_mismatch: &Value,
) -> Res<Value> {
    score_marker(calls, thresholds, weights, flags, sample_sq, &json!("y"), control_mismatch, amel_calls)
}

pub fn score_negative_marker(
    confidences: &Value,
    zero_peak_value: &Value,
    spike_metric: &Value,
    spike_weight: &Value,
    sample_sq: &Value,
) -> Res<Value> {
    let zero = number(zero_peak_value, "QC_ZERO_PEAK", -3.0, 1e6)?;
    let spike = number(spike_metric, "spike_metric", -2.0, 1.0)?;
    if ![-2.0, -1.0, 0.0, 1.0].contains(&spike) {
        return Err("Explicit spike metric state -2/-1/0/1 required".into());
    }
    let weight = number(spike_weight, "spike weight", 0.0, 1.0)?;
    let sq = number(sample_sq, "sample_sq", 0.0, 1.0)?;
    let list = match confidences {
        Value::Array(c) if c.len() <= 4096 => c,
        _ => return Err("At most4096 initial call confidences required".into()),
    };
    let mut initial = Vec::new();
    for v in list {
        initial.push(number(v, "initial confidence", 0.0, 1.0)?);
    }
    let mut gq = if zero == 0.0 { 0.0 } else { 1.0 };
    let mut events = vec![json!({"criterion": "negative_zero_peak", "value": fv(zero), "gq_after": fv(gq)})];
    if spike == 1.0 {
        let before = gq;
        gq *= 1.0 - weight;
        events.push(json!({"criterion": "spike", "factor": fv(1.0 - weight), "gq_before": fv(before), "gq_after": fv(gq)}));
    }
    Ok(json!({
        "status": "evaluated_subset", "scope": "fresh_negative_control_zero_peak_and_spike",
        "gq": fv(gq), "cc": (zero == 0.0) as i64, "metrics": {"zero_peak": fv(zero), "spike": fv(spike)},
        "events": events, "sample_sq": fv(sq), "spike_weight": fv(weight),
        "initial_confidence_mean": null, "confidence_aggregate": null,
        "ordinary_confidence_assignment_applied": false,
        "call_confidence_before_sq": initial.iter().map(|v| fv(*v)).collect::<Vec<_>>(),
        "call_confidence_after_sq": initial.iter().map(|v| fv(*v * sq)).collect::<Vec<_>>(),
        "external_validation": false, "confidence_is_probability": false,
    }))
}
