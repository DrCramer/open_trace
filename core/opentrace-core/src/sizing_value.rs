//! Dict-level port of `sizing_pipeline.analyze_size_standard` and its stages (`prepare_trace`, autoheight,
//! SG peak dicts, `filter_standard_peaks`, `match_ratio_standard`, `evaluate_sizing_quality`).
//!
//! Unlike the struct-based modules this keeps the reference's JSON structure: key order, conditional keys and
//! partially filled results on rejection. Numeric kernels are reused from the struct ports.
use crate::analysis::Calibration;
use crate::autoheight::estimate_standard_autoheight;
use crate::fsum::fsum;
use crate::numeric::{py_median, py_sum};
use crate::preprocessing::{correct, estimate_baseline, smooth_trace};
use crate::ratio::{best_paths, MAX_CANDIDATES, MAX_EXPECTED, MAX_INPUT_PEAKS, SCORE_TOLERANCE};
use crate::sg_detect::detect_sg_peaks;
use crate::sizing_quality::chi_square_survival;
use crate::standard_wrapper::wrap_standard_peaks;
use serde_json::{json, Map, Value};

type Obj = Map<String, Value>;

pub fn fv(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

/// Finite non-bool real (Python `isinstance(Real)` minus bool, finite).
pub fn fnum(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Number(n)) => n.as_f64().filter(|f| f.is_finite()),
        _ => None,
    }
}

fn is_int_value(v: Option<&Value>) -> Option<i64> {
    match v {
        Some(Value::Number(n)) if n.is_i64() => n.as_i64(),
        _ => None,
    }
}

// ---------------------------------------------------------------- prepare_trace

const MAX_WINDOW: i64 = 100_001;

fn window_radius(w: i64) -> Result<usize, String> {
    if !(1..=MAX_WINDOW).contains(&w) || w % 2 != 1 {
        return Err("Window must be an odd integer in [1, 100001]".into());
    }
    Ok((w / 2) as usize)
}

pub struct Prepared {
    pub baseline: Vec<f64>,
    pub corrected: Vec<i64>,
    pub signal: Vec<i64>,
    pub parameters: Value,
}

pub fn prepare_trace(values: &[i64], window: i64, spike_window: i64, smoothing_window: i64, smooth_passes: i64) -> Result<Prepared, String> {
    if values.is_empty() || values.len() > 1_000_000 {
        return Err("Trace must be a nonempty bounded list or tuple".into());
    }
    if values.iter().any(|&v| !(-(1i64 << 31)..(1i64 << 31)).contains(&v)) {
        return Err("Trace values must be signed 32-bit integers".into());
    }
    let broad = window_radius(window)?;
    let narrow = window_radius(spike_window)?;
    let smooth = window_radius(smoothing_window)?;
    let baseline = estimate_baseline(values, broad, narrow, smooth);
    let corrected = correct(values, &baseline);
    if corrected.iter().any(|&x| x >= (1i64 << 31)) {
        return Err("Baseline-corrected trace exceeds signed 32-bit range".into());
    }
    if !(0..=32).contains(&smooth_passes) {
        return Err("Smoothing passes must be an integer in [0, 32]".into());
    }
    let signal = smooth_trace(&corrected, smooth_passes as usize);
    let parameters = json!({"window": window, "spike_window": spike_window, "smoothing_window": smoothing_window,
        "smooth_passes": smooth_passes, "edge_policy": "clipped centered support", "rounding": "nonnegative half up",
        "original_runtime_validated": false});
    Ok(Prepared { baseline, corrected, signal, parameters })
}

// ---------------------------------------------------------------- autoheight

/// Autoheight dict without `control_signal`, or the reference's ValueError text.
pub fn autoheight_value(signal: &[i64]) -> Result<Value, String> {
    let a = estimate_standard_autoheight(signal)?;
    let quarter = signal.len() / 4;
    Ok(json!({"threshold": fv(a.threshold), "median_control": fv(a.median_control),
        "parameters": {"floor_rfu": 30.0, "median_multiplier": 25.0, "first_average": 11, "quarter_window": quarter,
            "quarter_window_even": quarter % 2 == 0, "clipped_average_windows": [quarter, 21, 51, 101, 201, 21],
            "polynomial_window": 21},
        "original_runtime_validated": false, "external_validation": false}))
}

// ---------------------------------------------------------------- SG peaks

#[allow(clippy::too_many_arguments)]
pub fn sg_peaks_value(signal: &[i64], threshold: f64, window: i64, degree: i64, min_width: f64, tau_begin: f64, tau_end: f64,
                      offscale: &[(f64, f64)]) -> Result<(Vec<Value>, Value), String> {
    let w = if window < 0 { 0 } else { window as usize };
    let d = if degree < 0 { 0 } else { degree as usize };
    let (peaks, dg) = detect_sg_peaks(signal, threshold, w, d, min_width, tau_begin, tau_end, offscale)?;
    let list = peaks
        .iter()
        .map(|p| {
            json!({"scan": p.scan, "sg_scan": fv(p.sg_scan), "begin": fv(p.begin), "end": fv(p.end), "height": fv(p.height),
                "begin_height": fv(p.begin_height), "end_height": fv(p.end_height), "fwhm_scan": fv(p.fwhm_scan),
                "area": p.area, "offscaled": p.offscaled})
        })
        .collect();
    let diag = json!({"sg_candidates": dg.sg_candidates, "invalid_bounds": dg.invalid_bounds, "height_rejected": dg.height_rejected,
        "width_rejected": dg.width_rejected, "edge_rejected": dg.edge_rejected,
        "duplicate_snapped_scans": dg.duplicate_snapped_scans, "degree": degree, "window": window,
        "threshold": fv(threshold), "companion_metrics": false, "original_runtime_validated": false,
        "accepted_peaks": dg.accepted_peaks});
    Ok((list, diag))
}

