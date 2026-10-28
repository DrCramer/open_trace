//! Port of `genemapper_open/peak_morphology.py`: jag (normalized second differences) and ideal-Gaussian
//! correlation measurements on a baseline-corrected companion. Schema 4 (true-peak delegation) is not part of
//! this module yet and is rejected explicitly.
use crate::calibration::py_exp;
use serde_json::{json, Map, Value};

pub type Res<T> = Result<T, String>;

/// Python `quality_control._number`: finite, bounded, non-bool real number.
pub fn bounded(v: &Value, name: &str, low: f64, high: f64) -> Res<f64> {
    if let Value::Number(n) = v {
        if let Some(x) = n.as_f64() {
            if x.is_finite() && low <= x && x <= high {
                return Ok(x);
            }
        }
    }
    Err(format!("{}: finite bounded number required", name))
}

/// Python `_values`: bounded list of numeric samples.
fn sample_values(v: &Value) -> Res<Vec<f64>> {
    let items = match v {
        Value::Array(a) if a.len() <= 2_000_001 => a,
        _ => return Err("Bounded numeric morphology samples required".into()),
    };
    items
        .iter()
        .map(|x| bounded(x, "morphology sample", -2147483648.0, 2147483647.0))
        .collect()
}

pub struct Jag {
    pub score: f64,
    pub count: i64,
    pub maximum: f64,
}

/// Python `_jag`: unavailable (short or nonpositive) returns the -1 sentinels.
fn jag(values: &[f64], threshold: f64) -> Jag {
    let unavailable = Jag { score: -1.0, count: -1, maximum: -1.0 };
    if values.len() < 3 {
        return unavailable;
    }
    let max_value = values.iter().cloned().fold(values[0], |m, x| if x > m { x } else { m });
    if max_value <= 0.0 {
        return unavailable;
    }
    let height = max_value;
    let mut total = 0.0f64;
    let mut count: i64 = 0;
    let mut maximum = -1.0f64;
    for index in 1..values.len() - 1 {
        let change = (2.0 * values[index] - values[index + 1] - values[index - 1]).abs() / 2.0 / height;
        total += change;
        if change > threshold {
            count += 1;
        }
        maximum = if change > maximum { change } else { maximum };
    }
    Jag { score: total / values.len() as f64, count, maximum }
}

/// Python `jag_metrics(values, threshold)`.
pub fn jag_metrics(values: &Value, threshold: &Value) -> Res<Jag> {
    let threshold = bounded(threshold, "jag threshold", 0.0, 1e12)?;
    let values = sample_values(values)?;
    Ok(jag(&values, threshold))
}

/// Python `gaussian_samples`: ideal Gaussian vs linearly interpolated observations, original order first.
pub fn gaussian_samples(values: &[f64], width: f64, height: f64, center: f64) -> (Vec<f64>, Vec<f64>) {
    if values.len() < 2 {
        return (vec![], vec![]);
    }
    let sigma = width / 2.35482;
    let denominator = sigma * 2.0 * sigma;
    let n = values.len();
    let mut extra: i64 = (30i64 - n as i64).max(0);
    let mut step = 0.0f64;
    if extra != 0 {
        step = (n as f64 - 1.0) / extra as f64;
        if step >= 1.0 && (step - ((step as i64) as f64)) < 0.11 {
            step = (step as i64) as f64 + 0.11;
        }
        if step == 0.0 {
            step = 0.11;
        }
        extra = ((((n as f64 - 1.0) / step) as i64) - 1).max(0);
    }
    let mut expected: Vec<f64> = Vec::new();
    let mut observed: Vec<f64> = Vec::new();
    let mut append = |position: f64, value: f64| {
        let d = position - center;
        expected.push(height * py_exp(-(d * d) / denominator));
        observed.push(value);
    };
    for (index, &value) in values.iter().enumerate() {
        append(index as f64, value);
    }
    let mut position = step;
    for _ in 0..extra {
        let lower = position.floor();
        let upper = position.ceil();
        let (a, b) = if (upper as usize) < n {
            (values[lower as usize], values[upper as usize])
        } else {
            (0.0, 0.0)
        };
        append(position, a + (b - a) * (position - lower));
        position += step;
    }
    (expected, observed)
}

/// Python `gaussian_fit_score`: squared Pearson correlation; zero variance gives 1.0.
pub fn gaussian_fit_score(values: &[f64], width: f64, height: f64, center: f64) -> f64 {
    let (expected, observed) = gaussian_samples(values, width, height, center);
    if expected.is_empty() {
        return -1.0;
    }
    let (mut se, mut so, mut seo, mut see, mut soo) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for (&e, &o) in expected.iter().zip(observed.iter()) {
        se += e;
        so += o;
        seo += e * o;
        see += e * e;
        soo += o * o;
    }
    let count = expected.len() as f64;
    let covariance = seo - se * so / count;
    let ve = see - se * se / count;
    let vo = soo - so * so / count;
    if ve != 0.0 && vo != 0.0 {
        covariance * covariance / (ve * vo)
    } else {
        1.0
    }
}

