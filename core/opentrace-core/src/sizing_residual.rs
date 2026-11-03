//! Ports of `analysis.find_peaks`/`parse_anchors` and `sizing.py` (residual-engine size-standard matching),
//! producing the reference's dict structure (key order, int/float types).
use crate::abif::Num;
use crate::analysis::Calibration;
use crate::profiles::{py_float_parse, py_isspace_pub, strip};
use crate::pydump::str_repr;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

type Obj = Map<String, Value>;

fn fv(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

pub fn num_json(n: Num) -> Value {
    match n {
        Num::Int(i) => json!(i),
        Num::Float(f) => fv(f),
    }
}

pub fn num_f(n: Num) -> f64 {
    match n {
        Num::Int(i) => i as f64,
        Num::Float(f) => f,
    }
}

fn num_lt(a: Num, b: Num) -> bool {
    num_f(a) < num_f(b)
}

/// `a - b` with Python int/float semantics.
fn num_sub(a: Num, b: Num) -> Num {
    match (a, b) {
        (Num::Int(x), Num::Int(y)) => Num::Int(x - y),
        _ => Num::Float(num_f(a) - num_f(b)),
    }
}

/// `statistics.median` of a nonempty list (middle element, or `(a + b) / 2` as a float for even length).
fn median(block: &[Num]) -> Num {
    let mut d = block.to_vec();
    d.sort_by(|a, b| num_f(*a).partial_cmp(&num_f(*b)).unwrap());
    let n = d.len();
    if n % 2 == 1 {
        d[n / 2]
    } else {
        let (a, b) = (d[n / 2 - 1], d[n / 2]);
        // (a + b) / 2 : true division
        match (a, b) {
            (Num::Int(x), Num::Int(y)) => Num::Float(crate::numeric::int_true_div((x + y) as i128, 2)),
            _ => Num::Float((num_f(a) + num_f(b)) / 2.0),
        }
    }
}

/// CPython 3.12+ `sum()` over ints and floats (exact int path until the first float, then Neumaier).
fn py_sum_nums(items: &[Num]) -> Num {
    let mut i_result: i64 = 0;
    let mut idx = 0;
    while idx < items.len() {
        match items[idx] {
            Num::Int(v) => {
                i_result += v;
                idx += 1;
            }
            Num::Float(_) => break,
        }
    }
    if idx == items.len() {
        return Num::Int(i_result);
    }
    // The first float is added by plain `int + float` (no compensation term), as in CPython.
    let first = match items[idx] {
        Num::Float(x) => x,
        Num::Int(_) => unreachable!(),
    };
    let mut f = i_result as f64 + first;
    let mut c = 0.0f64;
    for item in &items[idx + 1..] {
        match *item {
            Num::Float(x) => {
                let t = f + x;
                if f.abs() >= x.abs() {
                    c += (f - t) + x;
                } else {
                    c += (x - t) + f;
                }
                f = t;
            }
            Num::Int(v) => f += v as f64,
        }
    }
    if c != 0.0 && c.is_finite() {
        f += c;
    }
    Num::Float(f)
}

/// `find_peaks(values, threshold, min_distance, baseline_window=101)` -> list of peak dicts.
pub fn find_peaks(values: &[Num], threshold: f64, min_distance: f64, baseline_window: i64) -> Result<Vec<Value>, String> {
    if !threshold.is_finite() || threshold < 0.0 {
        return Err("Threshold must be finite and nonnegative".into());
    }
    if min_distance < 1.0 || baseline_window < 3 {
        return Err("Distance >= 1 and baseline window >= 3 required".into());
    }
    if values.iter().any(|v| matches!(v, Num::Float(f) if !f.is_finite())) {
        return Err("Non-finite trace values".into());
    }
    let bw = baseline_window as usize;
    let n = values.len();
    let mut blocks: Vec<(f64, Num)> = Vec::new();
    let mut start = 0;
    while start < n {
        let block = &values[start..(start + bw).min(n)];
        blocks.push((start as f64 + (block.len() as f64 - 1.0) / 2.0, median(block)));
        start += bw;
    }
    let mut corrected: Vec<Num> = Vec::with_capacity(n);
    let mut segment = 0usize;
    for (scan, &value) in values.iter().enumerate() {
        while segment + 1 < blocks.len() && scan as f64 > blocks[segment + 1].0 {
            segment += 1;
        }
        let (x0, y0) = blocks[segment];
        if segment + 1 < blocks.len() && scan as f64 > x0 {
            let (x1, y1) = blocks[segment + 1];
            let y0f = num_f(y0);
            let baseline = y0f + (scan as f64 - x0) / (x1 - x0) * num_f(num_sub(y1, y0));
            corrected.push(Num::Float(num_f(value) - baseline));
        } else {
            corrected.push(num_sub(value, y0));
        }
    }
    let c: Vec<f64> = corrected.iter().map(|&v| num_f(v)).collect();
    let mut candidates: Vec<usize> = Vec::new();
    let mut i = 1usize;
    while i + 1 < c.len() {
        if c[i] >= threshold && c[i] > c[i - 1] {
            let mut end = i;
            while end + 1 < c.len() && c[end + 1] == c[i] {
                end += 1;
            }
            if end + 1 < c.len() && c[end] > c[end + 1] {
                candidates.push((i + end) / 2);
            }
            i = end + 1;
        } else {
            i += 1;
        }
    }
    let mut order = candidates.clone();
    order.sort_by(|&a, &b| (-c[a]).partial_cmp(&(-c[b])).unwrap().then(a.cmp(&b)));
    let mut chosen: Vec<usize> = Vec::new();
    for scan in order {
        if chosen.iter().all(|&p| (scan as f64 - p as f64).abs() >= min_distance) {
            chosen.push(scan);
        }
    }
    chosen.sort_unstable();
    let mut peaks = Vec::new();
    for scan in chosen {
        let (mut left, mut right) = (scan, scan);
        while left > scan.saturating_sub(100) && c[left - 1] > 0.0 {
            left -= 1;
        }
        while right < (n - 1).min(scan + 100) && c[right + 1] > 0.0 {
            right += 1;
        }
        let slice: Vec<Num> = corrected[left..=right]
            .iter()
            .map(|&v| if num_f(v) > 0.0 { v } else { Num::Int(0) })
            .collect();
        let area = py_sum_nums(&slice);
        peaks.push(json!({"scan": scan, "height": num_json(corrected[scan]), "raw_height": num_json(values[scan]),
            "area": num_json(area), "width_scans": right - left + 1}));
    }
    Ok(peaks)
}

/// `parse_anchors(text)`: `None` for blank text.
pub fn parse_anchors(text: &str) -> Result<Option<Calibration>, String> {
    if strip(text).is_empty() {
        return Ok(None);
    }
    let mut anchors: Vec<(f64, f64)> = Vec::new();
    for item in text.replace('\n', ",").split(',') {
        if !strip(item).is_empty() {
            let parts: Vec<&str> = item.split(':').collect();
            if parts.len() != 2 {
                return Err("Use scan:bp pairs separated by commas".into());
            }
            let mut v = [0f64; 2];
            for (k, p) in parts.iter().enumerate() {
                v[k] = py_float_parse(p).ok_or_else(|| format!("could not convert string to float: {}", str_repr(p)))?;
            }
            anchors.push((v[0], v[1]));
        }
    }
    Calibration::new(&anchors).map(Some)
}

// ---------------------------------------------------------------------------------------- sizing.py

const MAX_CANDIDATES: usize = 64;
const MAX_EXPECTED: usize = 32;
const MIN_EXPECTED: usize = 4;
const MAX_SCAN: f64 = 100_000_000.0;
const MAX_BP: f64 = 1_000_000.0;

fn finite_number(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Number(n)) => n.as_f64().filter(|f| f.is_finite()),
        _ => None,
    }
}