// ---------------------------------------------------------------- prefilters

fn ratio_ieee(n: f64, d: f64) -> f64 {
    if d != 0.0 {
        n / d
    } else if n == 0.0 {
        f64::NAN
    } else {
        f64::INFINITY.copysign(n) * 1.0f64.copysign(d)
    }
}

struct Chk {
    index: usize,
    peak: Obj,
    scan: f64,
    height: f64,
    begin: f64,
    end: f64,
    bh: f64,
    eh: f64,
    fwhm: f64,
    off: bool,
}

fn idx_list(v: &[usize]) -> Value {
    Value::Array(v.iter().map(|&i| json!(i)).collect())
}

/// Python `filter_standard_peaks(peaks, expected_count, signal, group_gap=...)`.
pub fn filter_standard_peaks(peaks: &[Obj], expected_count: Option<i64>, signal: &[i64], group_gap: &Value) -> Value {
    let mut dg = Obj::new();
    dg.insert("input_count".into(), json!(peaks.len()));
    dg.insert("expected_count".into(), expected_count.map(|e| json!(e)).unwrap_or(Value::Null));
    dg.insert("group_gap".into(), group_gap.clone());
    dg.insert("early_stage_applied".into(), json!(false));
    for k in ["early_shape_removed_indices", "early_high_removed_indices", "early_prefix_removed_indices",
              "connected_tail_removed_indices", "group_removed_indices", "group_events"] {
        dg.insert(k.into(), json!([]));
    }
    dg.insert("original_runtime_validated".into(), json!(false));
    let mut reasons: Vec<String> = Vec::new();
    let finish = |status: &str, out: Vec<Value>, reasons: Vec<String>, dg: Obj| -> Value {
        json!({"status": status, "peaks": out, "reasons": reasons, "diagnostics": Value::Object(dg)})
    };
    macro_rules! reject {
        ($m:expr) => {{
            reasons.push($m.to_string());
            return finish("rejected", vec![], reasons, dg);
        }};
    }
    let expected = match expected_count {
        Some(e) if (3..=32).contains(&e) => e as usize,
        _ => reject!("expected_count must be an integer in [3, 32]"),
    };
    let gg = match fnum(Some(group_gap)) {
        Some(g) if (0.0..=100_000_000.0).contains(&g) => g,
        _ => reject!("group_gap must be finite and in [0, 100000000]"),
    };
    if signal.is_empty() || signal.len() > 1_000_000 {
        reject!("signal must be a nonempty bounded list or tuple");
    }
    if signal.iter().any(|&v| !(-(1i64 << 31)..(1i64 << 31)).contains(&v)) {
        reject!("signal must contain signed 32-bit integers");
    }
    if peaks.len() > 4096 {
        reject!("peaks must be an explicit list or tuple of at most 4096 peaks");
    }
    let mut checked: Vec<Chk> = Vec::new();
    for (index, src) in peaks.iter().enumerate() {
        let g = |k: &str| fnum(src.get(k));
        let (scan, height, begin, end, bh, eh, fwhm) = match (g("scan"), g("height"), g("begin"), g("end"), g("begin_height"), g("end_height"), g("fwhm_scan")) {
            (Some(a), Some(b), Some(c), Some(d), Some(e), Some(f), Some(h)) => (a, b, c, d, e, f, h),
            _ => reject!(format!("Peak {}: finite numeric morphology fields required", index)),
        };
        let off = match src.get("offscaled") {
            None => Some(false),
            Some(Value::Bool(b)) => Some(*b),
            _ => None,
        };
        let ok = 0.0 <= begin && begin <= scan && scan <= end && end < signal.len() as f64
            && 0.0 < height && height <= 1e12 && 0.0 <= fwhm && fwhm <= 1_000_000.0
            && bh.abs() <= 1e12 && eh.abs() <= 1e12 && off.is_some();
        if !ok {
            reject!(format!("Peak {}: invalid bounded morphology or offscaled flag", index));
        }
        if let Some(last) = checked.last() {
            if scan <= last.scan {
                reject!("Peak scans must increase strictly; input is not sorted implicitly");
            }
        }
        checked.push(Chk { index, peak: src.clone(), scan, height, begin, end, bh, eh, fwhm, off: off.unwrap() });
    }
    if checked.len() < expected {
        reject!("Fewer candidates than the full expected standard before prefilters");
    }
    let cutoff = (checked.last().unwrap().scan + 3.0 * checked[0].scan) / 4.0;
    dg.insert("early_cutoff_scan".into(), fv(cutoff));
    let late: Vec<&Chk> = checked.iter().filter(|c| c.scan >= cutoff).collect();
    dg.insert("late_reference_count".into(), json!(late.len()));
    let mut survivors: Vec<&Chk> = checked.iter().collect();
    let mut shape_removed = vec![];
    let mut high_removed = vec![];
    let mut prefix_removed = vec![];
    let mut connected_removed = vec![];
    if late.len() as f64 >= 0.75 * expected as f64 {
        dg.insert("early_stage_applied".into(), json!(true));
        let mut reference: Vec<&Chk> = late.clone();
        reference.sort_by(|a, b| (-a.height).partial_cmp(&(-b.height)).unwrap());
        reference.truncate(expected);
        let nref = reference.len() as f64;
        let mean_height = py_sum(reference.iter().map(|p| p.height)) / nref;
        let mean_begin = py_sum(reference.iter().map(|p| p.bh)) / nref;
        let mean_width = py_sum(reference.iter().map(|p| p.fwhm)) / nref;
        dg.insert("reference_means".into(), json!({"height": fv(mean_height), "begin_height": fv(mean_begin), "fwhm_scan": fv(mean_width)}));
        let mut retained: Vec<&Chk> = Vec::new();
        let (mut last_large_scan, mut last_large): (f64, Option<&Chk>) = (0.0, None);
        let mut last_large_value: Value = json!(0.0);
        for p in &checked {
            if p.scan > cutoff {
                retained.push(p);
                continue;
            }
            let (h, b, e) = (p.height, p.bh, p.eh);
            let shape = b / h > 0.7 || e / h > 0.7 || (b - e).abs() / h > 0.5;
            let update_large;
            if shape {
                shape_removed.push(p.index);
                update_large = h > 2.0 * mean_height;
            } else if h > 10.0 * mean_height || (p.fwhm > 2.0 * mean_width && h > 5.0 * mean_height) {
                high_removed.push(p.index);
                update_large = true;
            } else {
                retained.push(p);
                update_large = false;
            }
            if update_large && p.scan > last_large_scan {
                last_large_scan = p.scan;
                last_large_value = p.peak.get("scan").cloned().unwrap_or(Value::Null);
                last_large = Some(p);
            }
        }
        survivors = Vec::new();
        for item in &retained {
            if item.scan > last_large_scan {
                survivors.push(*item);
            } else {
                prefix_removed.push(item.index);
            }
        }
        dg.insert("last_large_early_scan".into(), last_large_value);
        if let (Some(first), Some(ll)) = (survivors.first().copied(), last_large) {
            if ratio_ieee(first.height - first.bh, first.height - first.eh) > 0.5 && ratio_ieee(first.bh, mean_begin) > 5.0 {
                let (previous_end, current_begin) = (ll.end, first.begin);
                let connected = if previous_end == current_begin {
                    true
                } else {
                    let start = previous_end.trunc() as i64;
                    let count = (current_begin - previous_end + 1.0).trunc() as i64;
                    if count < 0 || start < 0 || start + count > signal.len() as i64 {
                        dg.insert("connectivity_error".into(), json!({"start": start, "count": count, "previous_index": ll.index, "current_index": first.index}));
                        dg.insert("early_shape_removed_indices".into(), idx_list(&shape_removed));
                        dg.insert("early_high_removed_indices".into(), idx_list(&high_removed));
                        dg.insert("early_prefix_removed_indices".into(), idx_list(&prefix_removed));
                        reject!("Malformed connected-tail signal interval; filtering withheld");
                    }
                    signal[start as usize..(start + count) as usize].iter().all(|&v| v as f64 >= first.bh)
                };
                if connected {
                    connected_removed.push(first.index);
                    survivors.remove(0);
                }
            }
        }
    } else {
        dg.insert("early_stage_skip_reason".into(), json!("Fewer than 0.75*expected_count late reference peaks"));
    }
    dg.insert("early_shape_removed_indices".into(), idx_list(&shape_removed));
    dg.insert("early_high_removed_indices".into(), idx_list(&high_removed));
    dg.insert("early_prefix_removed_indices".into(), idx_list(&prefix_removed));
    dg.insert("connected_tail_removed_indices".into(), idx_list(&connected_removed));
    dg.insert("after_early_count".into(), json!(survivors.len()));

    let n = survivors.len();
    let right_shape: Vec<bool> = survivors.iter().map(|p| p.eh / p.height > 0.7 || (p.eh - p.bh) / p.height > 0.5).collect();
    let left_shape: Vec<bool> = survivors.iter().map(|p| p.bh / p.height > 0.7 || (p.bh - p.eh) / p.height > 0.5).collect();
    let mut examined = vec![false; n];
    let mut n_examined = 0usize;
    let mut removed = vec![false; n];
    let mut remaining = n;
    let mut group_removed: Vec<usize> = vec![];
    let mut events: Vec<Value> = vec![];
    while remaining > expected && n_examined < n {
        // max over unexamined, first maximal height wins (Python max keeps the first).
        let mut dominant = usize::MAX;
        for i in 0..n {
            if examined[i] {
                continue;
            }
            if dominant == usize::MAX || survivors[i].height > survivors[dominant].height {
                dominant = i;
            }
        }
        let peak = survivors[dominant];
        let mut chosen: Option<(usize, &str)> = None;
        for (neighbor, direction, flags) in [(dominant as i64 - 1, "left", &right_shape), (dominant as i64 + 1, "right", &left_shape)] {
            if neighbor < 0 || neighbor >= n as i64 || examined[neighbor as usize] {
                continue;
            }
            let nb = neighbor as usize;
            let other = survivors[nb];
            let shape_smaller = flags[nb] && other.height < peak.height;
            let lower_ratio = other.height / peak.height < 0.6;
            if (other.scan - peak.scan).abs() < gg && (shape_smaller || lower_ratio) && !peak.off {
                chosen = Some((nb, direction));
                break;
            }
        }
        if let Some((c, side)) = chosen {
            removed[c] = true;
            examined[c] = true;
            n_examined += 1;
            remaining -= 1;
            group_removed.push(survivors[c].index);
            events.push(json!({"dominant_index": peak.index, "removed_index": survivors[c].index, "side": side}));
        }
        examined[dominant] = true;
        n_examined += 1;
    }
    // `examined` is a set in Python: chosen neighbors were added before the dominant, counts are distinct.
    dg.insert("group_removed_indices".into(), idx_list(&group_removed));
    dg.insert("group_events".into(), Value::Array(events));
    let output: Vec<Value> = survivors.iter().enumerate().filter(|(i, _)| !removed[*i]).map(|(_, p)| Value::Object(p.peak.clone())).collect();
    dg.insert("output_count".into(), json!(output.len()));
    dg.insert("below_expected_after_filtering".into(), json!(output.len() < expected));
    dg.insert("group_examined_count".into(), json!(n_examined));
    finish("passed", output, reasons, dg)
}

