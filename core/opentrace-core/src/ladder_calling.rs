//! Port of `genemapper_open/ladder_calling.py`.
use crate::allele_calling::{prepare_peaks, MAX_MARKER_PEAKS};
use crate::kit_definition::load_kit_definition;
use crate::ladder_observation_binding::bind;
use crate::pullup_quality::py_eq;
use crate::quality_control::number;
use serde_json::{json, Map, Value};
use std::collections::{BTreeSet, HashMap, HashSet};

pub type Res<T> = Result<T, String>;

fn fv(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

fn pidx(p: &Value) -> usize {
    p["peak_index"].as_u64().unwrap() as usize
}

struct Built {
    window: [f64; 2],
    calls: Vec<Value>,
    reviews: Vec<Value>,
    events: Vec<Value>,
    confidence: Value,
    cutoff: Value,
    linked: Vec<usize>,
}

#[allow(clippy::too_many_arguments)]
fn build_marker(
    marker: &Value,
    source: &Map<String, Value>,
    matching: &Map<String, Value>,
    prepared: &[Value],
    by_index: &HashMap<usize, &Value>,
    local: &[usize],
    peaks: &Value,
) -> Res<Built> {
    let channel = marker["channel"].as_str().unwrap();
    if source.get("channel") != Some(&marker["channel"]) {
        return Err("Matching marker channel differs from kit".into());
    }
    if !matches!(matching.get("status"), Some(Value::String(s)) if s == "accepted" || s == "partial") {
        return Err("Overall ladder matching not accepted or partial".into());
    }
    let window = match source.get("window") {
        Some(Value::Object(w)) if w.get("inclusive") == Some(&json!(true)) => w,
        _ => return Err("Explicit inclusive ladder matching window required".into()),
    };
    let null = Value::Null;
    let low = number(window.get("min_bp").unwrap_or(&null), "ladder window min", -1e6, 1e6)?;
    let high = number(window.get("max_bp").unwrap_or(&null), "ladder window max", 0.0, 1e6)?;
    if low > high {
        return Err("Reversed ladder matching window".into());
    }
    let size = |p: &Value| p["size_bp"].as_f64().unwrap();
    let linked: Vec<&Value> = prepared.iter().filter(|p| p["channel"] == json!(channel) && low <= size(p) && size(p) <= high).collect();
    if linked.len() > MAX_MARKER_PEAKS {
        return Err("Ladder window exceeds512 linked peaks; no truncation applied".into());
    }
    let labels = marker["ladder_labels"].as_array().unwrap();
    let rows = match source.get("matched") {
        Some(Value::Array(r)) if r.len() == labels.len() => r,
        _ => return Err("Complete matched expected ladder rows required".into()),
    };
    let bins: HashMap<&str, &Value> = marker["bins"].as_array().unwrap().iter().map(|b| (b["label"].as_str().unwrap(), b)).collect();
    let marker_confidence = number(source.get("confidence").unwrap_or(&null), "marker ladder confidence", 0.0, 1e12)?;
    let mut selected: HashMap<usize, (&Map<String, Value>, f64, bool)> = HashMap::new();
    for (index, m) in rows.iter().enumerate() {
        let mm = m.as_object().ok_or("Matched ladder row required")?;
        let ordinal = match mm.get("candidate_index") {
            Some(Value::Number(n)) => n.as_i64(),
            _ => None,
        };
        let ordinal = match ordinal {
            Some(o) if o >= 0 && (o as usize) < local.len() => o as usize,
            _ => return Err("Ladder candidate input ordinal missing or invalid".into()),
        };
        let peak_index = local[ordinal];
        let peak = by_index[&peak_index];
        if selected.contains_key(&peak_index) {
            return Err("A ladder peak cannot satisfy two expected labels in one marker".into());
        }
        let label = mm.get("label").unwrap_or(&null);
        let bin = match label.as_str() {
            Some(l) if label == &labels[index] => bins.get(l),
            _ => None,
        };
        let bin = bin.ok_or("Matched label differs from expected ladder order")?;
        if !(low <= size(peak) && size(peak) <= high) {
            return Err("Matched source lies outside ladder window".into());
        }
        if peak["flags"].as_array().unwrap().iter().any(|f| f == "spike" || f == "pullup") {
            return Err("Matched ladder source has an excluded artifact flag".into());
        }
        for key in ["scan", "size_bp", "height"] {
            if !py_eq(mm.get(key).unwrap_or(&null), &peak[key]) {
                return Err("Matched source observations disagree with indexed peak".into());
            }
        }
        if !py_eq(mm.get("center_bp").unwrap_or(&null), &bin["center_bp"]) {
            return Err("Matched bin center differs from definition".into());
        }
        let confidence = number(mm.get("confidence").unwrap_or(&null), "ladder initial confidence", 0.0, 1e12)?;
        if confidence != marker_confidence {
            return Err("Matched confidence differs from marker matching result".into());
        }
        let cleared_given = match mm.get("cleared") {
            Some(Value::Bool(b)) => *b,
            _ => return Err("Explicit post-match clear-range state required".into()),
        };
        let cleared = marker["parameters"]["clear_ranges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["min_bp"].as_f64().unwrap() <= size(peak) && size(peak) <= r["max_bp"].as_f64().unwrap());
        if cleared != cleared_given {
            return Err("Matched clear-range state differs from definition".into());
        }
        selected.insert(peak_index, (mm, confidence, cleared));
    }
    let diagnostics = match source.get("diagnostics") {
        Some(d) if !d.is_null() && d != &json!({}) && d != &json!(false) && d != &json!(0) => d.clone(),
        _ => json!({}),
    };
    let dm = diagnostics.as_object().ok_or("AttributeError: diagnostics has no get")?;
    let cutoff = dm.get("cutoff_height").cloned().unwrap_or(Value::Null);
    let cap = dm.get("top_2n_height").cloned().unwrap_or(Value::Null);
    let below = |h: f64, limit: &Value| -> Res<bool> {
        match limit {
            Value::Number(n) => Ok(h < n.as_f64().unwrap()),
            _ => Err("TypeError: unsupported cutoff type".into()),
        }
    };
    let (mut reviews, mut calls, mut events) = (Vec::new(), Vec::new(), Vec::new());
    for peak in &linked {
        let i = pidx(peak);
        let mut review = peak.as_object().unwrap().clone();
        review.insert("state".into(), json!("unmatched_ladder_candidate"));
        review.insert("reasons".into(), json!([]));
        let flags = peak["flags"].as_array().unwrap();
        let artifact: Vec<Value> = ["spike", "pullup"].iter().filter(|f| flags.iter().any(|x| x == **f)).map(|f| json!(f)).collect();
        let height = peak["height"].as_f64().unwrap();
        if !artifact.is_empty() {
            review.insert("state".into(), json!("excluded_artifact"));
            review.insert("reasons".into(), Value::Array(artifact));
        } else if let Some((m, confidence, cleared)) = selected.get(&i) {
            if *cleared {
                review.insert("state".into(), json!("clear_range"));
                review.insert("reasons".into(), json!(["clear_range"]));
            } else {
                review.insert("state".into(), json!("selected"));
                let mut call = peak.as_object().unwrap().clone();
                call.insert("label".into(), m["label"].clone());
                call.insert("bin_hits".into(), json!([m["label"].clone()]));
                call.insert("off_ladder".into(), json!(false));
                call.insert("kind".into(), json!("ladder_matched"));
                call.insert("confidence".into(), fv(*confidence));
                call.insert("confidence_source".into(), json!("ladder_ratio_and_unmatched_height_penalty"));
                call.insert("expected_bin_center_bp".into(), m["center_bp"].clone());
                call.insert("source_peak_id".into(), peaks[i].get("id").cloned().unwrap_or(Value::Null));
                call.insert("review_required".into(), json!(true));
                calls.push(Value::Object(call));
            }
        } else if !cutoff.is_null() && below(height, &cutoff)? {
            review.insert("state".into(), json!("below_ladder_cutoff"));
            review.insert("reasons".into(), json!(["below_ladder_cutoff"]));
        } else if !cap.is_null() && below(height, &cap)? {
            review.insert("state".into(), json!("below_top_2n_cutoff"));
            review.insert("reasons".into(), json!(["below_top_2n_cutoff"]));
        } else {
            review.insert("reasons".into(), json!(["unmatched_ladder_candidate"]));
        }
        if review["state"] != json!("selected") {
            events.push(json!({"peak_index": i, "reason": review["state"].clone()}));
        }
        reviews.push(Value::Object(review));
    }
    Ok(Built {
        window: [low, high],
        calls,
        reviews,
        events,
        confidence: source.get("confidence").cloned().unwrap_or(Value::Null),
        cutoff,
        linked: linked.iter().map(|p| pidx(p)).collect(),
    })
}

pub fn ladder_call_observations(peaks: &Value, kit: &Value, matching: &Value) -> Res<Value> {
    let definition = load_kit_definition(kit)?;
    let prepared = prepare_peaks(peaks)?;
    let by_index: HashMap<usize, &Value> = prepared.iter().map(|p| (pidx(p), p)).collect();
    let mut channel_indices: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, p) in peaks.as_array().unwrap().iter().enumerate() {
        let ch = p["channel"].as_str().unwrap_or("").to_string();
        channel_indices.entry(ch).or_default().push(index);
    }
    let mm = match matching {
        Value::Object(m) if matches!(m.get("markers"), Some(Value::Object(_))) => m,
        _ => return Err("Explicit marker ladder matching result required".into()),
    };
    let src_markers = mm["markers"].as_object().unwrap();
    let mut assigned: HashSet<usize> = HashSet::new();
    let mut rows: Vec<Map<String, Value>> = Vec::new();
    for marker in definition["markers"].as_array().unwrap() {
        let name = marker["name"].as_str().unwrap();
        let channel = marker["channel"].as_str().unwrap();
        let mut row = Map::new();
        row.insert("name".into(), json!(name));
        row.insert("channel".into(), json!(channel));
        row.insert("status".into(), json!("rejected"));
        row.insert("reasons".into(), json!([]));
        row.insert("calls".into(), json!([]));
        row.insert("stutter_calls".into(), json!([]));
        row.insert("peak_review".into(), json!([]));
        row.insert("events".into(), json!([]));
        let source = match src_markers.get(name) {
            Some(Value::Object(s)) if s.get("status") == Some(&json!("accepted")) => s,
            _ => {
                row.insert("reasons".into(), json!(["Ladder marker matching not accepted"]));
                rows.push(row);
                continue;
            }
        };
        let local = channel_indices.get(channel).cloned().unwrap_or_default();
        match build_marker(marker, source, mm, &prepared, &by_index, &local, peaks) {
            Ok(b) => {
                row.insert("status".into(), json!("tentative"));
                row.insert("window_bp".into(), json!([fv(b.window[0]), fv(b.window[1])]));
                row.insert("calls".into(), Value::Array(b.calls));
                row.insert("peak_review".into(), Value::Array(b.reviews));
                row.insert("events".into(), Value::Array(b.events));
                row.insert("matching_confidence".into(), b.confidence);
                row.insert("cutoff_height".into(), b.cutoff);
                assigned.extend(b.linked);
            }
            Err(e) => {
                row.insert("reasons".into(), json!([e]));
            }
        }
        rows.push(row);
    }
    // Cross-marker overlap: calls keyed by source peak index, in insertion order.
    let mut order: Vec<usize> = Vec::new();
    let mut by_peak: HashMap<usize, Vec<(usize, usize)>> = HashMap::new();
    for (mi, row) in rows.iter().enumerate() {
        for (ci, call) in row["calls"].as_array().unwrap().iter().enumerate() {
            let pi = pidx(call);
            if !by_peak.contains_key(&pi) {
                order.push(pi);
            }
            by_peak.entry(pi).or_default().push((mi, ci));
        }
    }
    for pi in order {
        let links = &by_peak[&pi];
        if links.len() > 1 {
            let names: Vec<Value> = links.iter().map(|(mi, _)| rows[*mi]["name"].clone()).collect();
            for (mi, ci) in links {
                let call = rows[*mi].get_mut("calls").unwrap().as_array_mut().unwrap()[*ci].as_object_mut().unwrap();
                let flags = call.get_mut("flags").unwrap().as_array_mut().unwrap();
                if !flags.iter().any(|f| f == "cross_marker_overlap") {
                    flags.push(json!("cross_marker_overlap"));
                }
                rows[*mi]
                    .get_mut("events")
                    .unwrap()
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"peak_index": pi, "reason": "cross_marker_overlap", "markers": names.clone()}));
            }
        }
    }
    let rejected: Vec<String> = rows.iter().filter(|r| r["status"] == json!("rejected")).map(|r| r["name"].as_str().unwrap().to_string()).collect();
    let mut reasons: Vec<Value> = Vec::new();
    if !rejected.is_empty() {
        reasons.push(json!(format!("Rejected ladder observation markers: {}", rejected.join(", "))));
    }
    let mut status = "rejected";
    if !rows.is_empty() && rejected.is_empty() {
        status = "tentative";
    } else if rejected.len() < rows.len() {
        status = "partial";
    }
    let all: BTreeSet<usize> = by_index.keys().copied().collect();
    let unassigned: Vec<Value> = all.iter().filter(|i| !assigned.contains(i)).map(|i| json!(i)).collect();
    let result = json!({
        "status": status,
        "reasons": reasons,
        "markers": Value::Array(rows.into_iter().map(Value::Object).collect()),
        "unassigned_peak_indices": unassigned,
        "research_only": true,
        "external_validation": false,
        "original_runtime_validated": false,
        "diagnostics": {
            "method": "ladder_matching_calls",
            "source_identity": "channel_local_input_ordinal_to_flat_input_index",
            "sample_filters_applied": false,
            "matching_repeated": false,
        },
    });
    // Skeleton key order in Python: status, reasons, markers, unassigned_peak_indices, research_only, ...
    bind(peaks, &definition, matching, &result)
}
