//! Port of `ratio_sizing._best_paths`: second-order dynamic program that keeps the two best paths per
//! (previous, current) state. The score arithmetic is evaluated in the reference order, so scores are
//! bit-identical; ties are resolved by the reference key `(-score, path)` (a total order on distinct paths).
use std::cmp::Ordering;

pub const MAX_COUNT: usize = 255;
pub const MAX_LEN: usize = 32;

#[derive(Clone, Copy)]
struct Item {
    score: f64,
    path: [u8; MAX_LEN],
}

/// `a` ranks before `b` under key `(-score, path)` (paths compared over the first `len` entries).
fn before(a: &Item, b: &Item, len: usize) -> bool {
    match (-a.score).partial_cmp(&(-b.score)) {
        Some(Ordering::Less) => true,
        Some(Ordering::Greater) => false,
        _ => a.path[..len] < b.path[..len],
    }
}

/// Keep the two best items in `slot` after offering `item` (the reference appends, sorts and keeps two).
fn offer(slot: &mut Vec<Item>, item: Item, len: usize) {
    if slot.len() < 2 {
        slot.push(item);
        if slot.len() == 2 && before(&slot[1], &slot[0], len) {
            slot.swap(0, 1);
        }
    } else if before(&item, &slot[1], len) {
        if before(&item, &slot[0], len) {
            slot[1] = slot[0];
            slot[0] = item;
        } else {
            slot[1] = item;
        }
    }
}