// ---------------------------------------------------------------- ratio matcher

pub struct RatioOut {
    pub result: Value,
    pub calibration: Option<Calibration>,
}

#[allow(clippy::too_many_arguments)]
pub fn match_ratio_standard(peaks: &[Obj], expected_bp: &Value, num_extra_peaks: i64, ratio_factor: f64, min_accepted_score: f64,
                            ambiguity_margin: f64, allow_prefix: bool, max_bp_per_scan: &Value) -> RatioOut {
    let mut diag = Obj::new();
    diag.insert("method".into(), json!("height_spacing_ratio_second_order_dp"));
    diag.insert("score_is_original_sq".into(), json!(false));
    diag.insert("external_validation".into(), json!(false));
    diag.insert("transitions_checked".into(), json!(0));
    diag.insert("score_comparison_tolerance".into(), fv(SCORE_TOLERANCE));
    let mut reasons: Vec<String> = vec![];
    let mut matched: Vec<Value> = vec![];
    let mut selection = Obj::new();
    let mut attempts: Vec<Value> = vec![];
    let mut anchors: Vec<Value> = vec![];
    let mut calibration: Option<Calibration> = None;
    let mut status = "rejected";
    macro_rules! out {
        () => {{
            let calib = calibration.clone();
            return RatioOut {
                result: json!({"status": status, "reasons": reasons, "anchors": anchors, "calibration": Value::Null,
                    "matched": matched, "selection": Value::Object(selection), "attempts": attempts,
                    "diagnostics": Value::Object(diag)}),
                calibration: calib,
            };
        }};
    }
    macro_rules! reject {
        ($m:expr) => {{
            reasons.push($m.to_string());
            out!();
        }};
    }
    if !(0.0..=1.0).contains(&ratio_factor) || !ratio_factor.is_finite() {
        reject!("ratio_factor must be finite and in [0, 1]");
    }
    if !min_accepted_score.is_finite() || !(min_accepted_score > 0.0 && min_accepted_score <= 1.0) {
        reject!("min_accepted_score must be finite and in (0, 1]");
    }
    if !ambiguity_margin.is_finite() || !(0.0..=1.0).contains(&ambiguity_margin) {
        reject!("ambiguity_margin must be finite and in [0, 1]");
    }
    let max_rate: Option<f64> = match max_bp_per_scan {
        Value::Null => None,
        v => match fnum(Some(v)) {
            Some(m) if m > 0.0 && m <= 100.0 => Some(m),
            _ => reject!("max_bp_per_scan must be None or finite and in (0, 100]"),
        },
    };
    if !(0..=32).contains(&num_extra_peaks) {
        reject!("num_extra_peaks must be an integer in [0, 32]");
    }
    let exp = match expected_bp {
        Value::Array(a) if (3..=MAX_EXPECTED).contains(&a.len()) => a,
        _ => reject!("Expected 3..32 explicit fragment sizes"),
    };
    let mut bp: Vec<f64> = Vec::new();
    for x in exp {
        match fnum(Some(x)) {
            Some(f) if f > 0.0 && f <= 1_000_000.0 => bp.push(f),
            _ => reject!("Expected fragment sizes must be finite and in (0, 1000000]"),
        }
    }
    if bp.windows(2).any(|w| w[1] - w[0] < 1e-6) {
        reject!("Expected fragment sizes must increase with gaps >= 0.000001 bp");
    }
    if peaks.is_empty() || peaks.len() > MAX_INPUT_PEAKS {
        reject!("Candidates must be an explicit list or tuple of 1..4096 peaks");
    }
    let mut cands: Vec<(usize, f64, f64)> = Vec::new();
    for (index, p) in peaks.iter().enumerate() {
        match (fnum(p.get("scan")), fnum(p.get("height"))) {
            (Some(s), Some(h)) if (0.0..=100_000_000.0).contains(&s) && h > 0.0 && h <= 1e12 => cands.push((index, s, h)),
            _ => reject!(format!("Candidate {}: bounded finite nonnegative scan and positive height required", index)),
        }
    }
    cands.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    if cands.windows(2).any(|w| w[1].1 - w[0].1 < 1e-6) {
        reject!("Candidate scans must increase with gaps >= 0.000001 scans");
    }
    let expected_count = bp.len();
    if cands.len() < expected_count {
        reject!("Fewer candidates than the full expected standard");
    }
    let limit = expected_count + num_extra_peaks as usize;
    if limit > MAX_CANDIDATES {
        reject!("Selected candidate limit exceeds 64");
    }
    let n = cands.len();
    let mut ranked: Vec<usize> = (0..n).collect();
    ranked.sort_by(|&a, &b| cands[a].2.partial_cmp(&cands[b].2).unwrap().then(a.cmp(&b)));
    let cutoff = cands[ranked[n - expected_count]].2;
    let take = limit.min(n);
    let mut chosen: Vec<usize> = ranked[n - take..].to_vec();
    chosen.sort_unstable();
    let sel: Vec<(usize, f64, f64)> = chosen.iter().map(|&i| cands[i]).collect();
    selection.insert("input_count".into(), json!(n));
    selection.insert("selected_count".into(), json!(sel.len()));
    selection.insert("expected_count".into(), json!(expected_count));
    selection.insert("extra_peaks".into(), json!(num_extra_peaks));
    selection.insert("height_clip".into(), fv(cutoff));
    selection.insert("policy".into(), json!("explicit tallest N+extra; later index wins cutoff ties"));
    selection.insert("candidate_indices".into(), Value::Array(sel.iter().map(|s| json!(s.0)).collect()));
    let heights: Vec<f64> = sel.iter().map(|s| s.2.min(cutoff)).collect();
    let scans: Vec<f64> = sel.iter().map(|s| s.1).collect();
    let lower = if allow_prefix && ratio_factor < 1.0 {
        2
    } else if allow_prefix {
        3
    } else {
        expected_count
    };
    let mut transitions_total: u64 = 0;
    let mut length = expected_count;
    let choices = loop {
        let (c, tr) = best_paths(&scans, &heights, &bp[..length], ratio_factor);
        transitions_total += tr;
        diag.insert("transitions_checked".into(), json!(transitions_total));
        let best = c[0].0;
        attempts.push(json!({"expected_prefix_count": length, "best_score": fv(best)}));
        if best + SCORE_TOLERANCE >= min_accepted_score || !allow_prefix || length == lower {
            break c;
        }
        length -= 1;
    };
    let (best, path) = (choices[0].0, choices[0].1.clone());
    let second = choices.get(1).map(|c| c.0);
    let gap = second.map(|s| best - s);
    matched = path
        .iter()
        .enumerate()
        .map(|(k, &index)| json!({"candidate_index": sel[index].0, "scan": fv(sel[index].1), "height": fv(sel[index].2), "expected_bp": fv(bp[k])}))
        .collect();
    let rate = (bp[path.len() - 1] - bp[0]) / (scans[*path.last().unwrap()] - scans[path[0]]);
    diag.insert("best_score".into(), fv(best));
    diag.insert("second_best_score".into(), second.map(fv).unwrap_or(Value::Null));
    diag.insert("ambiguity_gap".into(), gap.map(fv).unwrap_or(Value::Null));
    diag.insert("ratio_factor".into(), fv(ratio_factor));
    diag.insert("min_accepted_score".into(), fv(min_accepted_score));
    diag.insert("ambiguity_margin".into(), fv(ambiguity_margin));
    diag.insert("complete".into(), json!(path.len() == expected_count));
    diag.insert("global_bp_per_scan".into(), fv(rate));
    diag.insert("max_bp_per_scan".into(), max_bp_per_scan.clone());
    if best + SCORE_TOLERANCE < min_accepted_score {
        reject!("No attempted prefix reaches the score threshold");
    }
    if path.len() != expected_count {
        reject!("Only an incomplete expected prefix reaches the score threshold; calibration withheld");
    }
    if let Some(g) = gap {
        if g <= ambiguity_margin + SCORE_TOLERANCE {
            reject!("Ambiguous ratio assignments; calibration withheld");
        }
    }
    if let Some(m) = max_rate {
        if rate > m {
            reject!("Assignment exceeds the explicit migration-rate bound; calibration withheld");
        }
    }
    let pairs: Vec<(f64, f64)> = path.iter().enumerate().map(|(k, &i)| (sel[i].1, bp[k])).collect();
    anchors = pairs.iter().map(|&(s, b)| json!([fv(s), fv(b)])).collect();
    calibration = Calibration::new(&pairs).ok();
    status = "accepted";
    out!()
}