/// `parse_expected_bp(text)`.
pub fn parse_expected_bp(text: &str) -> Result<Vec<f64>, String> {
    if text.chars().count() > 4096 {
        return Err("Expected fragment text must be a string of at most 4096 characters".into());
    }
    let t = strip(text);
    // re.split(r"[,;\s]+", t, maxsplit=32)
    let is_sep = |c: char| c == ',' || c == ';' || py_isspace_pub(c);
    let mut parts: Vec<String> = Vec::new();
    let chars: Vec<char> = t.chars().collect();
    let mut cur = String::new();
    let mut splits = 0;
    let mut i = 0;
    while i < chars.len() {
        if splits < MAX_EXPECTED && is_sep(chars[i]) {
            while i < chars.len() && is_sep(chars[i]) {
                i += 1;
            }
            parts.push(std::mem::take(&mut cur));
            splits += 1;
        } else {
            cur.push(chars[i]);
            i += 1;
        }
    }
    parts.push(cur);
    if !(MIN_EXPECTED..=MAX_EXPECTED).contains(&parts.len()) {
        return Err(format!("Enter {}..{} expected fragment sizes", MIN_EXPECTED, MAX_EXPECTED));
    }
    let mut sizes = Vec::new();
    for p in &parts {
        match py_float_parse(p) {
            Some(f) => sizes.push(f),
            None => return Err("Expected fragment sizes must be numbers separated by commas or whitespace".into()),
        }
    }
    if sizes.iter().any(|&bp| !bp.is_finite() || !(0.0 < bp && bp <= MAX_BP)) {
        return Err("Expected fragment sizes must be finite, positive and <= 1000000 bp".into());
    }
    if sizes.windows(2).any(|w| w[1] <= w[0]) {
        return Err("Expected fragment sizes must increase strictly; duplicates are invalid".into());
    }
    Ok(sizes)
}