/// Python slice `signal[a:b]` for nonnegative indices (clipped to the list length).
fn py_slice(signal: &[f64], a: i64, b: i64) -> Vec<f64> {
    let n = signal.len() as i64;
    let a = a.clamp(0, n) as usize;
    let b = b.clamp(0, n) as usize;
    if a >= b {
        vec![]
    } else {
        signal[a..b].to_vec()
    }
}

fn reversed(mut v: Vec<f64>) -> Vec<f64> {
    v.reverse();
    v
}

fn concat(mut a: Vec<f64>, b: Vec<f64>) -> Vec<f64> {
    a.extend(b);
    a
}

pub struct Selection {
    pub height: f64,
    pub center: i64,
    pub relative_center: i64,
    pub origin: i64,
    pub near_crossings: [i64; 2],
    pub half_crossings: [i64; 2],
    pub truncated: [bool; 2],
}

pub struct Extracted {
    pub whole: Vec<f64>,
    pub top: Vec<f64>,
    pub left: Vec<f64>,
    pub right: Vec<f64>,
    pub selection: Selection,
}

/// Python `_extract`: local peak support from the normal (non-legacy) source route.
pub fn extract(signal: &[f64], lo: i64, hi: i64, center: i64, fraction: f64, cap: f64) -> Extracted {
    let at = |i: i64| signal[i as usize];
    let mut height = at(lo);
    let mut selected = center;
    for index in lo + 1..=hi {
        if at(index) > height {
            height = at(index);
            selected = index;
        }
    }
    if selected == center {
        height = at(center);
    } else {
        let distance = (selected - center).abs();
        for index in lo..=hi {
            if at(index) == height && (index - center).abs() < distance {
                selected = index;
            }
        }
    }
    if selected == lo || selected == hi {
        selected = center;
        height = at(center);
    }
    let mut near = height * fraction;
    if cap >= 0.0 {
        near = if cap < near { cap } else { near };
    }
    // Returns (index, truncated) for the first sample at or below `level` moving away from the peak.
    let crossing = |step: i64, level: f64| -> (i64, bool) {
        let mut index = selected + step;
        while lo <= index && index <= hi {
            if at(index) <= level {
                return (index, false);
            }
            index += step;
        }
        (if step < 0 { lo } else { hi }, true)
    };
    let (left, truncated_left) = crossing(-1, near);
    let (right, truncated_right) = crossing(1, near);
    let (half_left, _) = crossing(-1, height / 2.0);
    let (half_right, _) = crossing(1, height / 2.0);
    let (mut bottom_left, mut bottom_right) = (left, right);
    if height / 2.0 <= near || left > half_left || right < half_right {
        bottom_left = lo;
        bottom_right = hi;
    }
    let sig = |a: i64, b: i64| py_slice(signal, a, b);
    let (whole, top, bl, br, relative, origin);
    if truncated_left && !truncated_right {
        whole = concat(reversed(sig(selected, right + 1)), sig(selected + 1, right + 1));
        top = concat(reversed(sig(selected, half_right + 1)), sig(selected + 1, half_right + 1));
        let br_v = sig(half_right, bottom_right + 1);
        br = br_v.clone();
        bl = reversed(br_v);
        relative = right - selected;
        origin = 2 * selected - right;
    } else if truncated_right && !truncated_left {
        whole = concat(sig(left, selected + 1), reversed(sig(left, selected)));
        top = concat(sig(half_left, selected + 1), reversed(sig(half_left, selected)));
        let bl_v = sig(bottom_left, half_left + 1);
        bl = bl_v.clone();
        br = reversed(bl_v);
        relative = selected - left;
        origin = left;
    } else {
        whole = sig(left, right + 1);
        top = sig(half_left, half_right + 1);
        bl = sig(bottom_left, half_left + 1);
        br = sig(half_right, bottom_right + 1);
        relative = selected - left;
        origin = left;
    }
    Extracted {
        whole,
        top,
        left: bl,
        right: br,
        selection: Selection {
            height,
            center: selected,
            relative_center: relative,
            origin,
            near_crossings: [left, right],
            half_crossings: [half_left, half_right],
            truncated: [truncated_left, truncated_right],
        },
    }
}

pub struct Definition {
    pub near_base_fraction: f64,
    pub max_base_height: f64,
    pub schema_version: i64,
    pub jag_threshold: f64,
}

