//! Port of `genemapper_open/sizing_quality.py`: morphology penalties times a chi-square survival term.
//!
//! lgamma is taken from libm rather than CPython's table, so results agree to a few ulp (not bit-identical).
use crate::fsum::fsum;
use crate::numeric::{lgamma, py_median};

pub const MAX_PEAKS: usize = 4096;
pub const MAX_EXPECTED: usize = 32;
const EPSILON: f64 = 2e-15;
const TINY: f64 = 1e-300;

/// Upper regularized incomplete gamma Q(shape, value); series or modified Lentz fraction.
fn regularized_gamma_q(shape: f64, value: f64) -> Result<f64, String> {
    if value == 0.0 {
        return Ok(1.0);
    }
    if value.is_infinite() {
        return Ok(0.0);
    }
    let log_factor = shape * value.ln() - value - lgamma(shape);
    if value < shape + 1.0 {
        let mut term = 1.0 / shape;
        let mut total = term;
        for step in 1..=10000 {
            term *= value / (shape + step as f64);
            total += term;
            if term.abs() <= total.abs() * EPSILON {
                let log_lower = (log_factor + total.ln()).min(0.0);
                return Ok((-log_lower.exp_m1()).min(1.0).max(0.0));
            }
        }
        return Err("Incomplete-gamma series did not converge".into());
    }
    let mut denominator = value + 1.0 - shape;
    let inverse0 = 1.0 / denominator;
    let mut numerator = 1.0 / TINY;
    let mut fraction = inverse0;
    let mut inverse = inverse0;
    for step in 1..=10000 {
        let coefficient = -(step as f64) * (step as f64 - shape);
        denominator += 2.0;
        let mut divisor = denominator + coefficient * inverse;
        if divisor.abs() < TINY {
            divisor = TINY.copysign(divisor);
        }
        numerator = denominator + coefficient / numerator;
        if numerator.abs() < TINY {
            numerator = TINY.copysign(numerator);
        }
        inverse = 1.0 / divisor;
        let change = inverse * numerator;
        fraction *= change;
        if (change - 1.0).abs() <= EPSILON {
            if fraction <= 0.0 {
                return Err("Incomplete-gamma fraction became nonpositive".into());
            }
            return Ok((log_factor + fraction.ln()).exp().min(1.0).max(0.0));
        }
    }
    Err("Incomplete-gamma fraction did not converge".into())
}

/// P(ChiSquared(df) >= statistic) for integer df in 1..=1024 and statistic >= 0.
pub fn chi_square_survival(statistic: f64, degrees_of_freedom: usize) -> Result<f64, String> {
    if !(1..=1024).contains(&degrees_of_freedom) {
        return Err("Finite nonnegative statistic or infinity, and integer df1..1024 required".into());
    }
    if statistic.is_nan() || statistic < 0.0 {
        return Err("Statistic must be nonnegative".into());
    }
    regularized_gamma_q(degrees_of_freedom as f64 / 2.0, statistic / 2.0)
}

