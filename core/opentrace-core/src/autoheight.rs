//! Port of `genemapper_open/autoheight.py`: Basic size-standard threshold from a discrete background model.
//! All arithmetic is integer except the directional tracker level, as in the reference.
use crate::numeric::py_median;

pub const MAX_POINTS: usize = 1_000_000;
pub const MAX_VALUE: i64 = 100_000_000;

#[derive(Debug, Clone, PartialEq)]
pub struct Autoheight {
    pub threshold: f64,
    pub median_control: f64,
    pub control: Vec<i64>,
}

fn average_stage(signal: &[i64], width: usize, clip: bool) -> Vec<i64> {
    if signal.len() < width {
        return signal.to_vec();
    }
    let radius = width / 2;
    let mut output = signal.to_vec();
    let mut total: i64 = signal[..width].iter().sum();
    output[radius] = total / width as i64;
    for index in (radius + 1)..(signal.len() - radius) {
        total += signal[index + radius] - signal[index - radius - 1];
        output[index] = total / width as i64;
    }
    if clip {
        signal.iter().zip(output.iter()).map(|(&before, &after)| before.min(after).max(0)).collect()
    } else {
        output.iter().map(|&v| v.max(0)).collect()
    }
}

fn polynomial_smooth(signal: &[i64]) -> Vec<i64> {
    let mut output = signal.to_vec();
    let n = signal.len();
    for index in 10..(n - 10) {
        let mut total: i64 = 329 * signal[index];
        for k in 1..=10i64 {
            total += (329 - 5 * k * k) * (signal[index - k as usize] + signal[index + k as usize]);
        }
        output[index] = total / 3059;
    }
    output
}

fn directional_track(signal: &[i64], reverse: bool) -> Vec<i64> {
    let n = signal.len();
    let mut level: f64 = if reverse {
        signal[n - 10..].iter().sum::<i64>() as f64 / 10.0
    } else {
        signal[..10].iter().sum::<i64>() as f64 / 10.0
    };
    let mut output = vec![0i64; n];
    let order: Vec<usize> = if reverse { (0..n).rev().collect() } else { (0..n).collect() };
    for index in order {
        let value = signal[index];
        if value <= level.trunc() as i64 {
            level = value as f64;
            output[index] = value;
            continue;
        }
        let gap = value as f64 - level;
        let denominator = if gap > 20.0 {
            gap * (gap * gap)
        } else if gap > 10.0 {
            gap * gap
        } else {
            gap
        };
        let weight = if denominator >= 1.0 { 1.0 / denominator } else { 0.0 };
        level += weight * gap;
        output[index] = (level + 0.5).trunc() as i64;
    }
    output
}

/// Port of `estimate_standard_autoheight(analyzed)`. `Err` carries the reference's ValueError message.
pub fn estimate_standard_autoheight(analyzed: &[i64]) -> Result<Autoheight, String> {
    if !(250..=MAX_POINTS).contains(&analyzed.len()) {
        return Err("Autoheight requires 250..1000000 analyzed integer samples".into());
    }
    if analyzed.iter().any(|&v| !(0..=MAX_VALUE).contains(&v)) {
        return Err("Autoheight requires nonnegative integer samples <=100000000".into());
    }
    let mut background = average_stage(analyzed, 11, false);
    let quarter = analyzed.len() / 4;
    for width in [quarter, 21, 51, 101, 201] {
        background = average_stage(&background, width, true);
    }
    background = polynomial_smooth(&background);
    background = average_stage(&background, 21, true);
    let forward = directional_track(&background, false);
    let backward = directional_track(&background, true);
    let control: Vec<i64> = analyzed
        .iter()
        .zip(forward.iter().zip(backward.iter()))
        .map(|(&value, (&left, &right))| (value - (value - left.max(right)).max(0)).max(0))
        .collect();
    let as_f: Vec<f64> = control.iter().map(|&v| v as f64).collect();
    let center = py_median(&as_f);
    let threshold = 30.0f64.max(25.0 * center);
    if !threshold.is_finite() {
        return Err("Autoheight is not finite".into());
    }
    Ok(Autoheight { threshold, median_control: center, control })
}
