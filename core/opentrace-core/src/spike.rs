//! Port of `genemapper_open/spike.py` (definition validation, companion measurement, classification) and the
//! independent `validate_adjustments` policy from `spike_adjustment.py`. The stateful `SpikeState` (which depends
//! on `allele_calling`) is ported separately.
use crate::peak_morphology::{bounded, measure, Definition};
use crate::small_svd::SOLVER_ID;
use crate::true_peak::round_scan;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

pub type Res<T> = Result<T, String>;

const DEFINITION_LIMIT_BYTES: usize = 16384;

fn bool_of(v: &Value, name: &str) -> Res<bool> {
    match v {
        Value::Bool(b) => Ok(*b),
        _ => Err(format!("{}: explicit boolean required", name)),
    }
}

fn key_set(v: &Value) -> Option<BTreeSet<String>> {
    match v {
        Value::Object(m) => Some(m.keys().cloned().collect()),
        _ => None,
    }
}

fn set_of(keys: &[&str]) -> BTreeSet<String> {
    keys.iter().map(|k| k.to_string()).collect()
}

/// Python `validate_adjustments(policy)`: explicit sample/ladder adjustment flags.
pub fn validate_adjustments(policy: &Value) -> Res<()> {
    let keys = set_of(&["sample_cutoff", "sample_marker_envelope", "ladder_height_ratio"]);
    if key_set(policy).map_or(true, |k| k != keys) {
        return Err("Explicit sample_cutoff, sample_marker_envelope and ladder_height_ratio required".into());
    }
    bool_of(&policy["sample_cutoff"], "sample_cutoff")?;
    bool_of(&policy["sample_marker_envelope"], "sample_marker_envelope")?;
    bounded(&policy["ladder_height_ratio"], "ladder_height_ratio", 0.0, 1e12)?;
    Ok(())
}

/// Python `load_spike_definition` for a mapping source (the JSON-text path is not ported yet).
pub fn load_spike_definition(source: &Value) -> Res<Value> {
    let approx_bytes = serde_json::to_string(source).map(|s| s.len()).unwrap_or(usize::MAX);
    if approx_bytes > DEFINITION_LIMIT_BYTES {
        return Err("Spike definition exceeds16KiB".into());
    }
    load_spike_definition_unsized(source)
}

