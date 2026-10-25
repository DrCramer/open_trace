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
