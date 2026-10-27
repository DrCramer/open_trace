//! Port of the `detect_sg_peaks` wrapper in `genemapper_open/sg_peaks.py`: argument validation, the cubic
//! Savitzky-Golay derivative, then `sg::detect`.
//!
//! Only the closed-form cubic derivative (`degree == 3`) is ported. The general-degree weights in the Python
//! reference are exact rational solutions; until they are ported, other degrees are rejected explicitly
//! instead of silently falling back.
use crate::sg::{cubic_weights, derivative, detect, Diagnostics, Peak};

pub const MAX_POINTS: usize = 1_000_000;

#[allow(clippy::too_many_arguments)]
pub fn detect_sg_peaks(
    values: &[i64],
    threshold: f64,
    window: usize,
    degree: usize,
    min_width: f64,
    tau_begin: f64,
    tau_end: f64,
    offscale: &[(f64, f64)],
) -> Result<(Vec<Peak>, Diagnostics), String> {
    if !(threshold.is_finite() && threshold > 0.0 && threshold <= 1e12) {
        return Err("Threshold must be finite and positive, <= 1e12".into());
    }
    if !(min_width.is_finite() && (0.0..=1e6).contains(&min_width)) {
        return Err("Minimum width must be finite and in [0, 1000000]".into());
    }
    if !(tau_begin.is_finite() && (0.0..=1e12).contains(&tau_begin))
        || !(tau_end.is_finite() && (-1e12..=0.0).contains(&tau_end))
    {
        return Err("Slope thresholds require finite tau_begin>=0 and tau_end<=0".into());
    }
    if offscale.len() > 10000 {
        return Err("Offscale intervals must be a bounded list or tuple".into());
    }
    let n = values.len() as f64;
    for &(a, b) in offscale {
        if !(a.is_finite() && b.is_finite() && 0.0 <= a && a <= b && b < n) {
            return Err("Offscale intervals must lie within the selected signal".into());
        }
    }
    // cubic_sg_derivative validation (Python order).
    if values.is_empty() || values.len() > MAX_POINTS {
        return Err("Signal must be a nonempty bounded list or tuple".into());
    }
    if values.iter().any(|&v| !(-(1i64 << 31)..(1i64 << 31)).contains(&v)) {
        return Err("Signal values must be signed 32-bit integers".into());
    }
    if !(5..=101).contains(&window) || window % 2 != 1 || values.len() < window {
        return Err("Cubic SG window must be odd, 5..101, and <= signal length".into());
    }
    if degree != 3 {
        return Err(format!("SG polynomial degree {} is not ported yet (only degree 3)", degree));
    }
    let weights = cubic_weights(window / 2);
    let d = derivative(values, &weights);
    Ok(detect(values, &d, threshold, min_width, tau_begin, tau_end, offscale))
}