/// The validation after the size check (the JSON-text path checks the byte length of the text instead).
pub fn load_spike_definition_unsized(source: &Value) -> Res<Value> {
    let map = match source {
        Value::Object(m) => m,
        _ => return Err("Explicit spike definition required".into()),
    };
    let version = match map.get("schema_version") {
        Some(Value::Number(n)) if n.is_i64() || n.is_u64() => n.as_i64().unwrap(),
        _ => return Err("Spike schema_version must be1,2,3 or4".into()),
    };
    if !(1..=4).contains(&version) {
        return Err("Spike schema_version must be1,2,3 or4".into());
    }
    let mut keys = set_of(&[
        "schema_version", "measurement_source", "scope", "morphology_enabled", "units", "min_height",
        "exhalf_width_threshold", "near_base_width_threshold", "area_ratio_threshold",
        "treat_overlap_as_spike", "near_base_fraction", "max_base_height",
    ]);
    if version >= 2 {
        keys.insert("adjustments".into());
    }
    if version >= 3 {
        keys.insert("morphology".into());
    }
    if version == 4 {
        keys.insert("true_peak".into());
    }
    let present: BTreeSet<String> = map.keys().cloned().collect();
    if present != keys {
        return Err("All explicit spike fields required".into());
    }
    if version >= 2 {
        validate_adjustments(&map["adjustments"])?;
    }
    let measurement = if version >= 3 { "baseline_corrected_companion" } else { "prepared_signal" };
    if map["measurement_source"] != json!(measurement) || map["scope"] != json!("non_reference_selected_channels") {
        return Err(format!(
            "Explicit {}/non_reference_selected_channels policy required",
            measurement
        ));
    }
    let morphology_enabled = match &map["morphology_enabled"] {
        Value::Bool(b) => *b,
        _ => return Err("Morphology policy must match spike schema".into()),
    };
    if morphology_enabled != (version >= 3) {
        return Err("Morphology policy must match spike schema".into());
    }
    if version >= 3 {
        if version == 3 && map["treat_overlap_as_spike"] != json!(true) {
            return Err("Morphology subset requires treat_overlap_as_spike=true; true-peak restoration is not implemented".into());
        }
        let parameters = match &map["morphology"] {
            Value::Object(p) if p.len() == 2 && p.contains_key("jag_threshold") && p.contains_key("fit_score_threshold") => p,
            _ => return Err("Explicit jag_threshold and fit_score_threshold required".into()),
        };
        bounded(&parameters["jag_threshold"], "jag_threshold", 0.0, 1e12)?;
        bounded(&parameters["fit_score_threshold"], "fit_score_threshold", 0.0, 1.0)?;
    }
    if version == 4 {
        let parameters = match &map["true_peak"] {
            Value::Object(p) if p.len() == 2 && p.contains_key("polynomial_solver") && p.contains_key("max_denominator_scan2") => p,
            _ => return Err("Explicit true_peak solver and Gaussian denominator bound required".into()),
        };
        if parameters["polynomial_solver"] != json!(SOLVER_ID) {
            return Err("Unsupported explicit true-peak polynomial solver".into());
        }
        bounded(&parameters["max_denominator_scan2"], "max_denominator_scan2", 1e-12, 1e12)?;
    }
    let units = &map["units"];
    if units != &json!("scan") && units != &json!("bp") {
        return Err("Explicit scan or bp units required".into());
    }
    bool_of(&map["treat_overlap_as_spike"], "treat_overlap_as_spike")?;
    for key in ["min_height", "exhalf_width_threshold", "near_base_width_threshold", "area_ratio_threshold"] {
        bounded(&map[key], key, 0.0, 1e12)?;
    }
    bounded(&map["near_base_fraction"], "near_base_fraction", 1e-12, 1.0)?;
    bounded(&map["max_base_height"], "max_base_height", -1.0, 1e12)?;
    Ok(source.clone())
}

/// Typed view of a validated spike definition (the fields the measurement and classification read).
pub struct SpikeDefinition {
    pub schema_version: i64,
    pub units: String,
    pub measurement_source: String,
    pub treat_overlap_as_spike: bool,
    pub min_height: f64,
    pub exhalf_width_threshold: f64,
    pub near_base_width_threshold: f64,
    pub area_ratio_threshold: f64,
    pub near_base_fraction: f64,
    pub max_base_height: f64,
    pub jag_threshold: f64,
    pub fit_score_threshold: f64,
    pub max_denominator_scan2: f64,
}

impl SpikeDefinition {
    /// Build from a validated definition value (see `load_spike_definition`).
    pub fn from_validated(v: &Value) -> SpikeDefinition {
        let f = |k: &str| v[k].as_f64().unwrap_or(0.0);
        let morphology = &v["morphology"];
        let true_peak = &v["true_peak"];
        SpikeDefinition {
            schema_version: v["schema_version"].as_i64().unwrap(),
            units: v["units"].as_str().unwrap().to_string(),
            measurement_source: v["measurement_source"].as_str().unwrap().to_string(),
            treat_overlap_as_spike: v["treat_overlap_as_spike"].as_bool().unwrap(),
            min_height: f("min_height"),
            exhalf_width_threshold: f("exhalf_width_threshold"),
            near_base_width_threshold: f("near_base_width_threshold"),
            area_ratio_threshold: f("area_ratio_threshold"),
            near_base_fraction: f("near_base_fraction"),
            max_base_height: f("max_base_height"),
            jag_threshold: morphology["jag_threshold"].as_f64().unwrap_or(0.0),
            fit_score_threshold: morphology["fit_score_threshold"].as_f64().unwrap_or(0.0),
            max_denominator_scan2: true_peak["max_denominator_scan2"].as_f64().unwrap_or(0.0),
        }
    }

    pub fn morphology_definition(&self) -> Definition {
        Definition {
            near_base_fraction: self.near_base_fraction,
            max_base_height: self.max_base_height,
            schema_version: self.schema_version,
            jag_threshold: self.jag_threshold,
            max_denominator_scan2: self.max_denominator_scan2,
        }
    }
}

