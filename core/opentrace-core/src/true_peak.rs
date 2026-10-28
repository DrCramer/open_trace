//! Port of `genemapper_open/true_peak.py`: fresh true-peak model from side crossovers and accepted pairs.
//! Operation order follows the Python source; `math.log`/`math.exp` go through libm as in CPython.
use crate::peak_morphology::bounded;
use crate::small_svd::{polynomial_fit, SOLVER_ID};
use serde_json::{json, Map, Value};

pub type Res<T> = Result<T, String>;

extern "C" {
    #[link_name = "log"]
    fn libm_log(x: f64) -> f64;
}

fn log(x: f64) -> f64 {
    unsafe { libm_log(x) }
}

fn fv(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

fn pair(p: (i64, f64)) -> Value {
    json!([p.0, fv(p.1)])
}

/// Python `rounding.round_scan`: ties toward +infinity, computed exactly from the binary64 value.
pub fn round_scan(x: f64) -> Res<i64> {
    if !(x.is_finite() && -1e8 <= x && x <= 1e8) {
        return Err("Finite bounded scan required".into());
    }
    if x.abs() < 1.0 {
        return Ok(if x >= 0.5 {
            1
        } else if x < -0.5 {
            -1
        } else {
            0
        });
    }
    let bits = x.to_bits();
    let negative = (bits >> 63) == 1;
    let biased = ((bits >> 52) & 0x7ff) as i32;
    let fraction = (bits & ((1u64 << 52) - 1)) as i128;
    let (mantissa, exponent) = if biased == 0 {
        (fraction, -1074)
    } else {
        (fraction | (1i128 << 52), biased - 1075)
    };
    let mantissa = if negative { -mantissa } else { mantissa };
    if exponent >= 0 {
        return Ok((mantissa << exponent) as i64);
    }
    let denominator: i128 = 1i128 << (-exponent);
    Ok((2 * mantissa + denominator).div_euclid(2 * denominator) as i64)
}

/// Python `two_point_gaussian`: A*exp(-x*x/C) about an already chosen center, strict C and A gates.
pub fn two_point_gaussian(
    first: (f64, f64),
    second: (f64, f64),
    height_cap: f64,
    denominator_limit: f64,
) -> Res<Option<(f64, f64)>> {
    let height_cap = bounded(&fv(height_cap), "height cap", 0.0, 1e12)?;
    let denominator_limit = bounded(&fv(denominator_limit), "Gaussian denominator limit", 1e-12, 1e12)?;
    let check = |p: (f64, f64)| -> Res<(f64, f64)> {
        Ok((
            bounded(&fv(p.0), "recentered scan", -1e8, 1e8)?,
            bounded(&fv(p.1), "point height", -1e12, 1e12)?,
        ))
    };
    let (x1, y1) = check(first)?;
    let (x2, y2) = check(second)?;
    if y1 <= 0.0 || y2 <= 0.0 {
        return Ok(None);
    }
    let (l1, l2) = (log(y1), log(y2));
    let delta = l2 - l1;
    if delta == 0.0 {
        return Ok(None);
    }
    let c = (x1 * x1 - x2 * x2) / delta;
    if !(0.0 < c && c < denominator_limit) {
        return Ok(None);
    }
    let a = crate::calibration::py_exp(l1 + x1 * x1 / c);
    // Python raises OverflowError here, which maps to no fit.
    if !a.is_finite() || !(0.0 < a && a <= height_cap) {
        return Ok(None);
    }
    Ok(Some((a, c)))
}

/// Result of `_collector` (one side of the peak): typed fields plus the Python dictionary order.
#[derive(Clone)]
pub struct Side {
    pub status: &'static str,
    pub intersection: Option<(i64, f64)>,
    pub maximum: Option<(i64, f64)>,
    pub consecutive: Vec<(i64, f64)>,
    pub gaussian: Vec<(i64, f64)>,
    pub last_scanned: Option<i64>,
    pub line_fit: Value,
    pub reason: Option<&'static str>,
    pub error: Option<String>,
}

impl Side {
    fn new() -> Side {
        Side {
            status: "no_crossover",
            intersection: None,
            maximum: None,
            consecutive: vec![],
            gaussian: vec![],
            last_scanned: None,
            line_fit: Value::Null,
            reason: None,
            error: None,
        }
    }

    /// Python dict of the side; `consecutive`/`gaussian` are kept (callers strip them where Python does).
    pub fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("status".into(), json!(self.status));
        m.insert("intersection".into(), self.intersection.map(pair).unwrap_or(Value::Null));
        m.insert("maximum".into(), self.maximum.map(pair).unwrap_or(Value::Null));
        m.insert("consecutive".into(), Value::Array(self.consecutive.iter().map(|&p| pair(p)).collect()));
        m.insert("gaussian".into(), Value::Array(self.gaussian.iter().map(|&p| pair(p)).collect()));
        m.insert("last_scanned".into(), self.last_scanned.map(|v| json!(v)).unwrap_or(Value::Null));
        m.insert("line_fit".into(), self.line_fit.clone());
        if let Some(r) = self.reason {
            m.insert("reason".into(), json!(r));
        }
        if let Some(e) = &self.error {
            m.insert("error".into(), json!(e));
        }
        Value::Object(m)
    }
}

