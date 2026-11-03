//! Port of `genemapper_open/ladder.py::match_ladder` (independent full ladder matching and per-bin offsets).
//!
//! Known gap: Python TypeError paths inside kit/peak validation (non-mapping JSON shapes that raise TypeError
//! rather than ValueError) produce a different message text here.
use crate::kit_definition::load_kit_definition;
use crate::pystats;
use crate::ratio::{best_paths, SCORE_TOLERANCE};
use crate::run_offsets::{ladder_window_offset, validate_run_offset_state};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

pub type Res<T> = Result<T, String>;

const MAX_INPUT_PEAKS: usize = 4096;
const MAX_MATCH_CANDIDATES: usize = 64;
const RATIO_FACTOR: f64 = 0.6;
const MIN_MATCH_SCORE: f64 = 0.75;

fn fv(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

fn finite(v: Option<&Value>, low: f64, high: f64) -> bool {
    match v {
        Some(Value::Number(n)) => n.as_f64().map_or(false, |f| f.is_finite() && low <= f && f <= high),
        _ => false,
    }
}

fn flag(peak: &Map<String, Value>, name: &str) -> Res<bool> {
    let flags = peak.get("flags").cloned().unwrap_or_else(|| json!({}));
    let nested = match &flags {
        Value::Object(m) => match m.get(name) {
            None => false,
            Some(Value::Bool(b)) => *b,
            Some(_) => return Err(format!("flags.{} must be boolean", name)),
        },
        Value::Array(l) if l.iter().all(|f| f.is_string()) => l.iter().any(|f| f == name),
        _ => return Err("Peak flags must be an object of booleans or a list of flag names".into()),
    };
    let direct = match peak.get(name) {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(format!("Peak {} must be boolean", name)),
    };
    Ok(direct || nested)
}

#[derive(Clone)]
struct Row {
    candidate_index: usize,
    scan: f64,
    size_bp: f64,
    height: f64,
    spike: bool,
    pullup: bool,
}

impl Row {
    fn json(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("candidate_index".into(), json!(self.candidate_index));
        m.insert("scan".into(), fv(self.scan));
        m.insert("size_bp".into(), fv(self.size_bp));
        m.insert("height".into(), fv(self.height));
        m.insert("spike".into(), json!(self.spike));
        m.insert("pullup".into(), json!(self.pullup));
        m
    }
}

fn valid_channel_name(c: &str) -> bool {
    match c.strip_prefix("DATA") {
        Some(d) => (1..=6).contains(&d.len()) && d.bytes().all(|b| b.is_ascii_digit()) && !d.starts_with('0'),
        None => false,
    }
}

fn channels(source: &Value) -> Res<Vec<(String, Vec<Row>)>> {
    let mut items: Vec<(String, Option<Vec<&Value>>)> = Vec::new();
    match source {
        Value::Array(list) => {
            if list.len() > MAX_INPUT_PEAKS {
                return Err("At most 4096 total sized peaks are supported".into());
            }
            for peak in list {
                let ch = match peak.as_object().and_then(|p| p.get("channel")).and_then(|c| c.as_str()) {
                    Some(c) => c,
                    None => return Err("Flat peaks require an explicit channel on every peak".into()),
                };
                match items.iter_mut().find(|(c, _)| c == ch) {
                    Some((_, Some(v))) => v.push(peak),
                    _ => items.push((ch.to_string(), Some(vec![peak]))),
                }
            }
        }
        Value::Object(m) if m.len() <= 128 => {
            for (k, v) in m {
                items.push((k.clone(), v.as_array().map(|l| l.iter().collect())));
            }
        }
        _ => return Err("Peaks require a channel mapping or an explicit flat list".into()),
    }
    let mut out: Vec<(String, Vec<Row>)> = Vec::new();
    let mut total = 0usize;
    for (channel, peaks) in items {
        if !valid_channel_name(&channel) {
            return Err("Peak channel must be a canonical DATA number".into());
        }
        let peaks = peaks.ok_or_else(|| format!("{}: explicit peak list required", channel))?;
        total += peaks.len();
        if total > MAX_INPUT_PEAKS {
            return Err("At most 4096 total sized peaks are supported".into());
        }
        let mut rows: Vec<Row> = Vec::new();
        for (index, peak) in peaks.iter().enumerate() {
            let p = match peak.as_object() {
                Some(p) if finite(p.get("scan"), 0.0, 100_000_000.0) && finite(p.get("size_bp"), 1e-300, 1_000_000.0) && finite(p.get("height"), 5e-324, 1e12) => p,
                _ => return Err(format!("{} peak {}: bounded finite scan, positive size_bp and height required", channel, index)),
            };
            if let Some(c) = p.get("channel") {
                if c != &json!(channel) {
                    return Err("Peak channel disagrees with its channel mapping".into());
                }
            }
            rows.push(Row {
                candidate_index: index,
                scan: p["scan"].as_f64().unwrap(),
                size_bp: p["size_bp"].as_f64().unwrap(),
                height: p["height"].as_f64().unwrap(),
                spike: flag(p, "spike")?,
                pullup: flag(p, "pullup")?,
            });
        }
        rows.sort_by(|a, b| a.scan.partial_cmp(&b.scan).unwrap());
        if rows.windows(2).any(|w| w[1].scan - w[0].scan < 1e-6 || w[1].size_bp - w[0].size_bp < 1e-6) {
            return Err(format!("{}: scan and size order must both increase with gaps >= 0.000001", channel));
        }
        match out.iter_mut().find(|(c, _)| c == &channel) {
            Some(e) => e.1 = rows,
            None => out.push((channel, rows)),
        }
    }
    Ok(out)
}

fn match_marker(peaks: &[Row], marker: &Value, low: f64, high: f64, margin: f64) -> Res<Map<String, Value>> {
    let mut result = Map::new();
    result.insert("status".into(), json!("rejected"));
    result.insert("reasons".into(), json!([]));
    result.insert("matched".into(), json!([]));
    result.insert("bin_offsets".into(), json!({}));
    result.insert("min_offset_bp".into(), Value::Null);
    result.insert("max_offset_bp".into(), Value::Null);
    result.insert("confidence".into(), Value::Null);
    result.insert("window".into(), json!({"min_bp": fv(low), "max_bp": fv(high), "inclusive": true}));
    let mut diag = Map::new();
    diag.insert("external_validation".into(), json!(false));
    diag.insert("score_is_original_sq".into(), json!(false));
    diag.insert("ratio_factor".into(), fv(RATIO_FACTOR));
    diag.insert("min_match_score".into(), fv(MIN_MATCH_SCORE));
    diag.insert("ambiguity_margin".into(), fv(margin));
    diag.insert("ambiguity_policy".into(), json!("reject best-minus-runner-up <= margin + 1e-12"));
    diag.insert("migration_rate_gate".into(), Value::Null);
    diag.insert("height_clipping".into(), json!(false));
    macro_rules! reject {
        ($reason:expr, $diag:expr) => {{
            result.insert("reasons".into(), json!([$reason]));
            result.insert("diagnostics".into(), Value::Object($diag));
            return Ok(result);
        }};
    }
    if low > high {
        reject!("Invalid ladder window after applying previous marker offset", diag);
    }
    let labels: Vec<&str> = marker["ladder_labels"].as_array().unwrap().iter().map(|l| l.as_str().unwrap()).collect();
    if labels.len() < 2 {
        reject!("At least two explicit ladder labels are required by this ratio objective", diag);
    }
    let bins: HashMap<&str, &Value> = marker["bins"].as_array().unwrap().iter().map(|b| (b["label"].as_str().unwrap(), b)).collect();
    let centers: Vec<&Value> = labels.iter().map(|l| &bins[l]["center_bp"]).collect();
    let expected: Vec<f64> = centers.iter().map(|c| c.as_f64().unwrap()).collect();
    let window: Vec<&Row> = peaks.iter().filter(|p| low <= p.size_bp && p.size_bp <= high).collect();
    let candidates: Vec<&Row> = window.iter().copied().filter(|p| !p.spike && !p.pullup).collect();
    diag.insert("window_count".into(), json!(window.len()));
    diag.insert("flag_excluded_count".into(), json!(window.len() - candidates.len()));
    diag.insert("expected_count".into(), json!(expected.len()));
    if candidates.is_empty() {
        reject!("No unflagged peaks in the inclusive ladder window", diag);
    }
    let max_h = candidates.iter().map(|p| p.height).fold(f64::NEG_INFINITY, |a, b| if b > a { b } else { a });
    let cutoff = max_h * marker["parameters"]["ladder_cutoff_ratio"].as_f64().unwrap();
    let mut candidates: Vec<&Row> = candidates.into_iter().filter(|p| p.height >= cutoff).collect();
    let cutoff_count = candidates.len();
    let mut cap_height: Option<f64> = None;
    if candidates.len() > 2 * expected.len() {
        let mut hs: Vec<f64> = candidates.iter().map(|p| p.height).collect();
        hs.sort_by(|a, b| b.partial_cmp(a).unwrap());
        let cap = hs[2 * expected.len() - 1];
        cap_height = Some(cap);
        candidates.retain(|p| p.height >= cap);
    }
    diag.insert("cutoff_height".into(), fv(cutoff));
    diag.insert("after_cutoff_count".into(), json!(cutoff_count));
    diag.insert("top_2n_height".into(), cap_height.map_or(Value::Null, fv));
    diag.insert("selected_count".into(), json!(candidates.len()));
    diag.insert("candidate_indices".into(), json!(candidates.iter().map(|p| p.candidate_index).collect::<Vec<_>>()));
    diag.insert("top_2n_policy".into(), json!("height cutoff; all ties retained"));
    if candidates.len() < expected.len() {
        reject!("Fewer ladder candidates than explicit expected labels; no partial match emitted", diag);
    }
    if candidates.len() > MAX_MATCH_CANDIDATES {
        reject!("Top-2N cutoff ties exceed the independent 64-candidate computation bound", diag);
    }
    let sizes: Vec<f64> = candidates.iter().map(|p| p.size_bp).collect();
    let heights: Vec<f64> = candidates.iter().map(|p| p.height).collect();
    let (choices, transitions) = best_paths(&sizes, &heights, &expected, RATIO_FACTOR);
    let (best, path) = (choices[0].0, choices[0].1.clone());
    let second = choices.get(1).map(|c| c.0);
    let gap = second.map(|s| best - s);
    diag.insert("best_score".into(), fv(best));
    diag.insert("second_best_score".into(), second.map_or(Value::Null, fv));
    diag.insert("ambiguity_gap".into(), gap.map_or(Value::Null, fv));
    diag.insert("transitions_checked".into(), json!(transitions));
    diag.insert("complete".into(), json!(path.len() == expected.len()));
    let mut proposed: Vec<Map<String, Value>> = path
        .iter()
        .enumerate()
        .map(|(k, &i)| {
            let mut m = candidates[i].json();
            m.insert("label".into(), json!(labels[k]));
            m.insert("center_bp".into(), centers[k].clone());
            m.insert("offset_bp".into(), fv(expected[k] - candidates[i].size_bp));
            m
        })
        .collect();
    let proposed_json = |p: &Vec<Map<String, Value>>| Value::Array(p.iter().map(|m| Value::Object(m.clone())).collect());
    diag.insert("proposed_matches".into(), proposed_json(&proposed));
    if best + SCORE_TOLERANCE < MIN_MATCH_SCORE {
        reject!("Full ladder assignment is below the ratio score threshold", diag);
    }
    if let Some(g) = gap {
        if g <= margin + SCORE_TOLERANCE {
            reject!("Ambiguous full ladder assignments; offsets withheld by independent review policy", diag);
        }
    }
    let hs: Vec<f64> = proposed.iter().map(|p| p["height"].as_f64().unwrap()).collect();
    let threshold = pystats::mean(&hs)? - pystats::stdev(&hs)?;
    let penalty_count = candidates.iter().enumerate().filter(|(i, p)| !path.contains(i) && p.height > threshold).count();
    let x = 1.0 - 0.25 * penalty_count as f64;
    let confidence = best * if x > 0.0 { x } else { 0.0 };
    let mut offsets = Map::new();
    for p in &proposed {
        offsets.insert(p["label"].as_str().unwrap().to_string(), p["offset_bp"].clone());
    }
    let clear_ranges = marker["parameters"]["clear_ranges"].as_array().unwrap();
    for row in proposed.iter_mut() {
        let size = row["size_bp"].as_f64().unwrap();
        let cleared = clear_ranges.iter().any(|s| s["min_bp"].as_f64().unwrap() <= size && size <= s["max_bp"].as_f64().unwrap());
        row.insert("cleared".into(), json!(cleared));
        row.insert("confidence".into(), fv(confidence));
    }
    diag.insert("proposed_matches".into(), proposed_json(&proposed));
    result.insert("status".into(), json!("accepted"));
    result.insert("matched".into(), proposed_json(&proposed));
    result.insert("bin_offsets".into(), Value::Object(offsets));
    result.insert("min_offset_bp".into(), proposed[0]["offset_bp"].clone());
    result.insert("max_offset_bp".into(), proposed.last().unwrap()["offset_bp"].clone());
    result.insert("confidence".into(), fv(confidence));
    diag.insert("unmatched_height_threshold".into(), fv(threshold));
    diag.insert("penalty_unmatched_count".into(), json!(penalty_count));
    diag.insert("confidence_is_probability".into(), json!(false));
    diag.insert("offset_rounding".into(), json!("unrounded independent float; no vendor decimal-rounding parity"));
    diag.insert("clear_range_policy".into(), json!("matched rows marked cleared; offsets retained"));
    result.insert("diagnostics".into(), Value::Object(diag));
    Ok(result)
}

pub fn match_ladder(
    peaks_by_channel: &Value,
    kit: &Value,
    size_standard_max_bp: &Value,
    sizing_accepted: &Value,
    ambiguity_margin: &Value,
    run_offset_state: Option<&Value>,
) -> Res<Value> {
    let mut result = Map::new();
    result.insert("status".into(), json!("rejected"));
    result.insert("reasons".into(), json!([]));
    result.insert("markers".into(), json!({}));
    let mut diag = Map::new();
    diag.insert("external_validation".into(), json!(false));
    diag.insert("method".into(), json!("independent_single_ladder_ratio"));
    diag.insert("source_contract".into(), json!("IDX16_ALLELE_CALLING AL-01/AL-05/AL-07"));
    diag.insert("run_offset_aggregation".into(), json!(false));
    diag.insert("original_pqv_applied".into(), json!(false));
    result.insert("diagnostics".into(), Value::Object(diag));
    let early = |mut result: Map<String, Value>, reason: &str| {
        result.insert("reasons".into(), json!([reason]));
        Ok(Value::Object(result))
    };
    if sizing_accepted != &json!(true) {
        return early(result, "Explicit accepted sizing is required before ladder offsets");
    }
    if !finite(Some(size_standard_max_bp), 1e-300, 1_000_000.0) {
        return early(result, "Explicit finite positive size_standard_max_bp <= 1000000 required");
    }
    if !finite(Some(ambiguity_margin), 0.0, 1.0) {
        return early(result, "ambiguity_margin must be finite and in [0, 1]");
    }
    let prepared = (|| -> Res<(Value, Vec<(String, Vec<Row>)>, Option<Value>)> {
        let definition = load_kit_definition(kit)?;
        let chans = channels(peaks_by_channel)?;
        let state = match run_offset_state {
            Some(s) if !s.is_null() => Some(validate_run_offset_state(s)?),
            _ => None,
        };
        Ok((definition, chans, state))
    })();
    let (definition, chans, state) = match prepared {
        Ok(x) => x,
        Err(e) => return early(result, &e),
    };
    result.insert("kit_name".into(), definition["name"].clone());
    {
        let d = result.get_mut("diagnostics").unwrap().as_object_mut().unwrap();
        d.insert("kit_diagnostics".into(), definition["diagnostics"].clone());
        if state.is_some() {
            d.insert("run_offset_aggregation".into(), json!(true));
            d.insert("run_window_coupling".into(), json!(true));
        }
    }
    let channel_rows: HashMap<&str, &Vec<Row>> = chans.iter().map(|(c, r)| (c.as_str(), r)).collect();
    let dmarkers = definition["markers"].as_array().unwrap();
    let mut grouped: Vec<(String, Vec<&Value>)> = Vec::new();
    for m in dmarkers {
        let ch = m["channel"].as_str().unwrap();
        match grouped.iter_mut().find(|(c, _)| c == ch) {
            Some((_, v)) => v.push(m),
            None => grouped.push((ch.to_string(), vec![m])),
        }
    }
    let margin = ambiguity_margin.as_f64().unwrap();
    let std_max = size_standard_max_bp.as_f64().unwrap();
    let mut markers_out = Map::new();
    let empty: Vec<Row> = Vec::new();
    for (channel, mut markers) in grouped {
        markers.sort_by(|a, b| {
            let k = |m: &Value| (m["min_bp"].as_f64().unwrap(), m["max_bp"].as_f64().unwrap(), m["name"].as_str().unwrap().to_string());
            let (ka, kb) = (k(a), k(b));
            ka.0.partial_cmp(&kb.0).unwrap().then(ka.1.partial_cmp(&kb.1).unwrap()).then(ka.2.cmp(&kb.2))
        });
        let mut previous: Option<&Value> = None;
        let mut previous_offset = 0.0f64;
        let mut previous_local = Value::Null;
        for (index, marker) in markers.iter().enumerate() {
            let mut coupling = Value::Null;
            if let Some(st) = &state {
                coupling = match previous {
                    Some(p) => ladder_window_offset(st, &p["name"], &previous_local)?,
                    None => json!({"marker": Value::Null, "offset_bp": fv(0.0), "run_contributors": [], "local_offset_bp": Value::Null,
                                   "value_count": 0, "first_marker_uses_run_offsets": false}),
                };
                previous_offset = coupling["offset_bp"].as_f64().unwrap();
            }
            let low = match previous {
                None => marker["min_bp"].as_f64().unwrap() - 7.0,
                Some(p) => p["max_bp"].as_f64().unwrap() - previous_offset,
            };
            let high = if index + 1 < markers.len() {
                let a = marker["max_bp"].as_f64().unwrap() - previous_offset;
                let b = markers[index + 1]["min_bp"].as_f64().unwrap();
                if b > a { b } else { a }
            } else {
                std_max
            };
            let rows = channel_rows.get(channel.as_str()).copied().unwrap_or(&empty);
            let mut row = match_marker(rows, marker, low, high, margin)?;
            row.insert("channel".into(), json!(channel));
            {
                let d = row.get_mut("diagnostics").unwrap().as_object_mut().unwrap();
                d.insert("previous_virtual_max_offset_bp".into(), fv(previous_offset));
                d.insert("previous_offset_policy".into(), json!("accepted previous marker from this ladder only; otherwise zero"));
                if state.is_some() {
                    d.insert("previous_offset_policy".into(), json!("admitted run previous-marker virtual-max values plus fresh local value; empty zero"));
                    d.insert("previous_offset_sources".into(), coupling);
                }
            }
            let accepted = row["status"] == json!("accepted");
            previous_offset = if accepted { row["max_offset_bp"].as_f64().unwrap() } else { 0.0 };
            previous_local = if accepted { row["max_offset_bp"].clone() } else { Value::Null };
            markers_out.insert(marker["name"].as_str().unwrap().to_string(), Value::Object(row));
            previous = Some(marker);
        }
    }
    let accepted = markers_out.values().filter(|r| r["status"] == json!("accepted")).count();
    let total = dmarkers.len();
    if accepted == total {
        result.insert("status".into(), json!("accepted"));
    } else if accepted > 0 {
        result.insert("status".into(), json!("partial"));
    }
    result.insert("markers".into(), Value::Object(markers_out));
    if accepted != total {
        result.insert("reasons".into(), json!(["One or more marker ladders lack accepted complete offsets"]));
    }
    Ok(Value::Object(result))
}
