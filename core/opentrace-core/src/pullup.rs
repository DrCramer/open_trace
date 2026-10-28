//! Port of `genemapper_open/pullup.py`: explicit pairwise pull-up reconstruction (parsed-mapping input).
//! Not ported: the JSON text/bytes path of `load_pullup_definition` (duplicate-key and nonfinite checks on text).
use crate::kit_definition::py_json_dumps_ascii;
use crate::quality_control::{boolean, number};
use serde_json::{json, Map, Value};
use std::collections::HashSet;

pub type Res<T> = Result<T, String>;

fn fv(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

/// Python `a - b` for JSON numbers: int when both are ints, otherwise binary64.
fn py_sub(a: &Value, b: &Value) -> Res<Value> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) if x.is_i64() && y.is_i64() => x
            .as_i64()
            .unwrap()
            .checked_sub(y.as_i64().unwrap())
            .map(|v| json!(v))
            .ok_or_else(|| "integer overflow".to_string()),
        _ => Ok(fv(num(a).unwrap_or(f64::NAN) - num(b).unwrap_or(f64::NAN))),
    }
}

fn py_abs(v: Value) -> Value {
    match &v {
        Value::Number(n) if n.is_i64() => json!(n.as_i64().unwrap().abs()),
        _ => fv(num(&v).unwrap_or(f64::NAN).abs()),
    }
}

/// Python `_digits_ok`-style check for `DATA[0-9]{1,6}`.
fn data_tag(s: &str) -> bool {
    match s.strip_prefix("DATA") {
        Some(rest) => (1..=6).contains(&rest.len()) && rest.bytes().all(|c| c.is_ascii_digit()),
        None => false,
    }
}

/// Python `load_pullup_definition` for a parsed mapping. Returns the validated definition unchanged in content.
pub fn load_pullup_definition(source: &Value) -> Res<Value> {
    if py_json_dumps_ascii(source).len() > 16384 {
        return Err("Pull-up definition exceeds16KiB".into());
    }
    let map = match source {
        Value::Object(m) => m,
        _ => return Err("Explicit pull-up detector/profile fields required".into()),
    };
    let mut keys: Vec<&str> = map.keys().map(|k| k.as_str()).collect();
    keys.sort();
    if keys != ["detector", "profile_threshold", "schema_version"] {
        return Err("Explicit pull-up detector/profile fields required".into());
    }
    match map.get("schema_version") {
        Some(Value::Number(n)) if n.is_i64() && n.as_i64() == Some(1) => {}
        _ => return Err("Pull-up schema_version must be1".into()),
    }
    number(&map["profile_threshold"], "profile_threshold", 0.0, 1.0)?;
    let detector = &map["detector"];
    if !detector.is_null() {
        let dmap = detector.as_object().ok_or("All explicit detector fields required")?;
        let mut dkeys: Vec<&str> = dmap.keys().map(|k| k.as_str()).collect();
        dkeys.sort();
        let mut expected_keys = vec![
            "algorithm", "calling_policy", "phase", "reference_order", "max_scan_distance",
            "height_ratio_threshold", "channel_pairs",
        ];
        expected_keys.sort();
        if dkeys != expected_keys {
            return Err("All explicit detector fields required".into());
        }
        let expected = [
            ("algorithm", "static_ar_pairwise_body"),
            ("calling_policy", "explicit_ordered_channel_pairs"),
            ("phase", "after_spike_restoration_before_calling"),
            ("reference_order", "stable_scan_midpoint_binary_search"),
        ];
        if expected.iter().any(|(k, v)| dmap[*k] != json!(v)) {
            return Err("Unsupported explicit pull-up policy".into());
        }
        match dmap.get("max_scan_distance") {
            Some(Value::Number(n)) if n.is_i64() && (0..=1_000_000).contains(&n.as_i64().unwrap()) => {}
            _ => return Err("max_scan_distance must be an integer in[0,1000000]".into()),
        }
        number(&dmap["height_ratio_threshold"], "height_ratio_threshold", 0.0, 1.0)?;
        let routes = match dmap.get("channel_pairs") {
            Some(Value::Array(r)) if (1..=32).contains(&r.len()) => r,
            _ => return Err("Explicit1..32 ordered channel pairs required".into()),
        };
        for route in routes {
            let rmap = match route {
                Value::Object(m) if m.len() == 2 && m.contains_key("candidate_channel") && m.contains_key("reference_channel") => m,
                _ => return Err("Candidate/reference channel pair required".into()),
            };
            let tags_ok = rmap.values().all(|t| matches!(t, Value::String(s) if data_tag(s)));
            if !tags_ok {
                return Err("Explicit DATA channel tags required".into());
            }
            if rmap["candidate_channel"] == rmap["reference_channel"] {
                return Err("Cross-channel route requires different dyes".into());
            }
        }
    }
    Ok(source.clone())
}