// ---------------------------------------------------------------- sizing quality

fn bounded(v: Option<&Value>, min: f64, max: f64, positive: bool) -> Option<f64> {
    let f = fnum(v)?;
    if !(min <= f && f <= max) || (positive && f <= 0.0) {
        None
    } else {
        Some(f)
    }
}

struct Sq {
    index: usize,
    scan: f64,
    height: f64,
    bh: f64,
    eh: f64,
    bp: Option<f64>,
}

pub fn evaluate_sizing_quality(peaks: &[Obj], expected_bp: &Value) -> Value {
    let mut r = Obj::new();
    r.insert("status".into(), json!("rejected"));
    r.insert("score".into(), Value::Null);
    r.insert("raw_score".into(), Value::Null);
    r.insert("penalty_factor".into(), json!(1.0));
    r.insert("reasons".into(), json!([]));
    r.insert("events".into(), json!([]));
    r.insert("complete".into(), json!(false));
    r.insert("original_count_gate_passed".into(), json!(false));
    r.insert("matched_count".into(), json!(0));
    r.insert("expected_count".into(), json!(0));
    r.insert("chi_square_statistic".into(), Value::Null);
    r.insert("degrees_of_freedom".into(), Value::Null);
    r.insert("chi_square_survival".into(), Value::Null);
    r.insert("global_bp_per_scan".into(), Value::Null);
    r.insert("original_runtime_validated".into(), json!(false));
    r.insert("external_validation".into(), json!(false));
    r.insert("score_is_original_sq".into(), json!(false));
    r.insert("score_rounding".into(), json!("nonnegative_half_up_4_decimal_places"));
    r.insert("method".into(), json!("independent_morphology_penalty_times_chi_square_survival"));
    fn push(r: &mut Obj, key: &str, v: Value) {
        if let Some(Value::Array(a)) = r.get_mut(key) {
            a.push(v);
        }
    }
    macro_rules! invalid {
        ($m:expr) => {{
            push(&mut r, "reasons", json!($m));
            return Value::Object(r);
        }};
    }
    macro_rules! zero {
        ($m:expr) => {{
            r.insert("status".into(), json!("evaluated"));
            r.insert("score".into(), json!(0.0));
            r.insert("raw_score".into(), json!(0.0));
            push(&mut r, "reasons", json!($m));
            return Value::Object(r);
        }};
    }
    let exp = match expected_bp {
        Value::Array(a) if (3..=MAX_EXPECTED).contains(&a.len()) => a,
        _ => invalid!("expected_bp must contain3..32 explicit increasing sizes"),
    };
    let expected: Vec<Option<f64>> = exp.iter().map(|v| bounded(Some(v), 0.0, 1_000_000.0, true)).collect();
    if expected.iter().any(|v| v.is_none()) {
        invalid!("expected_bp requires finite positive values and gaps>=0.000001bp");
    }
    let expected: Vec<f64> = expected.into_iter().map(|v| v.unwrap()).collect();
    if expected.windows(2).any(|w| w[1] - w[0] < 1e-6) {
        invalid!("expected_bp requires finite positive values and gaps>=0.000001bp");
    }
    r.insert("expected_count".into(), json!(expected.len()));
    if peaks.len() > 4096 {
        invalid!("peaks must be a list or tuple of at most4096 mappings");
    }
    let mut norm: Vec<Sq> = Vec::new();
    for (index, p) in peaks.iter().enumerate() {
        let scan = bounded(p.get("scan"), 0.0, 100_000_000.0, false);
        let height = bounded(p.get("height"), 0.0, 1e12, true);
        let bh = bounded(p.get("begin_height"), -1e12, 1e12, false);
        let eh = bounded(p.get("end_height"), -1e12, 1e12, false);
        let (scan, height, bh, eh) = match (scan, height, bh, eh) {
            (Some(a), Some(b), Some(c), Some(d)) => (a, b, c, d),
            _ => invalid!(format!("peak{} has invalid scan/height/boundary features", index)),
        };
        let bp = match p.get("matched_bp") {
            None | Some(Value::Null) => None,
            Some(v) => match bounded(Some(v), 0.0, 1_000_000.0, true) {
                Some(f) => Some(f),
                None => invalid!(format!("peak{} has invalid matched_bp", index)),
            },
        };
        norm.push(Sq { index, scan, height, bh, eh, bp });
    }
    if norm.windows(2).any(|w| w[1].scan - w[0].scan < 1e-6) {
        invalid!("input peaks must already have strictly increasing scans with gaps>=0.000001");
    }
    let matched: Vec<&Sq> = norm.iter().filter(|p| p.bp.is_some()).collect();
    r.insert("matched_count".into(), json!(matched.len()));
    r.insert("original_count_gate_passed".into(), json!(matched.len() >= expected.len()));
    if matched.len() < expected.len() {
        zero!("incomplete_standard");
    }
    if matched.iter().map(|p| p.bp.unwrap()).collect::<Vec<_>>() != expected {
        zero!("matched_sizes_do_not_equal_expected_standard");
    }
    r.insert("complete".into(), json!(true));
    let mut penalty = 1.0f64;
    macro_rules! apply {
        ($kind:expr, $peak:expr, $factor:expr) => {{
            penalty *= $factor;
            push(&mut r, "events", json!({"kind": $kind, "peak_index": $peak.index, "factor": fv($factor)}));
            r.insert("penalty_factor".into(), fv(penalty));
        }};
    }
    for field in ["begin_height", "end_height"] {
        let get = |p: &Sq| if field == "begin_height" { p.bh } else { p.eh };
        let average = fsum(matched.iter().map(|p| get(p))) / matched.len() as f64;
        for p in &matched {
            let v = get(p);
            if v > 10.0 * average && v / p.height > 0.25 {
                apply!(format!("{}_outlier", field), p, 0.5f64);
            }
        }
    }
    let center = py_median(&matched.iter().map(|p| p.height).collect::<Vec<_>>());
    for p in &matched {
        if p.height > 8.0 * center {
            apply!("height_above_8_medians", p, 0.5f64);
        }
    }
    for w in matched.windows(2) {
        if w[1].height / w[0].height > 10.0 {
            zero!("directed_neighbor_height_ratio_above10");
        }
    }
    for w in matched.windows(2) {
        let (left, right) = (w[0], w[1]);
        let average = (left.height + right.height) / 2.0;
        for p in &norm[left.index + 1..right.index] {
            if p.bp.is_some() {
                continue;
            }
            if p.height > 5.0 * average {
                zero!("unmatched_between_anchors_above5_mean");
            }
            if p.height > 0.7 * average {
                apply!("unmatched_between_anchors_above0_7_mean", p, 0.9f64);
            }
        }
    }
    let (first, last) = (matched[0], matched[matched.len() - 1]);
    let rate = (last.bp.unwrap() - first.bp.unwrap()) / (last.scan - first.scan);
    r.insert("global_bp_per_scan".into(), fv(rate));
    if rate > 0.25 {
        zero!("global_migration_rate_above0_25");
    }
    let statistic = fsum(matched.windows(2).map(|w| {
        let (l, rt) = (w[0], w[1]);
        let d = rate * (rt.scan - l.scan) - (rt.bp.unwrap() - l.bp.unwrap());
        crate::numeric::py_pow(d, 2.0) / (rt.bp.unwrap() - l.bp.unwrap())
    }));
    let degrees = matched.len() - 2;
    let survival = match chi_square_survival(statistic, degrees) {
        Ok(v) => v,
        Err(_) => invalid!("chi_square_numerical_evaluation_failed"),
    };
    let raw = (penalty * survival).min(1.0).max(0.0);
    r.insert("status".into(), json!("evaluated"));
    r.insert("score".into(), fv(((raw * 10000.0 + 0.5).floor()) / 10000.0));
    r.insert("raw_score".into(), fv(raw));
    r.insert("chi_square_statistic".into(), fv(statistic));
    r.insert("degrees_of_freedom".into(), json!(degrees));
    r.insert("chi_square_survival".into(), fv(survival));
    Value::Object(r)
}