/// Python `_collector`: walks from the half-height crossing and gathers the side's consecutive and Gaussian points.
pub fn collector(signal: &[f64], lo: i64, hi: i64, start: i64, step: i64, center: i64) -> Side {
    let mut result = Side::new();
    let mut positions = vec![start, start - step];
    let third = start - 2 * step;
    if (step > 0 && third >= center) || (step < 0 && third <= center) {
        positions.push(third);
    }
    if positions.iter().any(|&p| !(lo <= p && p <= hi)) {
        result.status = "unavailable";
        result.reason = Some("collector_line_points_outside_bounds");
        return result;
    }
    let (intercept, slope);
    if positions.len() == 2 {
        let s = (signal[positions[1] as usize] - signal[positions[0] as usize])
            / ((positions[1] - positions[0]) as f64);
        slope = s;
        intercept = signal[start as usize] - slope * (start as f64);
        result.line_fit = json!({"solver": "two_point_line", "coefficients": [fv(intercept), fv(slope)]});
    } else {
        let xs: Vec<f64> = positions.iter().map(|&p| p as f64).collect();
        let ys: Vec<f64> = positions.iter().map(|&p| signal[p as usize]).collect();
        match polynomial_fit(&xs, &ys, 1, 64) {
            Ok(fit) => {
                let c = fit["coefficients"].as_array().unwrap();
                intercept = c[0].as_f64().unwrap();
                slope = c[1].as_f64().unwrap();
                result.line_fit = fit;
            }
            Err(error) => {
                result.status = "unavailable";
                result.reason = Some("collector_polynomial_unavailable");
                result.error = Some(error);
                return result;
            }
        }
    }
    let mut index = start + step;
    let mut found = false;
    while lo <= index && index <= hi {
        result.last_scanned = Some(index);
        let value = signal[index as usize];
        if value > slope * (index as f64) + intercept || value <= 0.0 {
            if value <= 0.0 {
                return result;
            }
            found = true;
            break;
        }
        index += step;
    }
    if !found {
        return result;
    }
    let first = (index, signal[index as usize]);
    let mut maximum = first;
    let mut consecutive = vec![first];
    let mut gaussian = vec![first];
    let mut last_height = first.1;
    result.status = "collected";
    result.intersection = Some(first);
    index += step;
    while lo <= index && index <= hi && (consecutive.len() < 3 || gaussian.len() < 4) {
        result.last_scanned = Some(index);
        let point = (index, signal[index as usize]);
        if point.1 > maximum.1 {
            maximum = point;
        }
        if consecutive.len() < 3 {
            consecutive.push(point);
        }
        if gaussian.len() < 4 && 0.0 < point.1 && point.1 < last_height {
            gaussian.push(point);
            last_height = point.1;
        }
        index += step;
    }
    result.maximum = Some(maximum);
    result.consecutive = consecutive;
    result.gaussian = gaussian;
    result
}

#[derive(Clone, Copy)]
pub struct Model {
    pub height: f64,
    pub center: f64,
    pub denominator_scan2: f64,
}

/// Python `_evaluate`.
pub fn evaluate(model: &Model, scan: f64) -> f64 {
    let delta = scan - model.center;
    model.height * crate::calibration::py_exp(-(delta * delta) / model.denominator_scan2)
}

