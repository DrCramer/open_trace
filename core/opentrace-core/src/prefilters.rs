//! Port of `genemapper_open/standard_prefilters.py`: early-signal and adjacent-peak filters for the size standard.
//!
//! Input peaks must be in strictly increasing scan order. Returned indices refer to the input slice.
use std::fmt;
use crate::numeric::py_sum;

pub const MAX_PEAKS: usize = 4096;
pub const MAX_POINTS: usize = 1_000_000;

#[derive(Debug, Clone, PartialEq)]
pub struct PeakIn {
    pub scan: f64,
    pub height: f64,
    pub begin: f64,
    pub end: f64,
    pub begin_height: f64,
    pub end_height: f64,
    pub fwhm_scan: f64,
    pub offscaled: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GroupEvent {
    pub dominant_index: usize,
    pub removed_index: usize,
    pub side: &'static str,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Diagnostics {
    pub input_count: usize,
    pub expected_count: usize,
    pub group_gap: f64,
    pub early_stage_applied: bool,
    pub early_stage_skip_reason: Option<&'static str>,
    pub early_shape_removed_indices: Vec<usize>,
    pub early_high_removed_indices: Vec<usize>,
    pub early_prefix_removed_indices: Vec<usize>,
    pub connected_tail_removed_indices: Vec<usize>,
    pub group_removed_indices: Vec<usize>,
    pub group_events: Vec<GroupEvent>,
    pub early_cutoff_scan: Option<f64>,
    pub late_reference_count: usize,
    pub reference_means: Option<(f64, f64, f64)>,
    pub last_large_early_scan: Option<f64>,
    pub after_early_count: usize,
    pub output_count: usize,
    pub below_expected_after_filtering: bool,
    pub group_examined_count: usize,
    pub connectivity_error: Option<(i64, i64, usize, usize)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FilterResult {
    pub passed: bool,
    /// Indices into the input slice of surviving peaks, in input order.
    pub kept: Vec<usize>,
    pub reasons: Vec<String>,
    pub diagnostics: Diagnostics,
}

impl fmt::Display for FilterResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", if self.passed { "passed" } else { "rejected" })
    }
}

fn finite(v: f64) -> bool {
    v.is_finite()
}

/// Python `_ratio`: IEEE comparison semantics for possibly zero morphology ratios.
fn ratio(numerator: f64, denominator: f64) -> f64 {
    if denominator != 0.0 {
        return numerator / denominator;
    }
    if numerator == 0.0 {
        return f64::NAN;
    }
    f64::INFINITY.copysign(numerator) * 1.0f64.copysign(denominator)
}

fn reject(mut diagnostics: Diagnostics, reason: impl Into<String>) -> FilterResult {
    let _ = &mut diagnostics;
    FilterResult { passed: false, kept: Vec::new(), reasons: vec![reason.into()], diagnostics }
}