// ---------------------------------------------------------------- pipeline

pub struct SizeParams {
    pub baseline_window: i64,
    pub spike_window: i64,
    pub baseline_smoothing_window: i64,
    pub smooth_passes: i64,
    pub detection_threshold: f64,
    pub sg_window: i64,
    pub min_width: f64,
    pub tau_begin: f64,
    pub tau_end: f64,
    pub group_gap: Value,
    pub ratio_factor: f64,
    pub min_accepted_score: f64,
    pub ambiguity_margin: f64,
    pub allow_prefix: bool,
    pub max_bp_per_scan: Value,
    pub offscale_intervals: Value,
    pub height_mode: String,
    pub coordinate_mode: String,
    pub sg_degree: i64,
    /// The trace holds Python floats: `prepare_trace` rejects it with the signed-32-bit-integer message.
    pub float_trace: bool,
}

impl SizeParams {
    /// Python defaults of `analyze_size_standard`.
    pub fn defaults() -> SizeParams {
        SizeParams {
            baseline_window: 51,
            spike_window: 5,
            baseline_smoothing_window: 1,
            smooth_passes: 2,
            detection_threshold: 100.0,
            sg_window: 15,
            min_width: 2.0,
            tau_begin: 0.0,
            tau_end: 0.0,
            group_gap: json!(25),
            ratio_factor: 0.8,
            min_accepted_score: 0.75,
            ambiguity_margin: 0.01,
            allow_prefix: true,
            max_bp_per_scan: json!(0.25),
            offscale_intervals: json!([]),
            height_mode: "explicit".into(),
            coordinate_mode: "ap_fractional".into(),
            sg_degree: 3,
            float_trace: false,
        }
    }
}