/// Python `_crossing`: first sample at or below `level` moving from `center`; truncated at the window edge.
fn crossing(signal: &[f64], lo: i64, hi: i64, center: i64, step: i64, level: f64) -> (i64, bool) {
    let mut index = center + step;
    while lo <= index && index <= hi {
        if signal[index as usize] <= level {
            return (index, false);
        }
        index += step;
    }
    (if step < 0 { lo } else { hi }, true)
}

/// Python `_true_width`: width at `fraction` of the model height, with linear crossing interpolation.
pub fn true_width(signal: &[f64], lo: i64, hi: i64, model: &Model, height: f64, fraction: f64) -> Res<Value> {
    let center = round_scan(model.center)?;
    let gate = |reason: Value| json!({"width_scan": fv(-1.0), "reason": reason, "rounded_center": center});
    if !(lo < center && center < hi && model.height < height) {
        return Ok(gate(json!("true_width_gate")));
    }
    let level = model.height * fraction;
    let (left, lc) = crossing(signal, lo, hi, center, -1, level);
    let (right, rc) = crossing(signal, lo, hi, center, 1, level);
    let mut l = left as f64;
    let mut r = right as f64;
    let difference = signal[(left + 1) as usize] - signal[left as usize];
    if !lc && difference > 0.0 {
        l += (level - signal[left as usize]) / difference;
    }
    let difference = signal[(right - 1) as usize] - signal[right as usize];
    if !rc && difference > 0.0 {
        r -= (level - signal[right as usize]) / difference;
    }
    let lw = center as f64 - l;
    let rw = r - center as f64;
    let width = if lc && !rc {
        2.0 * rw + 1.0
    } else if rc && !lc {
        2.0 * lw + 1.0
    } else {
        lw + rw + 1.0
    };
    Ok(json!({
        "width_scan": fv(width),
        "reason": Value::Null,
        "rounded_center": center,
        "threshold": fv(level),
        "crossings": [left, right],
        "interpolated_crossings": [fv(l), fv(r)],
        "truncated": [lc, rc],
    }))
}

/// Python `_model_from_sides`: centre from the maxima and an optional quadratic, then the accepted Gaussian pairs.
pub fn model_from_sides(
    left: &Side,
    right: &Side,
    lo: i64,
    hi: i64,
    height: f64,
    limit: f64,
) -> Res<(Option<Model>, Value)> {
    let mut candidate: Option<(f64, f64)> = right.maximum.map(|(i, h)| (i as f64, h));
    let mut source = if candidate.is_some() { "right_measured" } else { "midpoint" };
    if candidate.is_none() {
        candidate = left.maximum.map(|(i, h)| (i as f64, h));
        if candidate.is_some() {
            source = "left_measured";
        }
    } else if let Some((li, lh)) = left.maximum {
        if lh > candidate.unwrap().1 {
            candidate = Some((li as f64, lh));
            source = "left_measured";
        }
    }
    let mut quadratic: Value = Value::Null;
    let mut quadratic_reason: Value = json!("insufficient_two_sided_consecutive_points");
    if left.consecutive.len() > 1 && right.consecutive.len() > 1 {
        let points: Vec<(i64, f64)> = left.consecutive.iter().chain(right.consecutive.iter()).cloned().collect();
        let xs: Vec<f64> = points.iter().map(|p| p.0 as f64).collect();
        let ys: Vec<f64> = points.iter().map(|p| p.1).collect();
        match polynomial_fit(&xs, &ys, 2, 64) {
            Ok(fit) => {
                let c = fit["coefficients"].as_array().unwrap();
                let (a0, a1, a2) = (c[0].as_f64().unwrap(), c[1].as_f64().unwrap(), c[2].as_f64().unwrap());
                let mut q = json!({"fit": fit, "vertex": Value::Null, "accepted": false});
                if a2 != 0.0 {
                    let x = -a1 / (2.0 * a2);
                    let li = left.intersection.unwrap().0 as f64;
                    let ri = right.intersection.unwrap().0 as f64;
                    if x.is_finite() && li <= x && x <= ri {
                        let y = a2 * x * x + a1 * x + a0;
                        if y.is_finite() {
                            q["vertex"] = json!([fv(x), fv(y)]);
                            q["accepted"] = json!(true);
                            if candidate.is_none() || y > candidate.unwrap().1 {
                                candidate = Some((x, y));
                                source = "quadratic";
                            }
                        }
                    }
                }
                quadratic = q;
                quadratic_reason = Value::Null;
            }
            Err(_) => quadratic_reason = json!("quadratic_polynomial_unavailable"),
        }
    }
    let center = match candidate {
        None => ((lo + hi).div_euclid(2)) as f64,
        Some((x, _)) => x,
    };
    let mut merged: Vec<(i64, f64)> = Vec::new();
    for index in 0..left.gaussian.len().max(right.gaussian.len()) {
        for side in [left, right] {
            if index < side.gaussian.len() {
                merged.push(side.gaussian[index]);
            }
        }
    }
    let mut accepted: Vec<(i64, i64, f64, f64)> = Vec::new();
    let mut attempts: i64 = 0;
    if merged.len() > 2 {
        'outer: for i in 0..merged.len() {
            for j in i + 1..merged.len() {
                attempts += 1;
                let (a, b) = (merged[i], merged[j]);
                let fit = two_point_gaussian(
                    (a.0 as f64 - center, a.1),
                    (b.0 as f64 - center, b.1),
                    height,
                    limit,
                )?;
                if let Some((h, d)) = fit {
                    accepted.push((a.0, b.0, h, d));
                    if accepted.len() == 4 {
                        break 'outer;
                    }
                }
            }
        }
    }
    let model = if accepted.is_empty() {
        None
    } else {
        let n = accepted.len() as f64;
        let mut a = accepted[0].2;
        let mut c = accepted[0].3;
        for f in &accepted[1..] {
            a += f.2;
            c += f.3;
        }
        Some(Model { height: a / n, center, denominator_scan2: c / n })
    };
    let accepted_value: Vec<Value> = accepted
        .iter()
        .map(|&(i, j, h, d)| json!({"indices": [i, j], "height": fv(h), "denominator_scan2": fv(d)}))
        .collect();
    let fit = json!({
        "selected_center": fv(center),
        "center_source": source,
        "quadratic": quadratic,
        "quadratic_reason": quadratic_reason,
        "merged_indices": merged.iter().map(|p| p.0).collect::<Vec<i64>>(),
        "attempted_pairs": attempts,
        "accepted_pairs": accepted_value,
    });
    Ok((model, fit))
}

