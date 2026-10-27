//! Port of `genemapper_open/sizing_pipeline.py::analyze_size_standard`: preparation, detection (explicit or
//! basic-standard autoheight), optional standard-wrapper coordinates, prefilters, ratio matching, base sizing
//! quality, and calibration anchors only when the ratio match is accepted.
use crate::analysis::Calibration;
use crate::autoheight::estimate_standard_autoheight;
use crate::prefilters::{filter_standard_peaks, PeakIn};
use crate::preprocessing::{correct, estimate_baseline, smooth_trace};
use crate::ratio::{match_ratio_standard, RatioCandidate};
use crate::sg_detect::detect_sg_peaks;
use crate::sizing_quality::{evaluate_sizing_quality, SqPeak};
use crate::wrapper::{wrap_standard_peaks, WrapError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeightMode {
    Explicit,
    BasicStandardAuto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinateMode {
    ApFractional,
    StandardWrapper,
}

#[derive(Debug, Clone)]
pub struct SizeParams {
    pub num_extra_peaks: usize,
    pub quality_threshold: f64,
    pub baseline_window: usize,
    pub spike_window: usize,
    pub baseline_smoothing_window: usize,
    pub smooth_passes: usize,
    pub height_mode: HeightMode,
    pub detection_threshold: f64,
    pub sg_window: usize,
    pub sg_degree: usize,
    pub min_width: f64,
    pub tau_begin: f64,
    pub tau_end: f64,
    pub group_gap: f64,
    pub ratio_factor: f64,
    pub min_accepted_score: f64,
    pub ambiguity_margin: f64,
    pub allow_prefix: bool,
    pub max_bp_per_scan: Option<f64>,
    pub offscale_intervals: Vec<(f64, f64)>,
    pub coordinate_mode: CoordinateMode,
}

impl SizeParams {
    /// Python defaults; `num_extra_peaks` and `quality_threshold` are required keywords there.
    pub fn new(num_extra_peaks: usize, quality_threshold: f64) -> Self {
        SizeParams {
            num_extra_peaks,
            quality_threshold,
            baseline_window: 51,
            spike_window: 5,
            baseline_smoothing_window: 1,
            smooth_passes: 2,
            height_mode: HeightMode::Explicit,
            detection_threshold: 100.0,
            sg_window: 15,
            sg_degree: 3,
            min_width: 2.0,
            tau_begin: 0.0,
            tau_end: 0.0,
            group_gap: 25.0,
            ratio_factor: 0.8,
            min_accepted_score: 0.75,
            ambiguity_margin: 0.01,
            allow_prefix: true,
            max_bp_per_scan: Some(0.25),
            offscale_intervals: Vec::new(),
            coordinate_mode: CoordinateMode::ApFractional,
        }
    }
}

#[derive(Debug)]
pub struct SizeResult {
    pub accepted: bool,
    pub stage: &'static str,
    pub reasons: Vec<String>,
    /// (scan, expected bp) in ratio-match order; filled only when accepted.
    pub anchors: Vec<(f64, f64)>,
    pub calibration: Option<Calibration>,
    pub selected_count: usize,
    pub base_score: Option<f64>,
    pub matched_count: usize,
}

fn window_ok(w: usize) -> bool {
    w % 2 == 1 && (1..=100_001).contains(&w)
}

pub fn analyze_size_standard(values: &[i64], expected_bp: &[f64], p: &SizeParams) -> SizeResult {
    let mut r = SizeResult {
        accepted: false,
        stage: "validation",
        reasons: Vec::new(),
        anchors: Vec::new(),
        calibration: None,
        selected_count: 0,
        base_score: None,
        matched_count: 0,
    };
    if !(p.quality_threshold.is_finite() && p.quality_threshold > 0.0 && p.quality_threshold <= 1.0) {
        r.reasons.push("An explicit finite quality_threshold in (0, 1] is required".into());
        return r;
    }

    // Preparation (prepare_trace): trace validation, window validation, baseline, correction, smoothing.
    r.stage = "preparation";
    if values.is_empty() || values.len() > 1_000_000 {
        r.reasons.push("Trace must be a nonempty bounded list or tuple".into());
        return r;
    }
    if values.iter().any(|&v| !(-(1i64 << 31)..(1i64 << 31)).contains(&v)) {
        r.reasons.push("Trace values must be signed 32-bit integers".into());
        return r;
    }
    if !(window_ok(p.baseline_window) && window_ok(p.spike_window) && window_ok(p.baseline_smoothing_window)) {
        r.reasons.push("Window must be an odd integer in [1, 100001]".into());
        return r;
    }
    let baseline = estimate_baseline(
        values,
        p.baseline_window / 2,
        p.spike_window / 2,
        p.baseline_smoothing_window / 2,
    );
    let corrected = correct(values, &baseline);
    if corrected.iter().any(|&x| x >= 1i64 << 31) {
        r.reasons.push("Baseline-corrected trace exceeds signed 32-bit range".into());
        return r;
    }
    let signal = smooth_trace(&corrected, p.smooth_passes);

    // Detection threshold: basic-standard autoheight replaces the explicit one.
    r.stage = "detection";
    let mut threshold = p.detection_threshold;
    if p.height_mode == HeightMode::BasicStandardAuto {
        r.stage = "autoheight";
        match estimate_standard_autoheight(&signal) {
            Ok(a) => threshold = a.threshold,
            Err(e) => {
                r.reasons.push(e);
                return r;
            }
        }
        r.stage = "detection";
    }

    let detect_offscale: &[(f64, f64)] = if p.coordinate_mode == CoordinateMode::ApFractional {
        &p.offscale_intervals
    } else {
        &[]
    };
    let (peaks, _diag) = match detect_sg_peaks(
        &signal,
        threshold,
        p.sg_window,
        p.sg_degree,
        p.min_width,
        p.tau_begin,
        p.tau_end,
        detect_offscale,
    ) {
        Ok(v) => v,
        Err(e) => {
            r.reasons.push(e);
            return r;
        }
    };
    let mut peak_in: Vec<PeakIn> = peaks
        .iter()
        .map(|q| PeakIn {
            scan: q.scan as f64,
            height: q.height,
            begin: q.begin,
            end: q.end,
            begin_height: q.begin_height,
            end_height: q.end_height,
            fwhm_scan: q.fwhm_scan,
            offscaled: q.offscaled,
        })
        .collect();

    if p.coordinate_mode == CoordinateMode::StandardWrapper {
        r.stage = "coordinate_conversion";
        match wrap_standard_peaks(&peak_in, signal.len(), 0, &p.offscale_intervals) {
            Ok((projected, _)) => {
                let src = &peak_in;
                peak_in = projected
                    .iter()
                    .map(|pr| PeakIn {
                        scan: pr.scan,
                        begin: pr.begin,
                        end: pr.end,
                        offscaled: pr.offscaled,
                        ..src[pr.source].clone()
                    })
                    .collect();
            }
            Err(WrapError::Invalid(m)) => {
                r.reasons.push(m);
                return r;
            }
        }
    }

    // Prefilters.
    r.stage = "prefilters";
    let filtered = filter_standard_peaks(&peak_in, expected_bp.len(), &signal, p.group_gap);
    if !filtered.passed {
        r.reasons.extend(filtered.reasons);
        return r;
    }

    // Ratio matching over the surviving peaks.
    r.stage = "matching";
    let remaining: Vec<PeakIn> = filtered.kept.iter().map(|&i| peak_in[i].clone()).collect();
    let cands: Vec<RatioCandidate> = remaining
        .iter()
        .map(|q| RatioCandidate { scan: q.scan, height: q.height })
        .collect();
    let m = match_ratio_standard(
        &cands,
        expected_bp,
        p.num_extra_peaks,
        p.ratio_factor,
        p.min_accepted_score,
        p.ambiguity_margin,
        p.allow_prefix,
        p.max_bp_per_scan,
    );
    r.matched_count = m.matched.len();
    r.reasons.extend(m.reasons.iter().cloned());
    let Some(selection) = m.selection.as_ref() else {
        return r;
    };

    let mut assign: Vec<Option<f64>> = vec![None; remaining.len()];
    for mm in &m.matched {
        assign[mm.candidate_index] = Some(mm.expected_bp);
    }
    let selected: Vec<SqPeak> = selection
        .candidate_indices
        .iter()
        .map(|&i| SqPeak {
            scan: remaining[i].scan,
            height: remaining[i].height,
            begin_height: remaining[i].begin_height,
            end_height: remaining[i].end_height,
            matched_bp: assign[i],
        })
        .collect();
    r.selected_count = selected.len();

    // Base sizing quality, then the explicit reporting threshold.
    match evaluate_sizing_quality(&selected, expected_bp) {
        Err(e) => {
            r.stage = "base_quality";
            r.reasons.push(e);
            return r;
        }
        Ok(q) => {
            r.base_score = q.score;
            if !q.evaluated {
                r.stage = "base_quality";
                r.reasons.extend(q.reasons);
                return r;
            }
            let score = q.score.unwrap();
            if score < p.quality_threshold {
                r.stage = "base_quality";
                r.reasons.push("Base sizing quality below explicit reporting threshold".into());
                return r;
            }
        }
    }

    if !m.accepted {
        return r;
    }
    r.anchors = m.matched.iter().map(|mm| (mm.scan, mm.expected_bp)).collect();
    r.calibration = m.calibration.clone();
    r.accepted = true;
    r.stage = "complete";
    r
}