fn fv(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

/// Python `_measure`: called after the spike module validated signal, bounds and definition.
pub fn measure(signal: &[f64], lo: i64, hi: i64, center: i64, width: f64, definition: &Definition) -> Res<Value> {
    if definition.schema_version == 4 {
        return Err("schema 4 true-peak delegation is not ported yet".into());
    }
    let mut result = Map::new();
    result.insert("status".into(), json!("unavailable"));
    result.insert("reason".into(), json!("insufficient_integer_support"));
    result.insert("jag_score".into(), fv(-1.0));
    result.insert("jag_score_top".into(), fv(-1.0));
    result.insert("jag_score_bottom".into(), fv(-1.0));
    result.insert("jag_count".into(), json!(-1));
    result.insert("jag_max".into(), fv(-1.0));
    result.insert("fit_score".into(), fv(-1.0));
    result.insert("area_ratio".into(), fv(-1.0));
    result.insert("true_peak_restoration".into(), json!("not_implemented"));
    result.insert("original_runtime_validated".into(), json!(false));
    if hi - lo < 3 {
        return Ok(Value::Object(result));
    }
    let ex = extract(signal, lo, hi, center, definition.near_base_fraction, definition.max_base_height);
    if ex.whole.len() < 3 {
        result.insert("reason".into(), json!("extracted_support_shorter_than_three"));
        return Ok(Value::Object(result));
    }
    let threshold = definition.jag_threshold;
    let whole_jag = jag(&ex.whole, threshold);
    let top_score = jag(&ex.top, threshold).score;
    let (left, right) = (&ex.left, &ex.right);
    let bottom_score = if left.len() < 3 && right.len() < 3 {
        top_score
    } else if left.len() >= 3 && right.len() >= 3 {
        (jag(left, threshold).score + jag(right, threshold).score) / 2.0
    } else {
        jag(if left.len() >= 3 { left } else { right }, threshold).score
    };
    let mut total = 0.0f64;
    for &value in &ex.whole {
        total += value.abs();
    }
    let s = &ex.selection;
    let ratio = if total > 0.0 { width * s.height / total } else { -1.0 };
    let mut fit = -1.0f64;
    let mut reason: Value = Value::Null;
    if width >= 1e-12 {
        fit = gaussian_fit_score(&ex.whole, width, s.height, s.relative_center as f64);
    } else {
        reason = json!("nonpositive_or_too_small_exhalf_width");
    }
    let selection = json!({
        "height": fv(s.height),
        "center": s.center,
        "relative_center": s.relative_center,
        "origin": s.origin,
        "near_crossings": [s.near_crossings[0], s.near_crossings[1]],
        "half_crossings": [s.half_crossings[0], s.half_crossings[1]],
        "truncated": [s.truncated[0], s.truncated[1]],
    });
    result.insert("status".into(), json!("evaluated_subset"));
    result.insert("reason".into(), reason);
    result.insert("jag_score".into(), fv(whole_jag.score));
    result.insert("jag_score_top".into(), fv(top_score));
    result.insert("jag_score_bottom".into(), fv(bottom_score));
    result.insert("jag_count".into(), json!(whole_jag.count));
    result.insert("jag_max".into(), fv(whole_jag.maximum));
    result.insert("fit_score".into(), fv(fit));
    result.insert("area_ratio".into(), fv(ratio));
    result.insert("selection".into(), selection);
    result.insert(
        "sample_counts".into(),
        json!({
            "whole": ex.whole.len(),
            "top": ex.top.len(),
            "bottom_left": left.len(),
            "bottom_right": right.len(),
        }),
    );
    Ok(Value::Object(result))
}

/// Python `gaussian_samples` with its argument validation (values, then width, height, center).
pub fn gaussian_samples_checked(
    values: &Value,
    width: &Value,
    height: &Value,
    center: &Value,
) -> Res<(Vec<f64>, Vec<f64>)> {
    let values = sample_values(values)?;
    let width = bounded(width, "exhalf width", 1e-12, 1e8)?;
    let height = bounded(height, "extracted height", 0.0, 2147483647.0)?;
    let center_high = (values.len() as f64 - 1.0).max(0.0);
    let center = bounded(center, "relative center", 0.0, center_high)?;
    Ok(gaussian_samples(&values, width, height, center))
}

/// Python `gaussian_fit_score` with validation (identical checks to `gaussian_samples`).
pub fn gaussian_fit_score_checked(values: &Value, width: &Value, height: &Value, center: &Value) -> Res<f64> {
    let values = sample_values(values)?;
    let width = bounded(width, "exhalf width", 1e-12, 1e8)?;
    let height = bounded(height, "extracted height", 0.0, 2147483647.0)?;
    let center = bounded(center, "relative center", 0.0, (values.len() as f64 - 1.0).max(0.0))?;
    Ok(gaussian_fit_score(&values, width, height, center))
}
