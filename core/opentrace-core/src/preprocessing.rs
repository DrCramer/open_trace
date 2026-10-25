//! Port of `genemapper_open/preprocessing.py` (flat centered morphology with clipped support).
use std::collections::VecDeque;

/// Running maximum (`maximum = true`) or minimum over the centered window `[c - radius, c + radius]`.
pub fn extrema(values: &[i64], radius: usize, maximum: bool) -> Vec<i64> {
    let n = values.len();
    let mut queue: VecDeque<usize> = VecDeque::new();
    let mut out = Vec::with_capacity(n);
    let mut inserted = 0usize;
    for center in 0..n {
        let stop = n.min(center + radius + 1);
        while inserted < stop {
            let value = values[inserted];
            while let Some(&back) = queue.back() {
                let drop = if maximum { values[back] <= value } else { values[back] >= value };
                if drop {
                    queue.pop_back();
                } else {
                    break;
                }
            }
            queue.push_back(inserted);
            inserted += 1;
        }
        let start = center.saturating_sub(radius);
        while *queue.front().unwrap() < start {
            queue.pop_front();
        }
        out.push(values[*queue.front().unwrap()]);
    }
    out
}

/// Clipped centered moving average (sequential double accumulation, as the reference).
pub fn average(values: &[f64], radius: usize) -> Vec<f64> {
    let n = values.len();
    let (mut left, mut right, mut total) = (0usize, 0usize, 0.0f64);
    let mut out = Vec::with_capacity(n);
    for center in 0..n {
        let stop = n.min(center + radius + 1);
        while right < stop {
            total += values[right];
            right += 1;
        }
        let start = center.saturating_sub(radius);
        while left < start {
            total -= values[left];
            left += 1;
        }
        out.push(total / (right - left) as f64);
    }
    out
}

fn opening(t: &[i64], r: usize) -> Vec<i64> {
    extrema(&extrema(t, r, false), r, true)
}
fn closing(t: &[i64], r: usize) -> Vec<i64> {
    extrema(&extrema(t, r, true), r, false)
}

/// `estimate_baseline(values, window=2*broad+1, spike_window=2*narrow+1, smoothing_window=2*smooth+1)`.
pub fn estimate_baseline(signal: &[i64], broad: usize, narrow: usize, smooth: usize) -> Vec<f64> {
    let lower = closing(&opening(signal, broad), narrow);
    let upper = opening(&closing(signal, narrow), broad);
    let result: Vec<f64> = lower.iter().zip(&upper).map(|(a, b)| (a + b) as f64 / 2.0).collect();
    if smooth > 0 {
        average(&result, smooth)
    } else {
        result
    }
}

/// Baseline subtraction clipped at zero and rounded half up: `floor(max(0, x - b) + 0.5)`.
pub fn correct(signal: &[i64], baseline: &[f64]) -> Vec<i64> {
    signal
        .iter()
        .zip(baseline)
        .map(|(&x, &b)| ((x as f64 - b).max(0.0) + 0.5).floor() as i64)
        .collect()
}

/// Two adjacent floor averages per pass, endpoints unchanged.
pub fn smooth_trace(values: &[i64], passes: usize) -> Vec<i64> {
    let mut s = values.to_vec();
    for _ in 0..passes {
        if s.len() < 3 {
            break;
        }
        let adj: Vec<i64> = s.windows(2).map(|w| (w[0] + w[1]).div_euclid(2)).collect();
        let mut next = Vec::with_capacity(s.len());
        next.push(s[0]);
        next.extend(adj.windows(2).map(|w| (w[0] + w[1]).div_euclid(2)));
        next.push(*s.last().unwrap());
        s = next;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extrema_clipped() {
        assert_eq!(extrema(&[1, 5, 2, 0, 3], 1, true), vec![5, 5, 5, 3, 3]);
        assert_eq!(extrema(&[1, 5, 2, 0, 3], 1, false), vec![1, 1, 0, 0, 0]);
    }
    #[test]
    fn smooth_floor() {
        assert_eq!(smooth_trace(&[0, 3, 0, -3, 0], 1), vec![0, 1, -1, -2, 0]);
    }
}