/// Returns `(score / (length - 2 * weight), path)` for up to two best complete assignments, plus the number of
/// transitions examined. Requires `scans.len() <= MAX_COUNT`, `bp.len() <= MAX_LEN`, `bp.len() >= 3`.
pub fn best_paths(scans: &[f64], heights: &[f64], bp: &[f64], weight: f64) -> (Vec<(f64, Vec<usize>)>, u64) {
    let count = scans.len();
    let length = bp.len();
    // The reference only takes the compiled fast path inside these limits; everything else is the plain DP.
    if !(count <= MAX_COUNT && (3..=MAX_LEN).contains(&length) && count >= length && count == heights.len()) {
        return best_paths_general(scans, heights, bp, weight);
    }
    let scale = heights.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let height_term: Vec<f64> = heights.iter().map(|h| (1.0 - weight) * (h / scale)).collect();
    let idx = |a: usize, b: usize| a * count + b;
    let mut states: Vec<Vec<Item>> = vec![Vec::new(); count * count];
    let mut live: Vec<(usize, usize)> = Vec::new();
    for b in 1..(count - length + 2) {
        for a in 0..b {
            let mut path = [0u8; MAX_LEN];
            path[0] = a as u8;
            path[1] = b as u8;
            states[idx(a, b)].push(Item { score: height_term[a] + height_term[b], path });
            live.push((a, b));
        }
    }
    let mut transitions: u64 = 0;
    for marker in 2..length {
        let target_ratio = (bp[marker] - bp[marker - 1]) / (bp[marker - 1] - bp[marker - 2]);
        let mut next: Vec<Vec<Item>> = vec![Vec::new(); count * count];
        let mut next_live: Vec<(usize, usize)> = Vec::new();
        for &(a, b) in &live {
            let previous_gap = scans[b] - scans[a];
            let prior = std::mem::take(&mut states[idx(a, b)]);
            for c in (b + 1)..(count - length + marker + 1) {
                transitions += 1;
                let scan_ratio = (scans[c] - scans[b]) / previous_gap;
                let x = scan_ratio / target_ratio;
                let y = target_ratio / scan_ratio;
                let agreement = if y < x { y } else { x }; // Python min(x, y): first argument on ties
                let slot = &mut next[idx(b, c)];
                if slot.is_empty() {
                    next_live.push((b, c));
                }
                for item in &prior {
                    let mut path = item.path;
                    path[marker] = c as u8;
                    offer(slot, Item { score: (item.score + weight * agreement) + height_term[c], path }, marker + 1);
                }
            }
        }
        states = next;
        live = next_live;
    }
    let mut all: Vec<Item> = Vec::new();
    for &(a, b) in &live {
        all.extend(states[idx(a, b)].iter().cloned());
    }
    all.sort_by(|p, q| {
        if before(p, q, length) {
            Ordering::Less
        } else if before(q, p, length) {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    });
    let denominator = length as f64 - 2.0 * weight;
    let out = all
        .iter()
        .take(2)
        .map(|it| (it.score / denominator, it.path[..length].iter().map(|&v| v as usize).collect()))
        .collect();
    (out, transitions)
}

/// Unbounded transcription of `_best_paths_py` (used when the fast path's size limits do not hold).
pub fn best_paths_general(scans: &[f64], heights: &[f64], bp: &[f64], weight: f64) -> (Vec<(f64, Vec<usize>)>, u64) {
    use std::collections::BTreeMap;
    let count = scans.len();
    let length = bp.len();
    let scale = heights.iter().cloned().fold(f64::NEG_INFINITY, |a, b| if b > a { b } else { a });
    let height_term: Vec<f64> = heights.iter().map(|h| (1.0 - weight) * (h / scale)).collect();
    let rank = |a: &(f64, Vec<usize>), b: &(f64, Vec<usize>)| (-a.0).partial_cmp(&(-b.0)).unwrap_or(Ordering::Equal).then_with(|| a.1.cmp(&b.1));
    let mut states: BTreeMap<(usize, usize), Vec<(f64, Vec<usize>)>> = BTreeMap::new();
    if count + 2 > length {
        for b in 1..(count + 2 - length) {
            for a in 0..b {
                states.insert((a, b), vec![(height_term[a] + height_term[b], vec![a, b])]);
            }
        }
    }
    let mut transitions: u64 = 0;
    for marker in 2..length {
        let target_ratio = (bp[marker] - bp[marker - 1]) / (bp[marker - 1] - bp[marker - 2]);
        let mut next: BTreeMap<(usize, usize), Vec<(f64, Vec<usize>)>> = BTreeMap::new();
        for (&(a, b), paths) in &states {
            let previous_gap = scans[b] - scans[a];
            if count + marker + 1 <= b + 1 + length {
                continue;
            }
            for c in (b + 1)..(count + marker + 1 - length) {
                transitions += 1;
                let scan_ratio = (scans[c] - scans[b]) / previous_gap;
                let x = scan_ratio / target_ratio;
                let y = target_ratio / scan_ratio;
                let agreement = if y < x { y } else { x };
                let options = next.entry((b, c)).or_default();
                for (score, path) in paths {
                    let mut p = path.clone();
                    p.push(c);
                    options.push(((score + weight * agreement) + height_term[c], p));
                }
                options.sort_by(rank);
                options.truncate(2);
            }
        }
        states = next;
    }
    let mut all: Vec<(f64, Vec<usize>)> = states.into_values().flatten().collect();
    all.sort_by(rank);
    all.truncate(2);
    let denominator = length as f64 - 2.0 * weight;
    (all.into_iter().map(|(s, p)| (s / denominator, p)).collect(), transitions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_best_two_deterministically() {
        let scans = [100.0, 200.0, 300.0, 400.0, 500.0, 610.0];
        let heights = [1000.0, 900.0, 950.0, 980.0, 970.0, 400.0];
        let bp = [100.0, 200.0, 300.0, 400.0];
        let (paths, transitions) = best_paths(&scans, &heights, &bp, 0.8);
        assert_eq!(paths.len(), 2);
        assert!(paths[0].0 >= paths[1].0);
        assert!(transitions > 0);
        assert_eq!(paths[0].1.len(), 4);
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Port of `ratio_sizing.match_ratio_standard`: bounded height/spacing ratio assignment with calibration gating.

use crate::analysis::Calibration;

pub const MAX_INPUT_PEAKS: usize = 4096;
pub const MAX_CANDIDATES: usize = 64;
pub const MAX_EXPECTED: usize = 32;
pub const SCORE_TOLERANCE: f64 = 1e-12;

#[derive(Debug, Clone, PartialEq)]
pub struct RatioCandidate {
    pub scan: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RatioMatched {
    pub candidate_index: usize,
    pub scan: f64,
    pub height: f64,
    pub expected_bp: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RatioSelection {
    pub input_count: usize,
    pub selected_count: usize,
    pub expected_count: usize,
    pub extra_peaks: usize,
    pub height_clip: f64,
    pub candidate_indices: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RatioDiagnostics {
    pub transitions_checked: u64,
    pub best_score: Option<f64>,
    pub second_best_score: Option<f64>,
    pub ambiguity_gap: Option<f64>,
    pub complete: bool,
    pub global_bp_per_scan: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RatioMatch {
    pub accepted: bool,
    pub reasons: Vec<String>,
    pub selection: Option<RatioSelection>,
    /// (expected prefix length, best score) for every attempted prefix.
    pub attempts: Vec<(usize, f64)>,
    pub matched: Vec<RatioMatched>,
    pub diagnostics: RatioDiagnostics,
    pub calibration: Option<Calibration>,
}

fn number_ok(v: f64) -> bool {
    v.is_finite()
}

/// Python `match_ratio_standard`. Candidate scans need not be sorted; `candidate_index` is the input position.
pub fn match_ratio_standard(
    peaks: &[RatioCandidate],
    expected_bp: &[f64],
    num_extra_peaks: usize,
    ratio_factor: f64,
    min_accepted_score: f64,
    ambiguity_margin: f64,
    allow_prefix: bool,
    max_bp_per_scan: Option<f64>,
) -> RatioMatch {
    let mut r = RatioMatch {
        accepted: false,
        reasons: Vec::new(),
        selection: None,
        attempts: Vec::new(),
        matched: Vec::new(),
        diagnostics: RatioDiagnostics {
            transitions_checked: 0,
            best_score: None,
            second_best_score: None,
            ambiguity_gap: None,
            complete: false,
            global_bp_per_scan: None,
        },
        calibration: None,
    };
    macro_rules! reject {
        ($m:expr) => {{
            r.reasons.push($m.to_string());
            return r;
        }};
    }
    if !number_ok(ratio_factor) || !(0.0..=1.0).contains(&ratio_factor) {
        reject!("ratio_factor must be finite and in [0, 1]");
    }
    if !number_ok(min_accepted_score) || !(min_accepted_score > 0.0 && min_accepted_score <= 1.0) {
        reject!("min_accepted_score must be finite and in (0, 1]");
    }
    if !number_ok(ambiguity_margin) || !(0.0..=1.0).contains(&ambiguity_margin) {
        reject!("ambiguity_margin must be finite and in [0, 1]");
    }
    if let Some(m) = max_bp_per_scan {
        if !number_ok(m) || !(m > 0.0 && m <= 100.0) {
            reject!("max_bp_per_scan must be None or finite and in (0, 100]");
        }
    }
    if num_extra_peaks > 32 {
        reject!("num_extra_peaks must be an integer in [0, 32]");
    }
    if !(3..=MAX_EXPECTED).contains(&expected_bp.len()) {
        reject!("Expected 3..32 explicit fragment sizes");
    }
    if expected_bp.iter().any(|&x| !number_ok(x) || !(x > 0.0 && x <= 1_000_000.0)) {
        reject!("Expected fragment sizes must be finite and in (0, 1000000]");
    }
    let bp = expected_bp;
    if bp.windows(2).any(|w| w[1] - w[0] < 1e-6) {
        reject!("Expected fragment sizes must increase with gaps >= 0.000001 bp");
    }
    if peaks.is_empty() || peaks.len() > MAX_INPUT_PEAKS {
        reject!("Candidates must be an explicit list or tuple of 1..4096 peaks");
    }
    for (index, p) in peaks.iter().enumerate() {
        if !number_ok(p.scan) || !(0.0..=100_000_000.0).contains(&p.scan) || !number_ok(p.height)
            || !(p.height > 0.0 && p.height <= 1_000_000_000_000.0)
        {
            reject!(format!("Candidate {}: bounded finite nonnegative scan and positive height required", index));
        }
    }
    // (original index, scan, height), sorted by scan (stable).
    let mut candidates: Vec<(usize, f64, f64)> = peaks.iter().enumerate().map(|(i, p)| (i, p.scan, p.height)).collect();
    candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    if candidates.windows(2).any(|w| w[1].1 - w[0].1 < 1e-6) {
        reject!("Candidate scans must increase with gaps >= 0.000001 scans");
    }
    let expected_count = bp.len();
    if candidates.len() < expected_count {
        reject!("Fewer candidates than the full expected standard");
    }
    let limit = expected_count + num_extra_peaks;
    if limit > MAX_CANDIDATES {
        reject!("Selected candidate limit exceeds 64");
    }
    // Rank by (height, position in sorted order); Python ranks the sorted list indices.
    let mut ranked: Vec<usize> = (0..candidates.len()).collect();
    ranked.sort_by(|&a, &b| candidates[a].2.partial_cmp(&candidates[b].2).unwrap().then(a.cmp(&b)));
    let n = candidates.len();
    let cutoff = candidates[ranked[n - expected_count]].2;
    let take = limit.min(n);
    let mut chosen: Vec<usize> = ranked[n - take..].to_vec();
    chosen.sort_unstable();
    let selected: Vec<(usize, f64, f64)> = chosen.iter().map(|&i| candidates[i]).collect();
    r.selection = Some(RatioSelection {
        input_count: n,
        selected_count: selected.len(),
        expected_count,
        extra_peaks: num_extra_peaks,
        height_clip: cutoff,
        candidate_indices: selected.iter().map(|s| s.0).collect(),
    });
    let heights: Vec<f64> = selected.iter().map(|s| s.2.min(cutoff)).collect();
    let scans: Vec<f64> = selected.iter().map(|s| s.1).collect();
    let lower = if allow_prefix && ratio_factor < 1.0 {
        2
    } else if allow_prefix {
        3
    } else {
        expected_count
    };
    let mut length = expected_count;
    let choices = loop {
        let (c, transitions) = best_paths(&scans, &heights, &bp[..length], ratio_factor);
        r.diagnostics.transitions_checked += transitions;
        let best = c[0].0;
        r.attempts.push((length, best));
        if best + SCORE_TOLERANCE >= min_accepted_score || !allow_prefix || length == lower {
            break c;
        }
        length -= 1;
    };
    let (best, path) = (choices[0].0, choices[0].1.clone());
    let second = choices.get(1).map(|c| c.0);
    let gap = second.map(|s| best - s);
    r.matched = path
        .iter()
        .enumerate()
        .map(|(k, &index)| RatioMatched {
            candidate_index: selected[index].0,
            scan: selected[index].1,
            height: selected[index].2,
            expected_bp: bp[k],
        })
        .collect();
    let rate = (bp[path.len() - 1] - bp[0]) / (scans[*path.last().unwrap()] - scans[path[0]]);
    r.diagnostics.best_score = Some(best);
    r.diagnostics.second_best_score = second;
    r.diagnostics.ambiguity_gap = gap;
    r.diagnostics.complete = path.len() == expected_count;
    r.diagnostics.global_bp_per_scan = Some(rate);
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
    if let Some(m) = max_bp_per_scan {
        if rate > m {
            reject!("Assignment exceeds the explicit migration-rate bound; calibration withheld");
        }
    }
    let anchors: Vec<(f64, f64)> = r.matched.iter().map(|m| (m.scan, m.expected_bp)).collect();
    r.calibration = Calibration::new(&anchors).ok();
    r.accepted = true;
    r
}
