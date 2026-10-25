//! Port of `genemapper_open/sg_peaks.py`: cubic Savitzky-Golay derivative and peak measurements.
use crate::fsum::fsum;
use crate::numeric::int_true_div;

/// Cubic (degree 3) SG first-derivative weights w_k, k = 1..radius, from the closed form of the reference.
pub fn cubic_weights(radius: usize) -> Vec<f64> {
    let r = radius as i128;
    let sum = |p: u32| -> i128 { 2 * (1..=r).map(|k| k.pow(p)).sum::<i128>() };
    let (s2, s4, s6) = (sum(2), sum(4), sum(6));
    let det = s2 * s6 - s4 * s4;
    (1..=r).map(|k| int_true_div(s6 * k - s4 * k.pow(3), det)).collect()
}

/// Derivative at unit spacing; the first/last `radius` outputs are zero.
pub fn derivative(values: &[i64], weights: &[f64]) -> Vec<f64> {
    let radius = weights.len();
    let n = values.len();
    let mut out = vec![0.0; n];
    if n < 2 * radius + 1 {
        return out;
    }
    for c in radius..n - radius {
        out[c] = fsum(weights.iter().enumerate().map(|(i, &w)| {
            let k = i + 1;
            w * (values[c + k] - values[c - k]) as f64
        }));
    }
    out
}

fn zero(x0: f64, y0: f64, x1: f64, y1: f64) -> Option<f64> {
    if y0 == y1 {
        return if y0 == 0.0 { Some((x0 + x1) / 2.0) } else { None };
    }
    Some(x0 - y0 * (x1 - x0) / (y1 - y0))
}

pub fn candidates(d: &[f64]) -> Vec<Option<f64>> {
    let mut positive = false;
    let mut zero_start: Option<usize> = None;
    let mut peaks = Vec::new();
    for (i, &v) in d.iter().enumerate() {
        if v > 0.0 {
            positive = true;
            zero_start = None;
        } else if v == 0.0 {
            if positive && zero_start.is_none() {
                zero_start = Some(i);
            }
        } else if positive {
            let crossing = match zero_start {
                Some(z) => Some((z as f64 + i as f64 - 1.0) / 2.0),
                None => zero((i - 1) as f64, d[i - 1], i as f64, v),
            };
            peaks.push(crossing);
            positive = false;
            zero_start = None;
        }
    }
    peaks
}

fn bounds(d: &[f64], center: f64, tb: f64, te: f64) -> (Option<f64>, Option<f64>) {
    let n = d.len() as i64;
    let at = |i: i64| d[i as usize];
    let mut index = center.ceil() as i64 - 1;
    while index >= 0 && 0.0 <= at(index) && at(index) < tb {
        index -= 1;
    }
    if index > 0 && at(index) >= tb {
        index -= 1;
        while index >= 0 && at(index) > tb {
            index -= 1;
        }
    }
    let mut begin = Some(0.0);
    if index >= 0 && at(index) <= 0.0 {
        begin = zero(index as f64, at(index), (index + 1) as f64, at(index + 1));
    } else if index >= 0 && at(index) <= tb {
        begin = zero(index as f64, at(index) - tb, (index + 1) as f64, at(index + 1) - tb);
    }
    let mut index = center.floor() as i64 + 1;
    while index < n && te < at(index) && at(index) <= 0.0 {
        index += 1;
    }
    if index < n && at(index) <= te {
        index += 1;
        while index < n && at(index) < te {
            index += 1;
        }
    }
    let mut end = Some((n - 1) as f64);
    if index < n && at(index) >= 0.0 {
        end = zero((index - 1) as f64, at(index - 1), index as f64, at(index));
    } else if index < n && at(index) >= te {
        end = zero((index - 1) as f64, at(index - 1) - te, index as f64, at(index) - te);
    }
    (begin, end)
}

fn interpolate(v: &[i64], scan: f64) -> f64 {
    if v.len() == 1 {
        return v[0] as f64;
    }
    let left = (scan.floor() as i64).min(v.len() as i64 - 2) as usize;
    v[left] as f64 + (scan - left as f64) * (v[left + 1] - v[left]) as f64
}