#[derive(Debug, Clone, PartialEq)]
pub struct SqPeak {
    pub scan: f64,
    pub height: f64,
    pub begin_height: f64,
    pub end_height: f64,
    pub matched_bp: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub kind: &'static str,
    pub peak_index: usize,
    pub factor: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SizingQuality {
    pub evaluated: bool,
    pub score: Option<f64>,
    pub raw_score: Option<f64>,
    pub reasons: Vec<String>,
    pub events: Vec<Event>,
    pub complete: bool,
    pub matched_count: usize,
    pub expected_count: usize,
    pub penalty_factor: f64,
    pub chi_square_statistic: Option<f64>,
    pub degrees_of_freedom: Option<usize>,
    pub chi_square_survival: Option<f64>,
    pub global_bp_per_scan: Option<f64>,
}

fn bounded(v: f64, min: f64, max: f64, positive: bool) -> bool {
    v.is_finite() && min <= v && v <= max && (!positive || v > 0.0)
}

/// Port of `evaluate_sizing_quality(peaks, expected_bp)`. `Err` carries a rejection reason (status 'rejected').
pub fn evaluate_sizing_quality(peaks: &[SqPeak], expected_bp: &[f64]) -> Result<SizingQuality, String> {
    let blank = SizingQuality {
        evaluated: false,
        score: None,
        raw_score: None,
        reasons: Vec::new(),
        events: Vec::new(),
        complete: false,
        matched_count: 0,
        expected_count: 0,
        penalty_factor: 1.0,
        chi_square_statistic: None,
        degrees_of_freedom: None,
        chi_square_survival: None,
        global_bp_per_scan: None,
    };
    if !(3..=MAX_EXPECTED).contains(&expected_bp.len()) {
        return Err("expected_bp must contain3..32 explicit increasing sizes".into());
    }
    if expected_bp.iter().any(|&v| !bounded(v, 0.0, 1_000_000.0, true))
        || expected_bp.windows(2).any(|w| w[1] - w[0] < 1e-6)
    {
        return Err("expected_bp requires finite positive values and gaps>=0.000001bp".into());
    }
    if peaks.len() > MAX_PEAKS {
        return Err("peaks must be a list or tuple of at most4096 mappings".into());
    }
    for (index, p) in peaks.iter().enumerate() {
        if !bounded(p.scan, 0.0, 100_000_000.0, false)
            || !bounded(p.height, 0.0, 1e12, true)
            || !bounded(p.begin_height, -1e12, 1e12, false)
            || !bounded(p.end_height, -1e12, 1e12, false)
            || matches!(p.matched_bp, Some(v) if !bounded(v, 0.0, 1_000_000.0, true))
        {
            return Err(format!("peak{} has invalid scan/height/boundary features", index));
        }
    }
    if peaks.windows(2).any(|w| w[1].scan - w[0].scan < 1e-6) {
        return Err("input peaks must already have strictly increasing scans with gaps>=0.000001".into());
    }
    let mut r = SizingQuality { expected_count: expected_bp.len(), ..blank };
    // (input index, peak) for matched peaks, in input order.
    let matched: Vec<(usize, &SqPeak)> =
        peaks.iter().enumerate().filter(|(_, p)| p.matched_bp.is_some()).collect();
    r.matched_count = matched.len();
    if matched.len() < expected_bp.len() {
        return Ok(zero(r, "incomplete_standard"));
    }
    let matched_bp: Vec<f64> = matched.iter().map(|(_, p)| p.matched_bp.unwrap()).collect();
    if matched_bp != expected_bp {
        return Ok(zero(r, "matched_sizes_do_not_equal_expected_standard"));
    }
    r.complete = true;
    let mut penalty = 1.0f64;
    let apply = |r: &mut SizingQuality, kind: &'static str, peak_index: usize, factor: f64, penalty: &mut f64| {
        *penalty *= factor;
        r.events.push(Event { kind, peak_index, factor });
        r.penalty_factor = *penalty;
    };
    for field in [0usize, 1] {
        let values: Vec<f64> = matched.iter().map(|(_, p)| if field == 0 { p.begin_height } else { p.end_height }).collect();
        let average = fsum(values.iter().cloned()) / matched.len() as f64;
        let kind = if field == 0 { "begin_height_outlier" } else { "end_height_outlier" };
        for (idx, p) in &matched {
            let v = if field == 0 { p.begin_height } else { p.end_height };
            if v > 10.0 * average && v / p.height > 0.25 {
                apply(&mut r, kind, *idx, 0.5, &mut penalty);
            }
        }
    }
    let center_height = py_median(&matched.iter().map(|(_, p)| p.height).collect::<Vec<f64>>());
    for (idx, p) in &matched {
        if p.height > 8.0 * center_height {
            apply(&mut r, "height_above_8_medians", *idx, 0.5, &mut penalty);
        }
    }
    for w in matched.windows(2) {
        if w[1].1.height / w[0].1.height > 10.0 {
            return Ok(zero(r, "directed_neighbor_height_ratio_above10"));
        }
    }
    for w in matched.windows(2) {
        let (li, lp) = w[0];
        let (ri, rp) = w[1];
        let average = (lp.height + rp.height) / 2.0;
        for idx in (li + 1)..ri {
            let p = &peaks[idx];
            if p.matched_bp.is_some() {
                continue;
            }
            if p.height > 5.0 * average {
                return Ok(zero(r, "unmatched_between_anchors_above5_mean"));
            }
            if p.height > 0.7 * average {
                apply(&mut r, "unmatched_between_anchors_above0_7_mean", idx, 0.9, &mut penalty);
            }
        }
    }
    let (first_i, first) = matched[0];
    let (last_i, last) = matched[matched.len() - 1];
    let _ = (first_i, last_i);
    let rate = (last.matched_bp.unwrap() - first.matched_bp.unwrap()) / (last.scan - first.scan);
    r.global_bp_per_scan = Some(rate);
    if rate > 0.25 {
        return Ok(zero(r, "global_migration_rate_above0_25"));
    }
    let terms: Vec<f64> = matched
        .windows(2)
        .map(|w| {
            let (l, rt) = (w[0].1, w[1].1);
            let d = rate * (rt.scan - l.scan) - (rt.matched_bp.unwrap() - l.matched_bp.unwrap());
            (d * d) / (rt.matched_bp.unwrap() - l.matched_bp.unwrap())
        })
        .collect();
    let statistic = fsum(terms.into_iter());
    let degrees = matched.len() - 2;
    let survival = match chi_square_survival(statistic, degrees) {
        Ok(v) => v,
        Err(_) => return Err("chi_square_numerical_evaluation_failed".into()),
    };
    let raw = (penalty * survival).min(1.0).max(0.0);
    r.evaluated = true;
    r.score = Some(((raw * 10000.0 + 0.5).floor()) / 10000.0);
    r.raw_score = Some(raw);
    r.chi_square_statistic = Some(statistic);
    r.degrees_of_freedom = Some(degrees);
    r.chi_square_survival = Some(survival);
    r.penalty_factor = penalty;
    Ok(r)
}

fn zero(mut r: SizingQuality, reason: &str) -> SizingQuality {
    r.evaluated = true;
    r.score = Some(0.0);
    r.raw_score = Some(0.0);
    r.reasons.push(reason.to_string());
    r
}