pub struct SizeOut {
    pub result: Value,
    pub calibration: Option<Calibration>,
}

fn as_objs(v: &Value) -> Vec<Obj> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_object().cloned()).collect()).unwrap_or_default()
}

pub fn analyze_size_standard(values: &[i64], expected_bp: &Value, num_extra_peaks: i64, quality_threshold: f64, p: &SizeParams) -> SizeOut {
    let mut res = Obj::new();
    res.insert("status".into(), json!("rejected"));
    res.insert("reasons".into(), json!([]));
    res.insert("stage".into(), json!("validation"));
    res.insert("anchors".into(), json!([]));
    res.insert("calibration".into(), Value::Null);
    res.insert("peaks".into(), json!([]));
    res.insert("selected_peaks".into(), json!([]));
    for k in ["preparation", "detector", "filtering", "matching", "base_quality", "quality_threshold"] {
        res.insert(k.into(), Value::Null);
    }
    res.insert("height_mode".into(), json!(p.height_mode));
    res.insert("autoheight".into(), Value::Null);
    res.insert("coordinate_mode".into(), json!(p.coordinate_mode));
    res.insert("coordinate_conversion".into(), Value::Null);
    res.insert("external_validation".into(), json!(false));
    res.insert("original_runtime_validated".into(), json!(false));
    let mut not_applied: Vec<&str> = vec![
        "original autoheight", "normalization", "companion/jag/near-base features", "wrapper integer coordinate conversion",
        "ay broad-width multiplier", "SQ override", "original UI categories",
    ];
    fn add_reason(res: &mut Obj, r: &str) {
        if let Some(Value::Array(a)) = res.get_mut("reasons") {
            a.push(json!(r));
        }
    }
    let finish = |mut res: Obj, not_applied: &Vec<&str>, cal: Option<Calibration>| -> SizeOut {
        res.insert("not_applied".into(), json!(not_applied));
        SizeOut { result: Value::Object(res), calibration: cal }
    };
    if !(quality_threshold.is_finite() && quality_threshold > 0.0 && quality_threshold <= 1.0) {
        add_reason(&mut res, "An explicit finite quality_threshold in (0, 1] is required");
        return finish(res, &not_applied, None);
    }
    res.insert("quality_threshold".into(), fv(quality_threshold));
    if p.height_mode != "explicit" && p.height_mode != "basic_standard_auto" {
        add_reason(&mut res, "height_mode must be explicit or basic_standard_auto");
        return finish(res, &not_applied, None);
    }
    if p.coordinate_mode != "ap_fractional" && p.coordinate_mode != "standard_wrapper" {
        add_reason(&mut res, "coordinate_mode must be ap_fractional or standard_wrapper");
        return finish(res, &not_applied, None);
    }
    res.insert("stage".into(), json!("preparation"));
    let prepared = match (if p.float_trace && !values.is_empty() && values.len() <= 1_000_000 {
        Err("Trace values must be signed 32-bit integers".to_string())
    } else {
        prepare_trace(values, p.baseline_window, p.spike_window, p.baseline_smoothing_window, p.smooth_passes)
    }) {
        Ok(x) => x,
        Err(e) => {
            add_reason(&mut res, &e);
            return finish(res, &not_applied, None);
        }
    };
    res.insert("preparation".into(), prepared.parameters.clone());
    let signal = prepared.signal;
    res.insert("stage".into(), json!("detection"));
    let mut threshold = p.detection_threshold;
    if p.height_mode == "basic_standard_auto" {
        res.insert("stage".into(), json!("autoheight"));
        let ah = match autoheight_value(&signal) {
            Ok(a) => a,
            Err(e) => {
                add_reason(&mut res, &e);
                return finish(res, &not_applied, None);
            }
        };
        threshold = ah["threshold"].as_f64().unwrap();
        res.insert("autoheight".into(), ah);
        not_applied.retain(|s| *s != "original autoheight");
        res.insert("stage".into(), json!("detection"));
    }
    // Offscale intervals (validated by the detector in ap_fractional mode only).
    let mut iv: Vec<(f64, f64)> = Vec::new();
    let mut iv_ok = true;
    if let Value::Array(a) = &p.offscale_intervals {
        for x in a {
            match x {
                Value::Array(t) if t.len() == 2 => match (fnum(t.first()), fnum(t.get(1))) {
                    (Some(a), Some(b)) => iv.push((a, b)),
                    _ => iv_ok = false,
                },
                _ => iv_ok = false,
            }
        }
    }
    let det_iv: Vec<(f64, f64)> = if p.coordinate_mode == "ap_fractional" { iv.clone() } else { vec![] };
    if p.coordinate_mode == "ap_fractional" && !iv_ok {
        // Same message the detector gives for a malformed interval.
        let long = p.offscale_intervals.as_array().map(|a| a.len() > 10000).unwrap_or(true);
        // Threshold/width/tau checks precede the interval checks in the reference.
        let pre = detect_sg_peaks(&signal, threshold, 0, 0, p.min_width, p.tau_begin, p.tau_end, &[]);
        let msg = match pre {
            Err(e) if e.starts_with("Threshold") || e.starts_with("Minimum") || e.starts_with("Slope") => e,
            _ if long => "Offscale intervals must be a bounded list or tuple".to_string(),
            _ => "Offscale intervals must lie within the selected signal".to_string(),
        };
        add_reason(&mut res, &msg);
        return finish(res, &not_applied, None);
    }
    let (peaks, dg) = match sg_peaks_value(&signal, threshold, p.sg_window, p.sg_degree, p.min_width, p.tau_begin, p.tau_end, &det_iv) {
        Ok(x) => x,
        Err(e) => {
            add_reason(&mut res, &e);
            return finish(res, &not_applied, None);
        }
    };
    let mut peaks_v = Value::Array(peaks);
    res.insert("detector".into(), dg);
    if p.coordinate_mode == "standard_wrapper" {
        res.insert("peaks".into(), peaks_v.clone());
        res.insert("stage".into(), json!("coordinate_conversion"));
        match wrap_standard_peaks(&peaks_v, &json!(signal.len()), &json!(0), &p.offscale_intervals) {
            Ok(w) => {
                peaks_v = w["peaks"].clone();
                res.insert("coordinate_conversion".into(), w["diagnostics"].clone());
                not_applied.retain(|s| *s != "wrapper integer coordinate conversion");
            }
            Err(e) => {
                add_reason(&mut res, &e);
                return finish(res, &not_applied, None);
            }
        }
    }
    res.insert("peaks".into(), peaks_v.clone());
    res.insert("stage".into(), json!("prefilters"));
    let expected_count = expected_bp.as_array().map(|a| a.len() as i64);
    let remaining_in = as_objs(&peaks_v);
    let filtered = filter_standard_peaks(&remaining_in, expected_count, &signal, &p.group_gap);
    res.insert("filtering".into(), filtered.clone());
    if filtered["status"] != json!("passed") {
        for r in filtered["reasons"].as_array().unwrap() {
            add_reason(&mut res, r.as_str().unwrap());
        }
        return finish(res, &not_applied, None);
    }
    res.insert("stage".into(), json!("matching"));
    let remaining = as_objs(&filtered["peaks"]);
    let m = match_ratio_standard(&remaining, expected_bp, num_extra_peaks, p.ratio_factor, p.min_accepted_score, p.ambiguity_margin,
                                 p.allow_prefix, &p.max_bp_per_scan);
    let mut matching = Obj::new();
    for k in ["status", "reasons", "matched", "selection", "attempts", "diagnostics"] {
        matching.insert(k.into(), m.result[k].clone());
    }
    res.insert("matching".into(), Value::Object(matching));
    for r in m.result["reasons"].as_array().unwrap() {
        add_reason(&mut res, r.as_str().unwrap());
    }
    if m.result["selection"].as_object().map(|o| o.is_empty()).unwrap_or(true) {
        return finish(res, &not_applied, None);
    }
    let mut assign: std::collections::HashMap<i64, Value> = std::collections::HashMap::new();
    for mp in m.result["matched"].as_array().unwrap() {
        assign.insert(mp["candidate_index"].as_i64().unwrap(), mp["expected_bp"].clone());
    }
    let mut selected: Vec<Obj> = Vec::new();
    for i in m.result["selection"]["candidate_indices"].as_array().unwrap() {
        let i = i.as_i64().unwrap();
        let mut d = remaining[i as usize].clone();
        d.insert("matched_bp".into(), assign.get(&i).cloned().unwrap_or(Value::Null));
        selected.push(d);
    }
    res.insert("selected_peaks".into(), Value::Array(selected.iter().cloned().map(Value::Object).collect()));
    let quality = evaluate_sizing_quality(&selected, expected_bp);
    res.insert("base_quality".into(), quality.clone());
    if quality["status"] != json!("evaluated") {
        res.insert("stage".into(), json!("base_quality"));
        for r in quality["reasons"].as_array().unwrap() {
            add_reason(&mut res, r.as_str().unwrap());
        }
        return finish(res, &not_applied, None);
    }
    if quality["score"].as_f64().unwrap() < quality_threshold {
        res.insert("stage".into(), json!("base_quality"));
        add_reason(&mut res, "Base sizing quality below explicit reporting threshold");
        return finish(res, &not_applied, None);
    }
    if m.result["status"] != json!("accepted") {
        return finish(res, &not_applied, None);
    }
    res.insert("status".into(), json!("accepted"));
    res.insert("stage".into(), json!("complete"));
    res.insert("anchors".into(), m.result["anchors"].clone());
    finish(res, &not_applied, m.calibration)
}