/// Python `_signal`: bounded nonnegative signed-32 integer samples.
pub fn signal_checked(v: &Value) -> Res<Vec<f64>> {
    let items = match v {
        Value::Array(a) if (1..=1_000_000).contains(&a.len()) => a,
        _ => return Err("Bounded prepared integer signal required".into()),
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let x = match item {
            Value::Number(n) if n.is_i64() || n.is_u64() => n.as_i64().unwrap_or(-1),
            _ => return Err("Prepared signal must be nonnegative signed32 integers".into()),
        };
        if !(0..2147483648).contains(&x) {
            return Err("Prepared signal must be nonnegative signed32 integers".into());
        }
        out.push(x as f64);
    }
    Ok(out)
}

fn fv(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

/// Python `_width_out`: widened width and mirrored integer area at a fractional height; areas stay integers.
pub fn width_out(signal: &[f64], lo: i64, hi: i64, center: i64, ratio: f64, cap: f64) -> Value {
    let at = |i: i64| signal[i as usize];
    let mut maximum = at(lo);
    for i in lo + 1..=hi {
        if at(i) > maximum {
            maximum = at(i);
        }
    }
    let mut apex = (lo..=hi).find(|&i| at(i) == maximum).unwrap();
    if apex != center {
        let original_distance = (apex - center).abs();
        for i in lo..=hi {
            if at(i) == maximum && (i - center).abs() < original_distance {
                apex = i;
            }
        }
    }
    if apex == lo || apex == hi {
        apex = center;
        maximum = at(center);
    }
    if maximum <= 0.0 {
        return json!({"width_scan": fv(-1.0), "area": fv(-1.0), "height": fv(-1.0), "center": apex});
    }
    let mut level = ratio * maximum;
    if cap >= 0.0 {
        level = if cap < level { cap } else { level };
    }
    let mut left = apex - 1;
    let mut right = apex + 1;
    while left >= lo && at(left) > level {
        left -= 1;
    }
    while right <= hi && at(right) > level {
        right += 1;
    }
    let truncated_left = left < lo;
    let truncated_right = right > hi;
    let left_index = left.max(lo);
    let right_index = right.min(hi);
    let left_cross = if truncated_left {
        lo as f64
    } else {
        let difference = at(left + 1) - at(left);
        if difference > 0.0 { left as f64 + (level - at(left)) / difference } else { left as f64 }
    };
    let right_cross = if truncated_right {
        hi as f64
    } else {
        let difference = at(right - 1) - at(right);
        if difference > 0.0 { right as f64 - (level - at(right)) / difference } else { right as f64 }
    };
    let mut left_width = apex as f64 - left_cross;
    let mut right_width = right_cross - apex as f64;
    let mut left_area: i64 = (left_index..apex).map(|i| at(i) as i64).sum();
    let mut right_area: i64 = (apex + 1..right_index + 1).map(|i| at(i) as i64).sum();
    if truncated_left && !truncated_right {
        left_width = left_width.max(right_width);
        left_area = right_area;
    } else if truncated_right && !truncated_left {
        right_width = left_width.max(right_width);
        right_area = left_area;
    }
    let area = left_area + right_area + at(apex) as i64;
    json!({
        "width_scan": fv(left_width + right_width),
        "area": area,
        "height": fv(maximum),
        "center": apex,
        "left_truncated": truncated_left,
        "right_truncated": truncated_right,
    })
}

/// Python `_measure_features`: companion jag/fit and width measurement of one SG peak (validated definition).
pub fn measure_features(signal: &[f64], peak: &Value, def: &SpikeDefinition) -> Res<Value> {
    let mut result = Map::new();
    result.insert("status".into(), json!("unavailable"));
    result.insert("reason".into(), Value::Null);
    result.insert("measurement_source".into(), json!(def.measurement_source));
    result.insert("exhalf_width_scan".into(), Value::Null);
    result.insert("near_base_width_scan".into(), Value::Null);
    result.insert("near_base_area".into(), Value::Null);
    result.insert("raw_height".into(), Value::Null);
    result.insert("area_ratio".into(), Value::Null);
    result.insert("jag_count".into(), json!(-1));
    result.insert("jag_count_source".into(), json!("fresh_uncomputed_sentinel_morphology_disabled"));
    result.insert("external_validation".into(), json!(false));
    result.insert("original_runtime_validated".into(), json!(false));
    let peak = match peak {
        Value::Object(_) => peak,
        _ => return Err("Measured SG peak required".into()),
    };
    let last = (signal.len() - 1) as f64;
    let begin = bounded(peak.get("begin").unwrap_or(&Value::Null), "begin", 0.0, last)?;
    let end = bounded(peak.get("end").unwrap_or(&Value::Null), "end", 0.0, last)?;
    let crossing = bounded(peak.get("sg_scan").unwrap_or(&Value::Null), "SG crossing", 0.0, last)?;
    let lo = begin.ceil() as i64;
    let hi = end.floor() as i64;
    let center = round_scan(crossing)?;
    if !(lo <= center && center <= hi) {
        result.insert("reason".into(), json!("sg_measurement_bounds_unavailable"));
        return Ok(Value::Object(result));
    }
    let half = width_out(signal, lo, hi, center, 0.5, -1.0);
    let base = width_out(signal, lo, hi, center, def.near_base_fraction, def.max_base_height);
    let base_area = base["area"].as_f64().unwrap_or(-1.0);
    let half_width = half["width_scan"].as_f64().unwrap();
    let half_height = half["height"].as_f64().unwrap();
    let ratio = if base_area > 0.0 { half_width * half_height / base_area } else { -1.0 };
    let flag = |v: &Value, k: &str| v.get(k).cloned().unwrap_or(json!(false));
    result.insert("status".into(), json!("evaluated_subset"));
    result.insert("exhalf_width_scan".into(), fv(half_width));
    result.insert("near_base_width_scan".into(), base["width_scan"].clone());
    result.insert("near_base_area".into(), base["area"].clone());
    result.insert("raw_height".into(), fv(half_height));
    result.insert("area_ratio".into(), fv(ratio));
    result.insert("measurement_center".into(), half["center"].clone());
    result.insert(
        "half_truncation".into(),
        json!([flag(&half, "left_truncated"), flag(&half, "right_truncated")]),
    );
    result.insert(
        "base_truncation".into(),
        json!([flag(&base, "left_truncated"), flag(&base, "right_truncated")]),
    );
    if def.schema_version >= 3 {
        let morphology = measure(signal, lo, hi, center, half_width, &def.morphology_definition())?;
        let jag_count = morphology["jag_count"].as_i64().unwrap();
        result.insert("morphology".into(), morphology.clone());
        result.insert("jag_count".into(), json!(jag_count));
        result.insert(
            "jag_count_source".into(),
            json!(if jag_count >= 0 { "measured_companion" } else { "morphology_uncomputed_sentinel" }),
        );
        result.insert("fit_score".into(), morphology["fit_score"].clone());
        result.insert("area_ratio".into(), morphology["area_ratio"].clone());
    }
    Ok(Value::Object(result))
}

/// Python `measure_spike_features(signal, peak, definition)`: validates the definition and the signal first.
pub fn measure_spike_features(signal: &Value, peak: &Value, definition: &Value) -> Res<Value> {
    let validated = load_spike_definition(definition)?;
    let samples = signal_checked(signal)?;
    let def = SpikeDefinition::from_validated(&validated);
    measure_features(&samples, peak, &def)
}

/// Python `classify_spike(features, height=..., offscaled=..., definition=...)`: fresh basic classification.
pub fn classify_spike(features: &Value, height: &Value, offscaled: &Value, definition: &Value) -> Res<Value> {
    let validated = load_spike_definition(definition)?;
    let def = SpikeDefinition::from_validated(&validated);
    let height = bounded(height, "height", 0.0, 1e12)?;
    bool_of(offscaled, "offscaled")?;
    let offscaled = offscaled.as_bool().unwrap();
    let mut result = Map::new();
    result.insert("status".into(), json!("unavailable"));
    result.insert("reason".into(), Value::Null);
    result.insert("spike_detected".into(), Value::Null);
    result.insert("overlap_spike".into(), Value::Null);
    result.insert("current_spike".into(), Value::Null);
    result.insert("threshold_used".into(), Value::Null);
    result.insert("units".into(), json!(def.units));
    result.insert(
        "scope".into(),
        json!("fresh_basic_initial_classification_before_downstream_adjustments"),
    );
    result.insert("external_validation".into(), json!(false));
    result.insert("original_runtime_validated".into(), json!(false));
    let feats = match features {
        Value::Object(m) if m.get("status") == Some(&json!("evaluated_subset")) => m,
        _ => {
            result.insert("reason".into(), json!("basic_spike_features_missing"));
            return Ok(Value::Object(result));
        }
    };
    let suffix = def.units.as_str();
    let names = [
        format!("exhalf_width_{}", suffix),
        format!("near_base_width_{}", suffix),
        "area_ratio".to_string(),
        "jag_count".to_string(),
    ];
    if names.iter().any(|k| feats.get(k).map_or(true, |v| v.is_null())) {
        result.insert("reason".into(), json!("spike_units_or_features_unavailable"));
        return Ok(Value::Object(result));
    }
    let f = bounded(&feats[&names[0]], &names[0], -1.0, 1e12)?;
    let b = bounded(&feats[&names[1]], &names[1], -1.0, 1e12)?;
    let r = bounded(&feats[&names[2]], &names[2], -1.0, 1e12)?;
    let j = bounded(&feats[&names[3]], &names[3], -1.0, 1e12)?;
    let mut detected = false;
    let mut overlap = false;
    let mut threshold = validated["near_base_width_threshold"].clone();
    let mut reason = "widths_not_narrow";
    if b >= 0.0 && height > def.min_height {
        if b < def.near_base_width_threshold {
            detected = true;
            reason = "near_base_width_below_threshold";
        } else if f >= 0.0 && f < def.exhalf_width_threshold {
            if r > def.area_ratio_threshold && !offscaled {
                detected = true;
                threshold = validated["exhalf_width_threshold"].clone();
                reason = "narrow_exhalf_with_area_ratio";
            } else {
                overlap = true;
                reason = "narrow_exhalf_overlap";
            }
        } else if j > 0.0 {
            overlap = true;
            reason = "positive_jag_count";
        }
    } else {
        reason = "height_or_base_width_gate";
    }
    let current = detected || (def.treat_overlap_as_spike && overlap);
    if overlap && !detected && def.treat_overlap_as_spike {
        threshold = validated["exhalf_width_threshold"].clone();
    }
    result.insert("status".into(), json!("evaluated_subset"));
    result.insert("reason".into(), json!(reason));
    result.insert("spike_detected".into(), json!(detected));
    result.insert("overlap_spike".into(), json!(overlap));
    result.insert("current_spike".into(), json!(current));
    result.insert("threshold_used".into(), threshold);
    if def.schema_version >= 3 {
        let broad = b >= 0.0
            && height > def.min_height
            && b >= def.near_base_width_threshold
            && !(0.0 <= f && f < def.exhalf_width_threshold);
        let fit_value = feats.get("fit_score").cloned().unwrap_or(Value::Null);
        if fit_value.is_null() {
            result.insert("status".into(), json!("unavailable"));
            result.insert("reason".into(), json!("morphology_fit_score_missing"));
            result.insert("spike_detected".into(), Value::Null);
            result.insert("overlap_spike".into(), Value::Null);
            result.insert("current_spike".into(), Value::Null);
            result.insert("threshold_used".into(), Value::Null);
            return Ok(Value::Object(result));
        }
        let fit = bounded(&fit_value, "fit_score", -1.0, 1e12)?;
        let poor = broad && fit < def.fit_score_threshold;
        result.insert("peak_deformity".into(), json!(if poor { "poor_fit" } else { "none" }));
        result.insert(
            "scope".into(),
            json!("fresh_companion_jag_fit_initial_classification_overlap_excluded"),
        );
        result.insert("true_peak_restoration_applied".into(), json!(false));
        if def.schema_version == 4 {
            result.insert(
                "scope".into(),
                json!("fresh_companion_true_peak_initial_classification"),
            );
            let requested = !detected && !def.treat_overlap_as_spike && overlap;
            result.insert("true_peak_restoration_requested".into(), json!(requested));
        }
    }
    Ok(Value::Object(result))
}