/// Python `_flag`: explicit boolean field, OR-ed with the flag list or map.
pub fn flag(peak: &Map<String, Value>, name: &str) -> Res<bool> {
    let value = peak.get(name).cloned().unwrap_or(json!(false));
    let value = boolean(&value, name)?;
    let flags = peak.get("flags").cloned().unwrap_or(json!([]));
    match &flags {
        Value::Object(m) => {
            if m.values().any(|v| !matches!(v, Value::Bool(_))) {
                return Err("Boolean flag map required".into());
            }
            Ok(value || m.get(name) == Some(&json!(true)))
        }
        Value::Array(list) => {
            if list.iter().any(|v| !matches!(v, Value::String(_))) {
                return Err("String flag list required".into());
            }
            Ok(value || list.iter().any(|v| v.as_str() == Some(name)))
        }
        _ => Err("String flag list required".into()),
    }
}

/// Python `_state`: stored pull-up state of one peak.
fn state_of(peak: &Map<String, Value>) -> Res<Map<String, Value>> {
    let label = peak.get("artifact_label").cloned().unwrap_or(Value::Null);
    if !label.is_null() {
        match &label {
            Value::String(s) if s.chars().count() <= 128 => {}
            _ => return Err("Bounded artifact label required".into()),
        }
    }
    let pullup = flag(peak, "pullup")?;
    let ratio = number(
        &peak.get("pullup_ratio").cloned().unwrap_or(json!(-1)),
        "stored pull-up ratio",
        -1e12,
        1e12,
    )?;
    let threshold = number(
        &peak.get("pullup_ratio_threshold").cloned().unwrap_or(json!(-1)),
        "stored pull-up threshold",
        -1.0,
        1e12,
    )?;
    let mut state = Map::new();
    state.insert("pullup".into(), json!(pullup));
    state.insert("ratio".into(), fv(ratio));
    state.insert("ratio_threshold".into(), fv(threshold));
    state.insert("artifact_label".into(), label);
    state.insert("peak_called_state".into(), Value::Null);
    state.insert("originally_artifact".into(), Value::Null);
    Ok(state)
}

/// Python `_full_set`: modeled full setter effects (flag, label, optional ratio and threshold).
fn full_set(state: &mut Map<String, Value>, value: bool, ratio: Option<f64>, threshold: Option<Value>) {
    state.insert("pullup".into(), json!(value));
    state.insert("peak_called_state".into(), json!(1));
    state.insert("originally_artifact".into(), json!(true));
    if !value {
        state.insert("artifact_label".into(), json!(""));
    } else {
        let label = state.get("artifact_label").cloned().unwrap_or(Value::Null);
        let plain = match &label {
            Value::Null => true,
            Value::String(s) => s.chars().all(|c| (c as u32) <= 32),
            _ => false,
        };
        if plain {
            state.insert("artifact_label".into(), json!("SPU"));
        }
    }
    if let Some(r) = ratio {
        state.insert("ratio".into(), fv(r));
        state.insert("ratio_threshold".into(), threshold.unwrap_or(Value::Null));
    }
}

/// Python `_binary_search`: midpoint search over reference scans; returns (insertion position, exact match).
fn binary_search(references: &[Value], scan: f64) -> (usize, bool) {
    let mut low: i64 = 0;
    let mut high: i64 = references.len() as i64 - 1;
    while low <= high {
        let middle = (low + high) / 2;
        let value = num(&references[middle as usize]["scan"]).unwrap_or(f64::NAN);
        if value < scan {
            low = middle + 1;
        } else if value > scan {
            high = middle - 1;
        } else {
            return (middle as usize, true);
        }
    }
    (low as usize, false)
}