/// Python `_areas`: area under the fitted model between the relative intersections, plus truncated tails.
pub fn areas(
    signal: &[f64],
    lo: i64,
    hi: i64,
    selection: &crate::peak_morphology::Selection,
    whole: &[f64],
    model: &Model,
    left_intersection: Option<(i64, f64)>,
    right_intersection: Option<(i64, f64)>,
) -> Value {
    let missing = || {
        json!({"area": fv(-1.0), "excess_area_ratio": fv(-1.0), "reason": "relative_intersections_missing",
               "relative_intersections": Value::Null})
    };
    let origin = selection.origin;
    let top = selection.relative_center;
    let (lc, rc) = (selection.truncated[0], selection.truncated[1]);
    let mut left: Option<i64> = left_intersection.map(|p| p.0 - origin);
    let mut right: Option<i64> = right_intersection.map(|p| p.0 - origin);
    if lc && !rc && right.is_some() {
        left = Some(2 * top - right.unwrap());
    } else if rc && !lc && left.is_some() {
        right = Some(2 * top - left.unwrap());
    }
    let (l, r) = match (left, right) {
        (Some(l), Some(r)) => (l, r),
        _ => return missing(),
    };
    if whole.len() < 3 {
        let mut m = match missing() {
            Value::Object(m) => m,
            _ => unreachable!(),
        };
        m.insert("reason".into(), json!("extracted_support_shorter_than_three"));
        return Value::Object(m);
    }
    let mut total = 0.0f64;
    for &value in whole {
        total += value.abs();
    }
    let mut area = 0.0f64;
    for (index, &value) in whole.iter().enumerate() {
        let index = index as i64;
        area += if l < index && index < r {
            evaluate(model, (index + origin) as f64)
        } else {
            value.abs()
        };
    }
    let mut left_tail = 0.0f64;
    let mut right_tail = 0.0f64;
    if !lc {
        for index in lo..origin {
            left_tail += signal[index as usize].abs();
        }
        if rc {
            left_tail *= 2.0;
        }
        area += left_tail;
    }
    if !rc {
        for index in (whole.len() as i64 - 1 + origin)..hi {
            right_tail += signal[index as usize].abs();
        }
        if lc {
            right_tail *= 2.0;
        }
        area += right_tail;
    }
    let mut excess = -1.0f64;
    if total > 0.0 {
        excess = (selection.height - evaluate(model, selection.center as f64)) * ((r - l) as f64) / 2.0 / total;
    }
    json!({
        "area": fv(area),
        "excess_area_ratio": fv(excess),
        "reason": Value::Null,
        "relative_intersections": [l, r],
        "raw_extracted_area": fv(total),
        "left_tail_added": fv(left_tail),
        "right_tail_added": fv(right_tail),
    })
}