fn half_width(v: &[i64], lo: usize, hi: usize, center: usize) -> f64 {
    if v[center] <= 0 {
        return 0.0;
    }
    let target = v[center] as f64 / 2.0;
    let fv = |i: usize| v[i] as f64;
    let mut li = center as i64 - 1;
    let mut ri = center + 1;
    while li >= lo as i64 && fv(li as usize) > target {
        li -= 1;
    }
    while ri <= hi && fv(ri) > target {
        ri += 1;
    }
    let (mut left, left_level);
    if li < lo as i64 {
        left = lo as f64;
        left_level = fv(lo);
    } else {
        let l = li as usize;
        let gap = v[l + 1] - v[l];
        if gap > 0 {
            left = l as f64 + (target - fv(l)) / gap as f64;
            left_level = target;
        } else {
            left = l as f64;
            left_level = fv(l);
        }
    }
    let (mut right, right_level);
    if ri > hi {
        right = hi as f64;
        right_level = fv(hi);
    } else {
        let gap = v[ri - 1] - v[ri];
        if gap > 0 {
            right = ri as f64 - (target - fv(ri)) / gap as f64;
            right_level = target;
        } else {
            right = ri as f64;
            right_level = fv(ri);
        }
    }
    if left_level > right_level {
        let above: Vec<usize> = (center..=hi).filter(|&i| fv(i) > left_level).collect();
        let below: Vec<usize> = (center..=hi).filter(|&i| fv(i) < left_level).collect();
        let a = *above.last().unwrap_or(&center);
        let b = *below.first().unwrap_or(&hi);
        right = a as f64;
        if v[a] > v[b] {
            right += (fv(a) - left_level) * (b as f64 - a as f64) / (v[a] - v[b]) as f64;
        }
    } else if left_level < right_level {
        let below: Vec<usize> = (lo..=center).filter(|&i| fv(i) < right_level).collect();
        let above: Vec<usize> = (lo..=center).filter(|&i| fv(i) > right_level).collect();
        let a = *below.last().unwrap_or(&lo);
        let b = *above.first().unwrap_or(&center);
        left = a as f64;
        if v[b] > v[a] {
            left += (right_level - fv(a)) * (b as f64 - a as f64) / (v[b] - v[a]) as f64;
        }
    }
    right - left
}

#[derive(Debug, Clone, PartialEq)]
pub struct Peak {
    pub scan: usize,
    pub sg_scan: f64,
    pub begin: f64,
    pub end: f64,
    pub height: f64,
    pub begin_height: f64,
    pub end_height: f64,
    pub fwhm_scan: f64,
    pub area: i64,
    pub offscaled: bool,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Diagnostics {
    pub sg_candidates: usize,
    pub invalid_bounds: usize,
    pub height_rejected: usize,
    pub width_rejected: usize,
    pub edge_rejected: usize,
    pub duplicate_snapped_scans: usize,
    pub accepted_peaks: usize,
}

/// Peak detection on a precomputed derivative (validation of the arguments stays in the Python wrapper).
pub fn detect(
    values: &[i64],
    d: &[f64],
    threshold: f64,
    min_width: f64,
    tau_begin: f64,
    tau_end: f64,
    offscale: &[(f64, f64)],
) -> (Vec<Peak>, Diagnostics) {
    let n = values.len();
    let cands = candidates(d);
    let mut diag = Diagnostics { sg_candidates: cands.len(), ..Default::default() };
    let mut peaks = Vec::new();
    for crossing in cands {
        // a crossing `None` (flat nonzero pair) behaves like the reference: math.ceil(None) raises there,
        // which cannot happen for a sign change; treat it as invalid bounds.
        let Some(crossing) = crossing else {
            diag.invalid_bounds += 1;
            continue;
        };
        let (b, e) = bounds(d, crossing, tau_begin, tau_end);
        let (begin, end) = match (b, e) {
            (Some(b), Some(e)) if 0.0 <= b && b <= crossing && crossing <= e && e < n as f64 => (b, e),
            _ => {
                diag.invalid_bounds += 1;
                continue;
            }
        };
        let (lo, hi, center) = (begin.ceil() as i64, end.floor() as i64, (crossing + 0.5).floor() as i64);
        if !(lo <= center && center <= hi) {
            diag.invalid_bounds += 1;
            continue;
        }
        let (lo, hi, center) = (lo as usize, hi as usize, center as usize);
        let height = *values[lo..=hi].iter().max().unwrap();
        let begin_height = interpolate(values, begin);
        let end_height = interpolate(values, end);
        if (height as f64) < threshold {
            diag.height_rejected += 1;
            continue;
        }
        let width = half_width(values, lo, hi, center);
        if !(width >= min_width || begin_height >= threshold || end_height >= threshold) {
            diag.width_rejected += 1;
            continue;
        }
        let snap = (lo..=hi).find(|&i| values[i] == height).unwrap();
        if !((snap as i64 - begin.trunc() as i64) > 1 && (end.trunc() as i64 - snap as i64) > 1) {
            diag.edge_rejected += 1;
            continue;
        }
        peaks.push(Peak {
            scan: snap,
            sg_scan: crossing,
            begin,
            end,
            height: height as f64,
            begin_height,
            end_height,
            fwhm_scan: width,
            area: values[lo..=hi].iter().sum(),
            offscaled: offscale.iter().any(|&(a, b)| a <= end && b >= begin),
        });
    }
    peaks.sort_by_key(|p| p.scan);
    let mut scans: Vec<usize> = peaks.iter().map(|p| p.scan).collect();
    scans.dedup();
    diag.duplicate_snapped_scans = peaks.len() - scans.len();
    diag.accepted_peaks = peaks.len();
    (peaks, diag)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn weights_r7() {
        let w = cubic_weights(7);
        assert_eq!(w.len(), 7);
        assert!(w[0] > 0.0);
    }
}