struct Settings {
    max_scan_distance: Value,
    height_ratio_threshold: Value,
}

/// Python `_attempt`: one candidate against one reference; updates the candidate state in place.
fn attempt(candidate: &Value, reference: &Value, state: &mut Map<String, Value>, settings: &Settings) -> Res<Value> {
    let before = Value::Object(state.clone());
    full_set(state, false, None, None);
    let height = num(&reference["height"]).unwrap_or(f64::NAN);
    let cand_height = num(&candidate["height"]).unwrap_or(f64::NAN);
    // Python divides before the positive-reference guard; a zero reference leaves the ratio unused.
    let ratio = if height != 0.0 { Some(cand_height / height) } else { None };
    let finite = ratio.map_or(false, |r| r.is_finite());
    if height != 0.0 && !finite {
        return Err("Nonfinite pull-up division is outside the finite adapter".into());
    }
    let distance = py_abs(py_sub(&reference["scan"], &candidate["scan"])?);
    let dist = num(&distance).unwrap_or(f64::NAN);
    let max_distance = num(&settings.max_scan_distance).unwrap_or(f64::NAN);
    let thr = num(&settings.height_ratio_threshold).unwrap_or(f64::NAN);
    let matched = height > 0.0
        && dist <= max_distance
        && finite
        && ratio.map_or(false, |r| r < thr);
    if matched {
        full_set(state, true, ratio, Some(settings.height_ratio_threshold.clone()));
    }
    let reason = if height <= 0.0 {
        "reference_height_not_positive"
    } else if dist > max_distance {
        "distance_above_threshold"
    } else if !matched {
        "ratio_not_below_threshold"
    } else {
        "matched"
    };
    Ok(json!({
        "reference_id": reference["_pullup_id"],
        "distance_scan": distance,
        "attempt_ratio": if finite { fv(ratio.unwrap()) } else { Value::Null },
        "ratio_available": finite,
        "matched": matched,
        "reason": reason,
        "before": before,
        "after": Value::Object(state.clone()),
    }))
}

fn sum_scans_distinct(references: &[Value]) -> usize {
    let mut seen = HashSet::new();
    for r in references {
        let x = num(&r["scan"]).unwrap_or(f64::NAN);
        seen.insert((x + 0.0).to_bits());
    }
    seen.len()
}