pub struct TruePeakDefinition {
    pub max_denominator_scan2: f64,
    pub near_base_fraction: f64,
}

fn model_value(m: &Model) -> Value {
    json!({"height": fv(m.height), "center": fv(m.center), "denominator_scan2": fv(m.denominator_scan2)})
}

/// Python `_measure_true_peak`: fresh true-peak model from both sides of the peak (schema 4 measurement).
pub fn measure_true_peak(
    signal: &[f64],
    lo: i64,
    hi: i64,
    selection: &crate::peak_morphology::Selection,
    whole: &[f64],
    definition: &TruePeakDefinition,
) -> Res<Value> {
    let mut result = Map::new();
    result.insert("status".into(), json!("unavailable"));
    result.insert("reason".into(), json!("no_accepted_gaussian_pairs"));
    result.insert("height".into(), fv(-1.0));
    result.insert("center".into(), fv(-1.0));
    result.insert("width_scan".into(), fv(-1.0));
    result.insert("area".into(), fv(-1.0));
    result.insert("excess_area_ratio".into(), fv(-1.0));
    result.insert("model".into(), Value::Null);
    result.insert("polynomial_solver".into(), json!(SOLVER_ID));
    result.insert("scope".into(), json!("fresh_baseline_corrected_companion_offset0"));
    result.insert("original_runtime_validated".into(), json!(false));
    let mut sides: Vec<Side> = Vec::new();
    for (index, step) in [-1i64, 1].into_iter().enumerate() {
        if selection.truncated[index] {
            let mut side = Side::new();
            side.status = "near_base_truncated";
            sides.push(side);
        } else {
            sides.push(collector(signal, lo, hi, selection.half_crossings[index], step, selection.center));
        }
    }
    let (left, right) = (&sides[0], &sides[1]);
    let (model, fit) = model_from_sides(left, right, lo, hi, selection.height, definition.max_denominator_scan2)?;
    let pair_or_null = |p: Option<(i64, f64)>| p.map(|v| json!([v.0, fv(v.1)])).unwrap_or(Value::Null);
    result.insert("fitting".into(), fit);
    result.insert("left_intersection".into(), pair_or_null(left.intersection));
    result.insert("right_intersection".into(), pair_or_null(right.intersection));
    let mut side_values = Vec::new();
    for side in &sides {
        // Keep Python key order: drop consecutive/gaussian without swap_remove (which reorders keys).
        let mut m = Map::new();
        if let Value::Object(full) = side.to_value() {
            for (k, v) in full {
                if k != "consecutive" && k != "gaussian" {
                    m.insert(k, v);
                }
            }
        }
        m.insert("consecutive_indices".into(), json!(side.consecutive.iter().map(|p| p.0).collect::<Vec<i64>>()));
        m.insert("gaussian_indices".into(), json!(side.gaussian.iter().map(|p| p.0).collect::<Vec<i64>>()));
        side_values.push(Value::Object(m));
    }
    result.insert("sides".into(), Value::Array(side_values));
    let model = match model {
        None => return Ok(Value::Object(result)),
        Some(m) => m,
    };
    let width = true_width(signal, lo, hi, &model, selection.height, definition.near_base_fraction)?;
    let area = areas(signal, lo, hi, selection, whole, &model, left.intersection, right.intersection);
    result.insert("status".into(), json!("evaluated"));
    result.insert("reason".into(), Value::Null);
    result.insert("height".into(), fv(model.height));
    result.insert("center".into(), fv(model.center));
    result.insert("width_scan".into(), width["width_scan"].clone());
    result.insert("area".into(), area["area"].clone());
    result.insert("excess_area_ratio".into(), area["excess_area_ratio"].clone());
    result.insert("model".into(), model_value(&model));
    result.insert("width_measurement".into(), width);
    result.insert("area_measurement".into(), area);
    Ok(Value::Object(result))
}