/// Python `format(x, "g")` (6 significant digits).
pub fn fmt_g(x: f64) -> String {
    if x == 0.0 {
        return if x.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x < 0.0 { "-inf".into() } else { "inf".into() };
    }
    let e = format!("{:.5e}", x);
    let (mant, exp) = e.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    if exp < -4 || exp >= 6 {
        let m = if mant.contains('.') { mant.trim_end_matches('0').trim_end_matches('.') } else { mant };
        format!("{}e{}{:02}", m, if exp < 0 { '-' } else { '+' }, exp.abs())
    } else {
        let s = format!("{:.*}", (5 - exp).max(0) as usize, x);
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s
        }
    }
}

fn error_bp(ls: f64, ms: f64, rs: f64, lb: f64, mb: f64, rb: f64) -> f64 {
    lb + (ms - ls) / (rs - ls) * (rb - lb) - mb
}

#[derive(Clone)]
struct Cand {
    index: usize,
    scan: f64,
    height: Option<f64>,
}

fn cand_json(c: &Cand) -> Obj {
    let mut m = Obj::new();
    m.insert("candidate_index".into(), json!(c.index));
    m.insert("scan".into(), fv(c.scan));
    m.insert("height".into(), c.height.map(fv).unwrap_or(Value::Null));
    m
}

pub struct SizingOut {
    pub result: Value,
    pub calibration: Option<Calibration>,
}