/// Python `apply_pullup_detection`: copies selected peak lists, evaluates the explicit ordered routes.
pub fn apply_pullup_detection(
    channels: &Value,
    definition: &Value,
    selected_group: &Value,
    reference_channel: &Value,
) -> Res<Value> {
    let definition = load_pullup_definition(definition)?;
    let group = match selected_group.as_str() {
        Some(g @ ("raw" | "analyzed")) => g.to_string(),
        _ => return Err("Selected raw/analyzed group required".into()),
    };
    let reference = match reference_channel.as_str() {
        Some(s) => s.to_string(),
        None => return Err("Explicit size-standard channel required".into()),
    };
    let items = match channels {
        Value::Array(a) if a.len() <= 128 => a,
        _ => return Err("Bounded channel list required".into()),
    };
    let settings_v = definition["detector"].clone();
    if settings_v.is_null() {
        return Ok(json!({
            "peaks_by_channel": {},
            "summary": {"status": "not_requested", "scope": "existing_flags_only",
                        "original_automatic_route_confirmed": false},
        }));
    }
    let settings = Settings {
        max_scan_distance: settings_v["max_scan_distance"].clone(),
        height_ratio_threshold: settings_v["height_ratio_threshold"].clone(),
    };
    let routes = settings_v["channel_pairs"].as_array().cloned().unwrap_or_default();

    let mut chosen: Vec<(String, Vec<Value>)> = Vec::new();
    let mut all_tags: Vec<String> = Vec::new();
    let mut total = 0usize;
    for channel in items {
        let cmap = channel.as_object().ok_or("Channel tag required")?;
        let tag = match cmap.get("tag") {
            Some(Value::String(t)) => t.clone(),
            _ => return Err("Channel tag required".into()),
        };
        if all_tags.contains(&tag) {
            return Err("Duplicate DATA channel".into());
        }
        all_tags.push(tag.clone());
        if cmap.get("group") != Some(&json!(group)) {
            continue;
        }
        let rows = match cmap.get("peaks") {
            Some(Value::Array(r)) if r.len() <= 4096 => r,
            _ => return Err("At most4096 measured peaks per channel".into()),
        };
        total += rows.len();
        if total > 32768 {
            return Err("At most32768 selected-group peaks required".into());
        }
        let mut prepared = Vec::with_capacity(rows.len());
        for (index, peak) in rows.iter().enumerate() {
            let pmap = peak.as_object().ok_or("Measured peak mapping required")?;
            if pmap.contains_key("pullup_classification") {
                return Err("Fresh FSA analysis required for pull-up detection".into());
            }
            number(&pmap.get("scan").cloned().unwrap_or(Value::Null), "scan", 0.0, 1e8)?;
            number(&pmap.get("height").cloned().unwrap_or(Value::Null), "height", -1e12, 1e12)?;
            let offscaled = pmap.get("offscaled").cloned().unwrap_or(json!(false));
            boolean(&offscaled, "offscaled")?;
            flag(pmap, "spike")?;
            state_of(pmap)?;
            let mut copy = pmap.clone();
            copy.insert("_pullup_id".into(), json!(format!("{}:{}", tag, index)));
            prepared.push(Value::Object(copy));
        }
        chosen.push((tag, prepared));
    }
    let chosen_tags: Vec<&str> = chosen.iter().map(|(t, _)| t.as_str()).collect();
    for route in &routes {
        let a = route["candidate_channel"].as_str().unwrap_or("");
        let b = route["reference_channel"].as_str().unwrap_or("");
        if a == reference {
            return Err("Size-standard channel cannot be a pull-up candidate".into());
        }
        if !chosen_tags.contains(&a) || !chosen_tags.contains(&b) {
            return Err("Pull-up routes must use channels in the selected DATA group".into());
        }
    }

    // Per-peak state and history, in the same order as Python's `states` dict.
    let mut ids: Vec<String> = Vec::new();
    let mut states: Vec<Map<String, Value>> = Vec::new();
    let mut histories: Vec<Map<String, Value>> = Vec::new();
    for (_, peaks) in &chosen {
        for p in peaks {
            let pmap = p.as_object().unwrap();
            let id = pmap["_pullup_id"].as_str().unwrap().to_string();
            let state = state_of(pmap)?;
            let mut h = Map::new();
            h.insert("status".into(), json!("not_targeted"));
            h.insert("initial_state".into(), Value::Object(state.clone()));
            h.insert("events".into(), json!([]));
            h.insert("skips".into(), json!([]));
            h.insert("matched_reference_id".into(), Value::Null);
            h.insert("original_automatic_route_confirmed".into(), json!(false));
            ids.push(id);
            states.push(state);
            histories.push(h);
        }
    }
    let index_of = |id: &str| ids.iter().position(|x| x == id).unwrap();
    let peaks_of = |tag: &str| -> Vec<Value> {
        chosen.iter().find(|(t, _)| t == tag).map(|(_, p)| p.clone()).unwrap_or_default()
    };

    let mut passes: Vec<Value> = Vec::new();
    let mut attempt_count: u64 = 0;
    for (order, route) in routes.iter().enumerate() {
        let candidate_tag = route["candidate_channel"].as_str().unwrap_or("").to_string();
        let reference_tag = route["reference_channel"].as_str().unwrap_or("").to_string();
        let candidates = peaks_of(&candidate_tag);
        let mut references = peaks_of(&reference_tag);
        references.sort_by(|a, b| {
            num(&a["scan"]).unwrap_or(f64::NAN).partial_cmp(&num(&b["scan"]).unwrap_or(f64::NAN)).unwrap()
        });
        let (mut attempted, mut skipped, mut matched) = (0u64, 0u64, 0u64);
        for candidate in &candidates {
            let cmap = candidate.as_object().unwrap();
            let id = cmap["_pullup_id"].as_str().unwrap().to_string();
            let k = index_of(&id);
            histories[k].insert("status".into(), json!("evaluated_explicit_passes"));
            if references.is_empty() {
                histories[k]["skips"].as_array_mut().unwrap().push(json!({"pass_index": order, "reason": "reference_list_empty"}));
                continue;
            }
            let offscaled = cmap.get("offscaled") == Some(&json!(true));
            let reason = if offscaled {
                Some("candidate_offscale")
            } else if states[k]["pullup"] == json!(true) {
                Some("candidate_already_pullup")
            } else if flag(cmap, "spike")? {
                Some("candidate_current_spike")
            } else {
                None
            };
            if let Some(reason) = reason {
                skipped += 1;
                histories[k]["skips"].as_array_mut().unwrap().push(json!({"pass_index": order, "reason": reason}));
                continue;
            }
            let scan = num(&cmap["scan"]).unwrap_or(f64::NAN);
            let (position, exact) = binary_search(&references, scan);
            let n = references.len();
            let indices: Vec<usize> = if exact {
                vec![position]
            } else if position == 0 {
                vec![0]
            } else if position == n {
                vec![n - 1]
            } else {
                vec![position, position - 1]
            };
            for index in indices {
                let mut event = attempt(candidate, &references[index], &mut states[k], &settings)?;
                let emap = event.as_object_mut().unwrap();
                emap.insert("pass_index".into(), json!(order));
                emap.insert("binary_search_position".into(), json!(position));
                emap.insert("exact_scan_match".into(), json!(exact));
                emap.insert("reference_sorted_index".into(), json!(index));
                histories[k]["events"].as_array_mut().unwrap().push(event);
                attempted += 1;
                attempt_count += 1;
                if attempt_count > 1_000_000 {
                    return Err("Pull-up comparison work budget exceeded".into());
                }
                if states[k]["pullup"] == json!(true) {
                    histories[k].insert(
                        "matched_reference_id".into(),
                        references[index]["_pullup_id"].clone(),
                    );
                    matched += 1;
                    break;
                }
            }
        }
        let mut pass = Map::new();
        pass.insert("pass_index".into(), json!(order));
        if let Value::Object(r) = route {
            for (k, v) in r {
                pass.insert(k.clone(), v.clone());
            }
        }
        pass.insert("candidate_count".into(), json!(candidates.len()));
        pass.insert("reference_count".into(), json!(references.len()));
        pass.insert("attempted_pairs".into(), json!(attempted));
        pass.insert("skipped_candidates".into(), json!(skipped));
        pass.insert("new_pullups".into(), json!(matched));
        pass.insert(
            "duplicate_reference_scans".into(),
            json!(references.len() - sum_scans_distinct(&references)),
        );
        passes.push(Value::Object(pass));
    }

    let mut output = Map::new();
    for (tag, peaks) in &chosen {
        if *tag == reference {
            continue;
        }
        let mut out_peaks = Vec::with_capacity(peaks.len());
        for peak in peaks {
            let pmap = peak.as_object().unwrap();
            let id = pmap["_pullup_id"].as_str().unwrap().to_string();
            let k = index_of(&id);
            let mut rebuilt = Map::new();
            for (key, v) in pmap {
                if key != "_pullup_id" {
                    rebuilt.insert(key.clone(), v.clone());
                }
            }
            let ratio_provenance = if histories[k]["matched_reference_id"].is_null() {
                "supplied_or_fresh_state"
            } else {
                "measured_pair"
            };
            histories[k].insert("state".into(), Value::Object(states[k].clone()));
            histories[k].insert("scope".into(), json!("explicit_cross_channel_body_reconstruction"));
            histories[k].insert("ratio_provenance".into(), json!(ratio_provenance));
            rebuilt.insert("pullup".into(), states[k]["pullup"].clone());
            rebuilt.insert("pullup_classification".into(), Value::Object(histories[k].clone()));
            out_peaks.push(Value::Object(rebuilt));
        }
        output.insert(tag.clone(), Value::Array(out_peaks));
    }
    let prefix = format!("{}:", reference);
    let current: u64 = ids
        .iter()
        .zip(states.iter())
        .filter(|(id, _)| !id.starts_with(&prefix))
        .map(|(_, s)| (s["pullup"] == json!(true)) as u64)
        .sum();
    Ok(json!({
        "peaks_by_channel": Value::Object(output),
        "summary": {
            "status": "evaluated_explicit_passes",
            "scope": "experimental_static_body_with_own_ordered_routes",
            "original_automatic_route_confirmed": false,
            "phase": settings_v["phase"],
            "attempted_pairs": attempt_count,
            "passes": passes,
            "current_pullups": current,
        },
    }))
}
