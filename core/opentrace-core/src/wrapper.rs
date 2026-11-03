//! Port of `genemapper_open/standard_wrapper.py`: integer projection of standard peak coordinates.
//!
//! Begin/scan/end are truncated to integers and shifted by `offset`; interval offscale flags are OR-ed in.
//! Returned records keep the source index so callers retain their full peak records.
use crate::prefilters::PeakIn;

pub const MAX_PEAKS: usize = 4096;
pub const MAX_OFFSCALE: usize = 10_000;

#[derive(Debug, Clone, PartialEq)]
pub struct Projected {
    /// Index into the input slice.
    pub source: usize,
    pub begin: f64,
    pub scan: f64,
    pub end: f64,
    pub offscaled: bool,
    /// Original local (begin, scan, end) before projection.
    pub source_local: (f64, f64, f64),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Diagnostics {
    pub input_count: usize,
    pub output_count: usize,
    pub signal_length: usize,
    pub offset: i64,
    pub fractional_coordinates_truncated: usize,
    pub new_offscaled_peaks: usize,
    pub duplicate_projected_scans: usize,
    pub offscale_interval_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WrapError {
    Invalid(String),
}

fn fin(v: f64) -> bool {
    v.is_finite()
}

pub fn wrap_standard_peaks(
    peaks: &[PeakIn],
    signal_length: usize,
    offset: i64,
    offscale_intervals: &[(f64, f64)],
) -> Result<(Vec<Projected>, Diagnostics), WrapError> {
    let bad = |m: &str| Err(WrapError::Invalid(m.to_string()));
    if !(1..=1_000_000).contains(&signal_length) {
        return bad("signal_length must be an integer in [1, 1000000]");
    }
    if offset < 0 || offset > (1i64 << 31) - signal_length as i64 {
        return bad("offset must keep projected coordinates within nonnegative signed32 range");
    }
    if peaks.len() > MAX_PEAKS {
        return bad("peaks must be a list or tuple of at most4096 mappings");
    }
    if offscale_intervals.len() > MAX_OFFSCALE {
        return bad("offscale_intervals must be a bounded list or tuple");
    }
    for &(a, b) in offscale_intervals {
        if !fin(a) || !fin(b) || !(0.0 <= a && a <= b && b <= ((1i64 << 31) - 1) as f64) {
            return bad("offscale intervals require finite ordered nonnegative signed32 coordinates");
        }
    }
    let n = signal_length as f64;
    let mut out: Vec<Projected> = Vec::with_capacity(peaks.len());
    let mut changed = 0usize;
    let mut new_offscale = 0usize;
    for (i, p) in peaks.iter().enumerate() {
        if !fin(p.begin) || !fin(p.scan) || !fin(p.end) {
            return bad("Each peak requires finite begin, scan and end");
        }
        if !(0.0 <= p.begin && p.begin <= p.scan && p.scan <= p.end && p.end < n) {
            return bad("Peak coordinates must lie within the local signal");
        }
        let _ = i;
        let mut project = |v: f64| -> f64 {
            if v != v.trunc() {
                changed += 1;
            }
            v.trunc() + offset as f64
        };
        let begin = project(p.begin);
        let scan = project(p.scan);
        let end = project(p.end);
        let previous = p.offscaled;
        let hit = offscale_intervals.iter().any(|&(a, b)| a <= end && b >= begin);
        let offscaled = previous || hit;
        if offscaled && !previous {
            new_offscale += 1;
        }
        out.push(Projected {
            source: i,
            begin,
            scan,
            end,
            offscaled,
            source_local: (p.begin, p.scan, p.end),
        });
    }
    // Python sorts by projected scan; the sort is stable.
    out.sort_by(|a, b| a.scan.partial_cmp(&b.scan).unwrap());
    let mut distinct: Vec<u64> = out.iter().map(|p| p.scan.to_bits()).collect();
    distinct.sort_unstable();
    distinct.dedup();
    let diag = Diagnostics {
        input_count: peaks.len(),
        output_count: out.len(),
        signal_length,
        offset,
        fractional_coordinates_truncated: changed,
        new_offscaled_peaks: new_offscale,
        duplicate_projected_scans: out.len() - distinct.len(),
        offscale_interval_count: offscale_intervals.len(),
    };
    Ok((out, diag))
}