/// `match_size_standard(peaks, expected_bp, max_residual_bp=..., ambiguity_margin_bp=0.5)`.
pub fn match_size_standard(peaks: &Value, expected_bp: &Value, max_residual_bp: &Value, ambiguity_margin_bp: &Value) -> SizingOut {
    let mut diag = Obj::new();
    diag.insert("method".into(), json!("ordered_second_order_dp"));
    diag.insert("quality_metric".into(), json!("interior_leave_one_out_linear_interpolation_bp"));
    diag.insert("external_validation".into(), json!(false));
    diag.insert("max_candidates".into(), json!(MAX_CANDIDATES));
    diag.insert("max_expected".into(), json!(MAX_EXPECTED));
    diag.insert("transitions_checked".into(), json!(0));
    let mut reasons: Vec<String> = vec![];
    let mut anchors: Vec<Value> = vec![];
    let mut candidate_count = 0usize;
    let mut expected_count = 0usize;
    let mut matched: Vec<Value> = vec![];
    let mut unmatched: Vec<Value> = vec![];
    let mut calibration: Option<Calibration> = None;
    let mut status = "rejected";
    macro_rules! out {
        () => {{
            return SizingOut {
                result: json!({"status": status, "reasons": reasons, "anchors": anchors, "candidate_count": candidate_count,
                    "expected_count": expected_count, "matched": matched, "unmatched": unmatched,
                    "diagnostics": Value::Object(diag), "calibration": Value::Null}),
                calibration,
            };
        }};
    }
    macro_rules! reject {
        ($m:expr) => {{
            reasons.push($m.to_string());
            out!();
        }};
    }
    let max_res = match finite_number(Some(max_residual_bp)) {
        Some(m) if m > 0.0 && m <= 100.0 => m,
        _ => reject!("max_residual_bp must be finite and in (0, 100]"),
    };
    let margin = match finite_number(Some(ambiguity_margin_bp)) {
        Some(m) if (0.0..=100.0).contains(&m) => m,
        _ => reject!("ambiguity_margin_bp must be finite and in [0, 100]"),
    };
    diag.insert("residual_limit_bp".into(), fv(max_res));
    diag.insert("ambiguity_margin_bp".into(), fv(margin));
    let exp = match expected_bp {
        Value::Array(a) => a,
        _ => reject!("expected_bp must be an explicit bounded list or tuple"),
    };
    expected_count = exp.len();
    if !(MIN_EXPECTED..=MAX_EXPECTED).contains(&exp.len()) {
        reject!(format!("Expected {}..{} fragment sizes", MIN_EXPECTED, MAX_EXPECTED));
    }
    let mut bp: Vec<f64> = Vec::new();
    for v in exp {
        match finite_number(Some(v)) {
            Some(f) if 0.0 < f && f <= MAX_BP => bp.push(f),
            _ => reject!("Expected fragment sizes must be finite, positive and <= 1000000 bp"),
        }
    }
    if bp.windows(2).any(|w| w[1] <= w[0]) {
        reject!("Expected fragment sizes must increase strictly; duplicates are invalid");
    }
    let pk = match peaks {
        Value::Array(a) => a,
        _ => reject!("Candidates must be an explicit bounded list or tuple"),
    };
    candidate_count = pk.len();
    if pk.len() > MAX_CANDIDATES {
        reject!(format!("More than {} candidates; select a scan interval or threshold explicitly", MAX_CANDIDATES));
    }
    if pk.len() < bp.len() {
        reject!("Fewer candidate peaks than expected fragments; missing markers are not permitted");
    }
    let mut cands: Vec<Cand> = Vec::new();
    for (index, p) in pk.iter().enumerate() {
        let obj = match p {
            Value::Object(o) => o,
            _ => reject!(format!("Candidate {}: finite numeric scan required", index)),
        };
        let scan = match finite_number(obj.get("scan")) {
            Some(s) => s,
            None => reject!(format!("Candidate {}: finite numeric scan required", index)),
        };
        if !(0.0 <= scan && scan <= MAX_SCAN) {
            reject!(format!("Candidate {}: scan must be in [0, {}]", index, MAX_SCAN as i64));
        }
        let height = match obj.get("height") {
            None | Some(Value::Null) => None,
            h => match finite_number(h) {
                Some(f) => Some(f),
                None => reject!(format!("Candidate {}: height must be finite when supplied", index)),
            },
        };
        cands.push(Cand { index, scan, height });
    }
    cands.sort_by(|a, b| a.scan.partial_cmp(&b.scan).unwrap());
    if cands.windows(2).any(|w| w[1].scan <= w[0].scan) {
        reject!("Candidate scans must be distinct; duplicates are invalid");
    }
    unmatched = cands.iter().map(|c| Value::Object(cand_json(c))).collect();
    let scans: Vec<f64> = cands.iter().map(|c| c.scan).collect();
    let (m, n) = (cands.len(), bp.len());
    let mut transitions: u64 = 0;
    let mut states: BTreeMap<(usize, usize), Vec<(f64, Vec<usize>)>> = BTreeMap::new();
    for a in 0..(m - n + 1) {
        for b in (a + 1)..(m - n + 2) {
            states.insert((a, b), vec![(0.0, vec![a, b])]);
        }
    }
    for marker in 2..n {
        let mut next: BTreeMap<(usize, usize), Vec<(f64, Vec<usize>)>> = BTreeMap::new();
        let max_next = m - (n - marker) + 1;
        for (&(a, b), paths) in &states {
            for c in (b + 1)..max_next {
                transitions += 1;
                let error = error_bp(scans[a], scans[b], scans[c], bp[marker - 2], bp[marker - 1], bp[marker]);
                if error.abs() > max_res {
                    continue;
                }
                let options = next.entry((b, c)).or_default();
                for (cost, path) in paths {
                    let mut np = path.clone();
                    np.push(c);
                    if options.iter().any(|(_, old)| *old == np) {
                        continue;
                    }
                    options.push((cost + error * error, np));
                    options.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap().then_with(|| x.1.cmp(&y.1)));
                    options.truncate(2);
                }
            }
        }
        states = next;
        diag.insert("transitions_checked".into(), json!(transitions));
        if states.is_empty() {
            reject!("No complete ordered assignment satisfies the leave-one-out residual limit; missing markers, distorted spacing or unsuitable candidates");
        }
    }
    diag.insert("transitions_checked".into(), json!(transitions));
    let mut solutions: Vec<(f64, Vec<usize>)> = states.into_values().flatten().collect();
    solutions.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap().then_with(|| x.1.cmp(&y.1)));
    if solutions.is_empty() {
        reject!("No complete ordered assignment");
    }
    let (cost, path) = solutions[0].clone();
    let residuals: Vec<f64> = (1..n - 1)
        .map(|k| error_bp(scans[path[k - 1]], scans[path[k]], scans[path[k + 1]], bp[k - 1], bp[k], bp[k + 1]))
        .collect();
    let rms = (cost / (n - 2) as f64).sqrt();
    let mut hold: Vec<Value> = vec![Value::Null];
    hold.extend(residuals.iter().map(|&r| fv(r)));
    hold.push(Value::Null);
    let max_abs = residuals.iter().fold(f64::NEG_INFINITY, |a, r| if r.abs() > a { r.abs() } else { a });
    diag.insert("rms_error_bp".into(), fv(rms));
    diag.insert("max_abs_error_bp".into(), fv(max_abs));
    diag.insert("holdout_count".into(), json!(n - 2));
    diag.insert("endpoint_holdout_available".into(), json!(false));
    diag.insert("holdout_residuals_bp".into(), Value::Array(hold.clone()));
    diag.insert("scan_span".into(), fv(scans[path[n - 1]] - scans[path[0]]));
    diag.insert("bp_span".into(), fv(bp[n - 1] - bp[0]));
    diag.insert("second_best_rms_error_bp".into(), Value::Null);
    diag.insert("ambiguity_gap_bp".into(), Value::Null);
    matched = path
        .iter()
        .enumerate()
        .map(|(k, &pi)| {
            let mut d = cand_json(&cands[pi]);
            d.insert("expected_bp".into(), fv(bp[k]));
            d.insert("holdout_error_bp".into(), hold[k].clone());
            Value::Object(d)
        })
        .collect();
    unmatched = cands.iter().enumerate().filter(|(k, _)| !path.contains(k)).map(|(_, c)| Value::Object(cand_json(c))).collect();
    if solutions.len() > 1 {
        let (second_cost, second_path) = solutions[1].clone();
        let second_rms = (second_cost / (n - 2) as f64).sqrt();
        let gap = second_rms - rms;
        diag.insert("second_best_rms_error_bp".into(), fv(second_rms));
        diag.insert("ambiguity_gap_bp".into(), fv(gap));
        diag.insert(
            "alternative_match".into(),
            Value::Array(
                second_path
                    .iter()
                    .enumerate()
                    .map(|(k, &i)| {
                        let mut d = cand_json(&cands[i]);
                        d.insert("expected_bp".into(), fv(bp[k]));
                        Value::Object(d)
                    })
                    .collect(),
            ),
        );
        if gap <= margin + 1e-12 {
            let min_height = |idx: &[usize]| -> Option<f64> {
                let hs: Vec<Option<f64>> = idx.iter().map(|&i| cands[i].height).collect();
                if hs.iter().all(|h| h.is_some()) {
                    Some(hs.iter().map(|h| h.unwrap()).fold(f64::INFINITY, f64::min))
                } else {
                    None
                }
            };
            let (best_min, alt_min) = (min_height(&path), min_height(&second_path));
            match (best_min, alt_min) {
                (Some(b), Some(a)) if b > a => {
                    diag.insert("ambiguity_resolved_by_height".into(), json!({"best_min_height": fv(b), "alternative_min_height": fv(a)}));
                }
                _ => reject!(format!(
                    "Ambiguous ordered assignments: alternative RMS is within {} bp of the best; calibration withheld",
                    fmt_g(margin)
                )),
            }
        }
    }
    let pairs: Vec<(f64, f64)> = path.iter().enumerate().map(|(k, &i)| (scans[i], bp[k])).collect();
    anchors = pairs.iter().map(|&(s, b)| json!([fv(s), fv(b)])).collect();
    calibration = Calibration::new(&pairs).ok();
    status = "accepted";
    out!()
}