/// Python `filter_standard_peaks(peaks, expected_count, signal, group_gap=...)`.
pub fn filter_standard_peaks(peaks: &[PeakIn], expected_count: usize, signal: &[i64], group_gap: f64) -> FilterResult {
    let mut diag = Diagnostics {
        input_count: peaks.len(),
        expected_count,
        group_gap,
        ..Default::default()
    };
    if !(3..=32).contains(&expected_count) {
        return reject(diag, "expected_count must be an integer in [3, 32]");
    }
    if !finite(group_gap) || !(0.0..=100_000_000.0).contains(&group_gap) {
        return reject(diag, "group_gap must be finite and in [0, 100000000]");
    }
    if signal.is_empty() || signal.len() > MAX_POINTS {
        return reject(diag, "signal must be a nonempty bounded list or tuple");
    }
    if signal.iter().any(|&v| !(-(1i64 << 31)..(1i64 << 31)).contains(&v)) {
        return reject(diag, "signal must contain signed 32-bit integers");
    }
    if peaks.len() > MAX_PEAKS {
        return reject(diag, "peaks must be an explicit list or tuple of at most 4096 peaks");
    }
    let n = signal.len() as f64;
    for (index, p) in peaks.iter().enumerate() {
        let vals = [p.scan, p.height, p.begin, p.end, p.begin_height, p.end_height, p.fwhm_scan];
        if vals.iter().any(|v| !finite(*v)) {
            return reject(diag, format!("Peak {}: finite numeric morphology fields required", index));
        }
        if !(0.0 <= p.begin && p.begin <= p.scan && p.scan <= p.end && p.end < n)
            || !(p.height > 0.0 && p.height <= 1e12)
            || !(p.fwhm_scan >= 0.0 && p.fwhm_scan <= MAX_POINTS as f64)
            || p.begin_height.abs() > 1e12
            || p.end_height.abs() > 1e12
        {
            return reject(diag, format!("Peak {}: invalid bounded morphology or offscaled flag", index));
        }
        if index > 0 && p.scan <= peaks[index - 1].scan {
            return reject(diag, "Peak scans must increase strictly; input is not sorted implicitly");
        }
    }
    if peaks.len() < expected_count {
        return reject(diag, "Fewer candidates than the full expected standard before prefilters");
    }

    let first = &peaks[0];
    let last = &peaks[peaks.len() - 1];
    let cutoff = (last.scan + 3.0 * first.scan) / 4.0;
    diag.early_cutoff_scan = Some(cutoff);
    let late: Vec<usize> = (0..peaks.len()).filter(|&i| peaks[i].scan >= cutoff).collect();
    diag.late_reference_count = late.len();
    // `survivors` holds original indices in input order.
    let mut survivors: Vec<usize> = (0..peaks.len()).collect();
    if late.len() as f64 >= 0.75 * expected_count as f64 {
        diag.early_stage_applied = true;
        // Stable sort by descending height, first `expected_count` (Python `sorted(..., key=-H)`).
        let mut reference = late.clone();
        reference.sort_by(|&a, &b| peaks[b].height.partial_cmp(&peaks[a].height).unwrap());
        reference.truncate(expected_count);
        let k = reference.len() as f64;
        let mean_height = py_sum(reference.iter().map(|&i| peaks[i].height)) / k;
        let mean_begin = py_sum(reference.iter().map(|&i| peaks[i].begin_height)) / k;
        let mean_width = py_sum(reference.iter().map(|&i| peaks[i].fwhm_scan)) / k;
        diag.reference_means = Some((mean_height, mean_begin, mean_width));

        let mut retained: Vec<usize> = Vec::new();
        let mut last_large_scan = 0.0f64;
        let mut last_large: Option<usize> = None;
        for i in 0..peaks.len() {
            let p = &peaks[i];
            if p.scan > cutoff {
                retained.push(i);
                continue;
            }
            let (height, begin, end) = (p.height, p.begin_height, p.end_height);
            let shape = begin / height > 0.7 || end / height > 0.7 || (begin - end).abs() / height > 0.5;
            let update_large;
            if shape {
                diag.early_shape_removed_indices.push(i);
                update_large = height > 2.0 * mean_height;
            } else if height > 10.0 * mean_height || (p.fwhm_scan > 2.0 * mean_width && height > 5.0 * mean_height) {
                diag.early_high_removed_indices.push(i);
                update_large = true;
            } else {
                retained.push(i);
                update_large = false;
            }
            if update_large && p.scan > last_large_scan {
                last_large_scan = p.scan;
                last_large = Some(i);
            }
        }
        survivors = Vec::new();
        for &i in &retained {
            if peaks[i].scan > last_large_scan {
                survivors.push(i);
            } else {
                diag.early_prefix_removed_indices.push(i);
            }
        }
        diag.last_large_early_scan = Some(last_large_scan);
        if let (Some(&first_surv), Some(large)) = (survivors.first(), last_large) {
            let first_p = &peaks[first_surv];
            if ratio(first_p.height - first_p.begin_height, first_p.height - first_p.end_height) > 0.5
                && ratio(first_p.begin_height, mean_begin) > 5.0
            {
                let previous_end = peaks[large].end;
                let current_begin = first_p.begin;
                let connected = if previous_end == current_begin {
                    true
                } else {
                    // Explicit observed int-copy interval (Python int() truncates toward zero).
                    let start = previous_end.trunc() as i64;
                    let count = (current_begin - previous_end + 1.0).trunc() as i64;
                    if count < 0 || start < 0 || start + count > signal.len() as i64 {
                        diag.connectivity_error = Some((start, count, large, first_surv));
                        return reject(diag, "Malformed connected-tail signal interval; filtering withheld");
                    }
                    let (s, e) = (start as usize, (start + count) as usize);
                    signal[s..e].iter().all(|&v| v as f64 >= first_p.begin_height)
                };
                if connected {
                    diag.connected_tail_removed_indices.push(first_surv);
                    survivors.remove(0);
                }
            }
        }
    } else {
        diag.early_stage_skip_reason = Some("Fewer than 0.75*expected_count late reference peaks");
    }
    diag.after_early_count = survivors.len();

    // Neighbor flags use the fixed list after the early stage.
    let right_shape: Vec<bool> = survivors
        .iter()
        .map(|&i| {
            let p = &peaks[i];
            p.end_height / p.height > 0.7 || (p.end_height - p.begin_height) / p.height > 0.5
        })
        .collect();
    let left_shape: Vec<bool> = survivors
        .iter()
        .map(|&i| {
            let p = &peaks[i];
            p.begin_height / p.height > 0.7 || (p.begin_height - p.end_height) / p.height > 0.5
        })
        .collect();
    let m = survivors.len();
    let mut examined = vec![false; m];
    let mut examined_count = 0usize;
    let mut removed = vec![false; m];
    let mut remaining = m;
    while remaining > expected_count && examined_count < m {
        // Python `max` over unexamined indices: first maximal height wins.
        let mut dominant: Option<usize> = None;
        for i in 0..m {
            if examined[i] {
                continue;
            }
            match dominant {
                None => dominant = Some(i),
                Some(d) => {
                    if peaks[survivors[i]].height > peaks[survivors[d]].height {
                        dominant = Some(i);
                    }
                }
            }
        }
        let dominant = dominant.unwrap();
        let peak = &peaks[survivors[dominant]];
        let mut chosen: Option<(usize, &'static str)> = None;
        let neighbors: [(isize, &'static str, &Vec<bool>); 2] = [
            (dominant as isize - 1, "left", &right_shape),
            (dominant as isize + 1, "right", &left_shape),
        ];
        for (neighbor, direction, flags) in neighbors {
            if neighbor < 0 || neighbor >= m as isize || examined[neighbor as usize] {
                continue;
            }
            let nb = neighbor as usize;
            let other = &peaks[survivors[nb]];
            let shape_smaller = flags[nb] && other.height < peak.height;
            let lower_ratio = other.height / peak.height < 0.6;
            if (other.scan - peak.scan).abs() < group_gap && (shape_smaller || lower_ratio) && !peak.offscaled {
                chosen = Some((nb, direction));
                break;
            }
        }
        if let Some((nb, side)) = chosen {
            removed[nb] = true;
            examined[nb] = true;
            examined_count += 1;
            remaining -= 1;
            diag.group_removed_indices.push(survivors[nb]);
            diag.group_events.push(GroupEvent {
                dominant_index: survivors[dominant],
                removed_index: survivors[nb],
                side,
            });
        }
        if !examined[dominant] {
            examined[dominant] = true;
            examined_count += 1;
        }
    }
    diag.group_examined_count = examined_count;
    let kept: Vec<usize> = (0..m).filter(|&i| !removed[i]).map(|i| survivors[i]).collect();
    diag.output_count = kept.len();
    diag.below_expected_after_filtering = kept.len() < expected_count;
    FilterResult { passed: true, kept, reasons: Vec::new(), diagnostics: diag }
}
