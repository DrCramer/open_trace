//! Port of `genemapper_open/allele_calling.py`, built up one function at a time.
//! Done so far: `prepare_peaks` (Python `_prepare_peaks`, input validation and normalization).
use serde_json::{json, Map, Value};

pub type Res<T> = Result<T, String>;

pub const MAX_INPUT_PEAKS: usize = 4096;

/// Python `allele_calling._number`: a finite real that is not a bool. JSON numbers are always finite.
fn is_number(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Number(_)))
}

fn as_f64(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        _ => f64::NAN,
    }
}

fn fv(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

/// Python `re.fullmatch(r'DATA[1-9][0-9]{0,5}', channel)`.
fn canonical_channel(s: &str) -> bool {
    let rest = match s.strip_prefix("DATA") {
        Some(r) => r.as_bytes(),
        None => return false,
    };
    (1..=6).contains(&rest.len()) && (b'1'..=b'9').contains(&rest[0]) && rest[1..].iter().all(|c| c.is_ascii_digit())
}

/// Python `_prepare_peaks`: validates raw peak records and returns normalized records sorted by (size_bp, peak_index).
pub fn prepare_peaks(peaks: &Value) -> Res<Vec<Value>> {
    let limit_message = format!("peaks must be a list of at most {} records", MAX_INPUT_PEAKS);
    let items = match peaks {
        Value::Array(a) if a.len() <= MAX_INPUT_PEAKS => a,
        _ => return Err(limit_message),
    };
    let mut prepared: Vec<(f64, usize, Value)> = Vec::with_capacity(items.len());
    for (index, peak) in items.iter().enumerate() {
        let map = peak.as_object().ok_or_else(|| format!("peak {} must be an object", index))?;
        let channel = match map.get("channel") {
            Some(Value::String(s)) if canonical_channel(s) => s.clone(),
            _ => return Err(format!("peak {} needs a canonical DATA channel", index)),
        };
        for key in ["scan", "size_bp", "height"] {
            if !is_number(map.get(key)) {
                return Err(format!("peak {} needs finite {}", index, key));
            }
        }
        let scan = as_f64(&map["scan"]);
        let size_bp = as_f64(&map["size_bp"]);
        let height = as_f64(&map["height"]);
        if !(0.0 <= scan && scan <= 100_000_000.0 && 0.0 < size_bp && size_bp <= 1_000_000.0
            && 0.0 < height && height <= 1e12)
        {
            return Err(format!("peak {} needs bounded scan and positive bounded size/height", index));
        }
        let mut flags: Vec<String> = match map.get("flags") {
            None => Vec::new(),
            Some(Value::Object(flag_map)) => {
                if flag_map.values().any(|v| !matches!(v, Value::Bool(_))) {
                    return Err(format!("peak {} flag map must contain string/boolean pairs", index));
                }
                flag_map
                    .iter()
                    .filter(|(_, v)| **v == Value::Bool(true))
                    .map(|(name, _)| name.clone())
                    .collect()
            }
            Some(Value::Array(list)) => {
                let mut names = Vec::with_capacity(list.len());
                for f in list {
                    match f {
                        Value::String(s) => names.push(s.clone()),
                        _ => return Err(format!("peak {} flags must be strings or a boolean flag map", index)),
                    }
                }
                names
            }
            Some(_) => return Err(format!("peak {} flags must be strings or a boolean flag map", index)),
        };
        // Python `list(dict.fromkeys(flags))`: first occurrence wins.
        let mut unique: Vec<String> = Vec::with_capacity(flags.len());
        for f in flags.drain(..) {
            if !unique.contains(&f) {
                unique.push(f);
            }
        }
        let mut flags = unique;
        for flag in ["spike", "pullup"] {
            if map.contains_key(flag) && !matches!(map[flag], Value::Bool(_)) {
                return Err(format!("peak {} {} must be boolean", index, flag));
            }
            if map.get(flag) == Some(&Value::Bool(true)) && !flags.iter().any(|f| f == flag) {
                flags.push(flag.to_string());
            }
        }
        prepared.push((
            size_bp,
            index,
            json!({
                "peak_index": index,
                "channel": channel,
                "scan": fv(scan),
                "size_bp": fv(size_bp),
                "height": fv(height),
                "flags": flags,
            }),
        ));
    }
    prepared.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.cmp(&b.1)));
    Ok(prepared.into_iter().map(|(_, _, v)| v).collect())
}

// ---------------------------------------------------------------------------
// Per-marker offsets, bin labels and picking (Python _reference_offsets, _marker_offsets, _label, _pick_marker).
// Output values keep their JSON int/float type, matching Python's int/float results.
// ---------------------------------------------------------------------------

pub const MAX_MARKER_PEAKS: usize = 512;
pub const MAX_PAIR_RANGE_CHECKS: u64 = 8_000_000;
pub const MAX_ABSOLUTE_OFFSET_BP: f64 = 1_000_000.0;

fn num_of(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Number(n)) => n.as_f64(),
        _ => None,
    }
}

fn str_of(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

/// Python `a - b` for JSON numbers: int when both are ints (checked), otherwise binary64.
fn py_sub(a: &Value, b: &Value) -> Res<Value> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) if x.is_i64() && y.is_i64() => x
            .as_i64()
            .unwrap()
            .checked_sub(y.as_i64().unwrap())
            .map(|v| json!(v))
            .ok_or_else(|| "integer overflow".to_string()),
        _ => {
            let r = num_of(Some(a)).unwrap_or(f64::NAN) - num_of(Some(b)).unwrap_or(f64::NAN);
            Ok(fv(r))
        }
    }
}

/// Python `_reference_offsets`: panel bins used unshifted when no accepted ladder offsets exist.
fn reference_offsets(marker: &serde_json::Map<String, Value>) -> Res<(Value, [Value; 2], Value)> {
    let bins = marker.get("bins").and_then(|b| b.as_array()).ok_or("bins must be a list")?;
    let mut by_bin = serde_json::Map::new();
    let mut bin_offsets = serde_json::Map::new();
    for bin in bins {
        let label = str_of(&bin["label"]);
        by_bin.insert(
            label.clone(),
            json!({"offset_bp": 0.0, "origin": "reference_bins", "source_bin": label, "derived": true}),
        );
        bin_offsets.insert(label, fv(0.0));
    }
    let record = json!({"bin_offsets": bin_offsets, "min_offset_bp": 0.0, "max_offset_bp": 0.0});
    let window = [marker["min_bp"].clone(), marker["max_bp"].clone()];
    let metadata = json!({
        "policy": "reference_bins",
        "bin_order": "kit_declared_order",
        "by_bin": by_bin,
        "min_offset_bp": 0.0,
        "max_offset_bp": 0.0,
        "reference_bins": true,
    });
    Ok((record, window, metadata))
}

/// Python `_marker_offsets`: resolves per-bin offsets for one marker from `match_ladder`-shaped data.
pub fn marker_offsets(marker: &Value, ladder: &Value, policy: &str) -> Res<(Value, [Value; 2], Value)> {
    let marker_map = marker.as_object().ok_or("marker must be an object")?;
    let name = marker_map.get("name").cloned().unwrap_or(Value::Null);
    let name_key = name.as_str().unwrap_or("").to_string();
    let mut policy = policy.to_string();
    if policy == "reference_bins" {
        let ladder_map = ladder.as_object();
        let record = match ladder_map {
            Some(m) => match m.get("markers") {
                None => None,
                Some(Value::Object(markers)) => markers.get(&name_key),
                Some(_) => return Err("markers must be an object".into()),
            },
            None => None,
        };
        let accepted_status = ladder_map
            .map(|m| matches!(m.get("status").and_then(|s| s.as_str()), Some("accepted") | Some("partial")))
            .unwrap_or(false);
        let record_ok = matches!(record, Some(Value::Object(r)) if r.get("status") == Some(&json!("accepted")));
        if ladder_map.is_none() || !accepted_status || !record_ok {
            return reference_offsets(marker_map);
        }
        policy = "forward_fill".to_string();
    }
    let ladder_map = ladder.as_object().ok_or("ladder_offsets must be an explicit ladder result object")?;
    if !matches!(ladder_map.get("status").and_then(|s| s.as_str()), Some("accepted") | Some("partial")) {
        return Err("ladder result must be accepted or partially accepted".into());
    }
    let markers = match ladder_map.get("markers") {
        Some(Value::Object(m)) => m,
        _ => return Err("ladder_offsets.markers must be an object".into()),
    };
    let record = match markers.get(&name_key) {
        Some(Value::Object(r)) if r.get("status") == Some(&json!("accepted")) => r,
        _ => return Err("missing or unaccepted ladder offsets".into()),
    };
    let offsets = match record.get("bin_offsets") {
        Some(Value::Object(o)) => o,
        _ => return Err("bin_offsets must be an object".into()),
    };
    let bins = marker_map.get("bins").and_then(|b| b.as_array()).ok_or("bins must be a list")?;
    let labels: Vec<String> = bins.iter().map(|b| str_of(&b["label"])).collect();
    if offsets.is_empty() || offsets.keys().any(|k| !labels.contains(k)) {
        return Err("bin_offsets need direct offsets with exact canonical bin-label keys".into());
    }
    for bound in ["min_offset_bp", "max_offset_bp"] {
        match num_of(record.get(bound)) {
            Some(x) if x.abs() <= MAX_ABSOLUTE_OFFSET_BP => {}
            _ => return Err(format!("missing, nonfinite or excessive {}", bound)),
        }
    }
    for (label, offset) in offsets {
        match num_of(Some(offset)) {
            Some(x) if x.abs() <= MAX_ABSOLUTE_OFFSET_BP => {}
            _ => return Err(format!("nonfinite or excessive offset for bin {}", label)),
        }
    }
    let mut resolved = serde_json::Map::new();
    let mut by_bin = serde_json::Map::new();
    let mut previous: Option<String> = None;
    for bin in bins {
        let label = str_of(&bin["label"]);
        let (delta, origin, source) = if let Some(direct) = offsets.get(&label) {
            (direct.clone(), "direct_ladder", Value::String(label.clone()))
        } else if policy == "reject" {
            return Err(format!("missing offset for bin {}", label));
        } else if previous.is_none() {
            (record["min_offset_bp"].clone(), "virtual_min", Value::Null)
        } else {
            let prev = previous.clone().unwrap();
            (resolved[&prev].clone(), "forward_fill", Value::String(prev))
        };
        resolved.insert(label.clone(), delta.clone());
        by_bin.insert(
            label.clone(),
            json!({"offset_bp": delta, "origin": origin, "source_bin": source, "derived": origin != "direct_ladder"}),
        );
        previous = Some(label);
    }
    let low = py_sub(&marker_map["min_bp"], &record["min_offset_bp"])?;
    let high = py_sub(&marker_map["max_bp"], &record["max_offset_bp"])?;
    let (low_f, high_f) = (num_of(Some(&low)).unwrap(), num_of(Some(&high)).unwrap());
    if !low_f.is_finite() || !high_f.is_finite() || low_f > high_f {
        return Err("ladder offsets produce an invalid marker window".into());
    }
    for bin in bins {
        let label = str_of(&bin["label"]);
        let sum = num_of(Some(&bin["center_bp"])).unwrap_or(f64::NAN) + num_of(resolved.get(&label)).unwrap_or(f64::NAN);
        if !sum.is_finite() {
            return Err("ladder offset arithmetic is nonfinite".into());
        }
    }
    let resolved_record = json!({
        "bin_offsets": resolved,
        "min_offset_bp": record["min_offset_bp"],
        "max_offset_bp": record["max_offset_bp"],
    });
    let metadata = json!({
        "policy": policy,
        "bin_order": "kit_declared_order",
        "by_bin": by_bin,
        "min_offset_bp": record["min_offset_bp"],
        "max_offset_bp": record["max_offset_bp"],
    });
    Ok((resolved_record, [low, high], metadata))
}

/// Python `_label`: assigns one peak to the bins that contain its offset-corrected size.
fn label(peak: &Value, bins: &[Value], offsets: &serde_json::Map<String, Value>, stutter: bool) -> Res<Value> {
    let size = num_of(Some(&peak["size_bp"])).unwrap_or(f64::NAN);
    let mut hits: Vec<&Value> = Vec::new();
    for bin in bins {
        let key = str_of(&bin["label"]);
        let offset = offsets.get(&key).ok_or_else(|| format!("missing offset for bin {}", key))?;
        let corrected = size + num_of(Some(offset)).unwrap_or(f64::NAN);
        if !corrected.is_finite() {
            return Err("corrected peak size is nonfinite".into());
        }
        let center = num_of(Some(&bin["center_bp"])).unwrap_or(f64::NAN);
        let left = num_of(Some(&bin["left_bp"])).unwrap_or(f64::NAN);
        let right = num_of(Some(&bin["right_bp"])).unwrap_or(f64::NAN);
        if center - left <= corrected && corrected <= center + right {
            hits.push(bin);
        }
    }
    let unique = hits.len() == 1;
    let mut call = serde_json::Map::new();
    for key in ["peak_index", "scan", "size_bp", "height", "flags"] {
        call.insert(key.into(), peak.get(key).cloned().unwrap_or(Value::Null));
    }
    call.insert("label".into(), if unique { hits[0]["label"].clone() } else { json!("OL") });
    call.insert("bin_hits".into(), Value::Array(hits.iter().map(|b| b["label"].clone()).collect()));
    call.insert("review_required".into(), json!(true));
    call.insert("kind".into(), json!(if stutter { "stutter" } else { "selected" }));
    if !stutter {
        call.insert("off_ladder".into(), json!(!unique));
        call.insert(
            "confidence_weight".into(),
            if unique { hits[0]["confidence_weight"].clone() } else { fv(1.0) },
        );
    }
    Ok(Value::Object(call))
}

/// Mutable state of one `_pick_marker` call. Python closures become methods; review objects are stored in
/// candidate order and looked up by peak_index, matching the Python dict of references.
struct Picker<'a> {
    bins: &'a [Value],
    bin_offsets: &'a serde_json::Map<String, Value>,
    by_bin: &'a serde_json::Map<String, Value>,
    repeat: f64,
    cutoff_ratio: f64,
    minus_min: f64,
    minus_max: f64,
    minus_ratio: f64,
    backward: Vec<Value>,
    forward: Vec<Value>,
    clear: Vec<Value>,
    active: Vec<Value>,
    eligible: Vec<bool>,
    reviews: Vec<Value>,
    review_of: std::collections::HashMap<i64, usize>,
    events: Vec<Value>,
    calls: Vec<Value>,
    stutter_calls: Vec<Value>,
    stutter_indices: Vec<usize>,
    threshold: f64,
}

impl<'a> Picker<'a> {
    fn review(&mut self, peak_index: &Value) -> &mut serde_json::Map<String, Value> {
        let pos = self.review_of[&peak_index.as_i64().unwrap()];
        self.reviews[pos].as_object_mut().unwrap()
    }

    fn event(&mut self, pairs: Vec<(&str, Value)>) {
        let mut m = serde_json::Map::new();
        for (k, v) in pairs {
            m.insert(k.into(), v);
        }
        self.events.push(Value::Object(m));
    }

    /// Python `exclude(index, reason, **details)`.
    fn exclude(&mut self, index: usize, reason: &str, details: Vec<(&str, Value)>) {
        self.eligible[index] = false;
        let peak_index = self.active[index]["peak_index"].clone();
        {
            let review = self.review(&peak_index);
            review.insert("state".into(), json!(reason));
            if let Some(Value::Array(a)) = review.get_mut("reasons") {
                a.push(json!(reason));
            }
        }
        let mut pairs = vec![("peak_index", peak_index), ("reason", json!(reason))];
        pairs.extend(details);
        self.event(pairs);
    }

    /// Python `remove_stutter(i, j, ranges, forward)`.
    fn remove_stutter(&mut self, i: usize, j: usize, forward: bool) -> bool {
        let ranges = if forward { self.forward.clone() } else { self.backward.clone() };
        let distance = self.active[j]["size_bp"].as_f64().unwrap_or(f64::NAN)
            - self.active[i]["size_bp"].as_f64().unwrap_or(f64::NAN);
        let (child, parent) = if forward { (j, i) } else { (i, j) };
        let ratio = self.active[child]["height"].as_f64().unwrap_or(0.0)
            / self.active[parent]["height"].as_f64().unwrap_or(0.0);
        for (range_index, range_) in ranges.iter().enumerate() {
            let a = range_["min_bp"].as_f64().unwrap_or(f64::NAN);
            let b = range_["max_bp"].as_f64().unwrap_or(f64::NAN);
            let (low, high) = if a <= b { (a, b) } else { (b, a) };
            if low <= distance && distance <= high && ratio < range_["ratio"].as_f64().unwrap_or(f64::NAN) {
                let reason = if forward { "forward_stutter" } else { "backward_stutter" };
                let parent_index = self.active[parent]["peak_index"].clone();
                self.exclude(
                    child,
                    reason,
                    vec![
                        ("parent_peak_index", parent_index),
                        ("distance_bp", fv(distance)),
                        ("height_ratio", fv(ratio)),
                        ("range_index", json!(range_index)),
                    ],
                );
                self.stutter_indices.push(child);
                return true;
            }
        }
        false
    }

    /// Python `clear_or_label(i, stutter)`.
    fn clear_or_label(&mut self, i: usize, stutter: bool) -> Res<()> {
        let peak = self.active[i].clone();
        let size = peak["size_bp"].as_f64().unwrap_or(f64::NAN);
        let peak_index = peak["peak_index"].clone();
        for range_ in self.clear.clone() {
            let lo = range_["min_bp"].as_f64().unwrap_or(f64::NAN);
            let hi = range_["max_bp"].as_f64().unwrap_or(f64::NAN);
            if lo <= size && size <= hi {
                let review = self.review(&peak_index);
                review.insert("state".into(), json!("clear_range"));
                if let Some(Value::Array(a)) = review.get_mut("reasons") {
                    a.push(json!("clear_range"));
                }
                self.event(vec![("peak_index", peak_index.clone()), ("reason", json!("clear_range"))]);
                return Ok(());
            }
        }
        let mut call = label(&peak, self.bins, self.bin_offsets, stutter)?;
        let hit_count = call["bin_hits"].as_array().map_or(0, |a| a.len());
        if hit_count == 1 {
            let hit = call["bin_hits"][0].as_str().unwrap_or("").to_string();
            let meta = self.by_bin.get(&hit).ok_or_else(|| format!("missing offset metadata for bin {}", hit))?.clone();
            let derived = meta["derived"].clone();
            let cobj = call.as_object_mut().unwrap();
            cobj.insert("offset_bp".into(), meta["offset_bp"].clone());
            cobj.insert("offset_origin".into(), meta["origin"].clone());
            cobj.insert("offset_derived".into(), derived.clone());
            if derived == json!(true) {
                if let Some(Value::Array(flags)) = cobj.get_mut("flags") {
                    if !flags.iter().any(|f| f == "derived_bin_offset") {
                        flags.push(json!("derived_bin_offset"));
                    }
                }
            }
        }
        if stutter {
            self.stutter_calls.push(call);
            return Ok(());
        }
        self.review(&peak_index).insert("state".into(), json!("selected"));
        call.as_object_mut().unwrap().insert("stutter_fraction_annotation".into(), Value::Null);
        let size_h = peak["height"].as_f64().unwrap_or(f64::NAN);
        for neighbor in self.active.clone() {
            let delta = size - neighbor["size_bp"].as_f64().unwrap_or(f64::NAN);
            if self.repeat - 0.5 < delta
                && delta < self.repeat + 0.5
                && neighbor["height"].as_f64().unwrap_or(f64::NAN) / size_h < 0.2
            {
                let ratio = neighbor["height"].as_f64().unwrap_or(f64::NAN) / size_h;
                call.as_object_mut().unwrap().insert("stutter_fraction_annotation".into(), fv(ratio));
                break;
            }
        }
        let off_ladder = call["off_ladder"] == json!(true);
        if off_ladder {
            let reason = if hit_count > 1 { "multiple_bin_hits" } else { "no_bin_hit" };
            self.review(&peak_index).get_mut("reasons").unwrap().as_array_mut().unwrap().push(json!(reason));
            self.event(vec![
                ("peak_index", peak_index.clone()),
                ("reason", json!(reason)),
                ("bin_hits", call["bin_hits"].clone()),
            ]);
        }
        self.calls.push(call);
        Ok(())
    }
}

/// Python `_pick_marker`: selects, labels and annotates the candidate peaks of one marker.
pub fn pick_marker(
    marker: &Value,
    candidates: &[Value],
    offset_record: &Value,
    window: &[Value; 2],
    offset_metadata: &Value,
) -> Res<Value> {
    let parameters = marker.get("parameters").filter(|p| p.is_object()).ok_or("parameters must be an object")?;
    let bins = marker["bins"].as_array().ok_or("bins must be a list")?;
    let bin_offsets = offset_record["bin_offsets"].as_object().ok_or("bin_offsets must be an object")?;
    let by_bin = offset_metadata["by_bin"].as_object().ok_or("by_bin must be an object")?;
    let mut events: Vec<Value> = Vec::new();
    for (label_name, metadata) in by_bin {
        if metadata["derived"] == json!(true) {
            let mut m = serde_json::Map::new();
            m.insert("reason".into(), json!("derived_bin_offset"));
            m.insert("bin_label".into(), json!(label_name));
            for (k, v) in metadata.as_object().unwrap() {
                m.insert(k.clone(), v.clone());
            }
            events.push(Value::Object(m));
        }
    }
    let mut picker = Picker {
        bins,
        bin_offsets,
        by_bin,
        repeat: marker["repeat"].as_f64().unwrap_or(f64::NAN),
        cutoff_ratio: parameters["cutoff_ratio"].as_f64().unwrap_or(f64::NAN),
        minus_min: parameters["minus_a_min_bp"].as_f64().unwrap_or(f64::NAN),
        minus_max: parameters["minus_a_max_bp"].as_f64().unwrap_or(f64::NAN),
        minus_ratio: parameters["minus_a_ratio"].as_f64().unwrap_or(f64::NAN),
        backward: parameters["backward_stutter_ranges"].as_array().cloned().unwrap_or_default(),
        forward: parameters["forward_stutter_ranges"].as_array().cloned().unwrap_or_default(),
        clear: parameters["clear_ranges"].as_array().cloned().unwrap_or_default(),
        active: Vec::new(),
        eligible: Vec::new(),
        reviews: Vec::new(),
        review_of: std::collections::HashMap::new(),
        events,
        calls: Vec::new(),
        stutter_calls: Vec::new(),
        stutter_indices: Vec::new(),
        threshold: 0.0,
    };
    for peak in candidates {
        let mut review = peak.as_object().unwrap().clone();
        review.insert("state".into(), json!("candidate"));
        review.insert("reasons".into(), json!([]));
        picker.review_of.insert(peak["peak_index"].as_i64().unwrap(), picker.reviews.len());
        picker.reviews.push(Value::Object(review));
        let flags = peak["flags"].as_array().cloned().unwrap_or_default();
        let artifacts: Vec<Value> = ["spike", "pullup"]
            .iter()
            .filter(|f| flags.iter().any(|x| x.as_str() == Some(**f)))
            .map(|f| json!(f))
            .collect();
        if !artifacts.is_empty() {
            let peak_index = peak["peak_index"].clone();
            {
                let review = picker.review(&peak_index);
                review.insert("state".into(), json!("excluded_artifact"));
                review.insert("reasons".into(), Value::Array(artifacts.clone()));
            }
            picker.event(vec![
                ("peak_index", peak_index),
                ("reason", json!("excluded_artifact")),
                ("flags", Value::Array(artifacts)),
            ]);
        } else {
            picker.active.push(peak.clone());
        }
    }
    let build = |picker: &mut Picker, cutoff: Option<f64>| -> Value {
        let mut result = serde_json::Map::new();
        result.insert("name".into(), marker["name"].clone());
        result.insert("channel".into(), marker["channel"].clone());
        result.insert("status".into(), json!("tentative"));
        result.insert("reasons".into(), json!([]));
        result.insert("window_bp".into(), json!([window[0], window[1]]));
        result.insert("calls".into(), Value::Array(std::mem::take(&mut picker.calls)));
        result.insert("stutter_calls".into(), Value::Array(std::mem::take(&mut picker.stutter_calls)));
        result.insert("peak_review".into(), Value::Array(std::mem::take(&mut picker.reviews)));
        result.insert("events".into(), Value::Array(std::mem::take(&mut picker.events)));
        result.insert("offsets".into(), offset_metadata.clone());
        if let Some(c) = cutoff {
            result.insert("cutoff_height".into(), fv(c));
        }
        Value::Object(result)
    };
    if picker.active.is_empty() {
        return Ok(build(&mut picker, None));
    }
    let max_height = picker
        .active
        .iter()
        .map(|p| p["height"].as_f64().unwrap_or(f64::NAN))
        .fold(f64::NEG_INFINITY, f64::max);
    picker.threshold = max_height * picker.cutoff_ratio;
    let threshold = picker.threshold;
    picker.eligible = picker
        .active
        .iter()
        .map(|p| p["height"].as_f64().unwrap_or(f64::NAN) > threshold)
        .collect();
    let n = picker.active.len();
    for i in 0..n {
        if !picker.eligible[i] {
            picker.exclude(i, "below_or_equal_cutoff", vec![("threshold", fv(threshold))]);
        }
    }
    for i in 0..n.saturating_sub(1) {
        if !picker.eligible[i] {
            continue;
        }
        for j in (i + 1)..n {
            if !picker.eligible[j] {
                continue;
            }
            let distance = picker.active[j]["size_bp"].as_f64().unwrap_or(f64::NAN)
                - picker.active[i]["size_bp"].as_f64().unwrap_or(f64::NAN);
            let ratio = picker.active[i]["height"].as_f64().unwrap_or(f64::NAN)
                / picker.active[j]["height"].as_f64().unwrap_or(f64::NAN);
            if picker.minus_min <= distance && distance <= picker.minus_max && ratio < picker.minus_ratio {
                let parent = picker.active[j]["peak_index"].clone();
                picker.exclude(
                    i,
                    "minus_a",
                    vec![
                        ("parent_peak_index", parent),
                        ("distance_bp", fv(distance)),
                        ("height_ratio", fv(ratio)),
                    ],
                );
                break;
            }
        }
    }
    // Mutating eligibility in this order is part of the documented contract.
    for i in 0..n.saturating_sub(1) {
        if !picker.eligible[i] {
            continue;
        }
        for j in (i + 1)..n {
            if picker.eligible[j] && picker.remove_stutter(i, j, false) {
                break;
            }
        }
    }
    for j in 1..n {
        if !picker.eligible[j] {
            continue;
        }
        for i in 0..j {
            if picker.eligible[i] && picker.remove_stutter(i, j, true) {
                break;
            }
        }
    }
    for i in 0..n {
        if picker.eligible[i] {
            picker.clear_or_label(i, false)?;
        }
    }
    let stutter = picker.stutter_indices.clone();
    for i in stutter {
        picker.clear_or_label(i, true)?;
    }
    Ok(build(&mut picker, Some(threshold)))
}

// ---------------------------------------------------------------------------
// call_sample (Python `call_sample`): sample-level orchestration over the per-marker stages above.
// ---------------------------------------------------------------------------

use crate::kit_definition::{load_kit_definition, py_json_dumps_ascii};
use crate::spike_adjustment::SpikeState;

const MAX_SOURCE_METADATA_BYTES: usize = 1_048_576;

/// Validation and preparation inside Python's first `try` block; any error becomes a rejection reason.
fn prepare_call(
    peaks: &Value,
    kit: &Value,
    sizing_accepted: &Value,
    source_metadata: Option<&Value>,
    missing_offset_policy: &Value,
    spike_adjustments: Option<&Value>,
) -> Res<(Value, Vec<Value>, Option<SpikeState>)> {
    let definition = load_kit_definition(kit)?;
    match missing_offset_policy.as_str() {
        Some("reject") | Some("forward_fill") | Some("reference_bins") => {}
        _ => return Err("missing_offset_policy must be reject, forward_fill or reference_bins".into()),
    }
    if sizing_accepted != &json!(true) {
        return Err("sizing must be explicitly accepted before sample calling".into());
    }
    if let Some(meta) = source_metadata {
        if !meta.is_object() {
            return Err("source_metadata must be an object or None".into());
        }
        if py_json_dumps_ascii(meta).len() > MAX_SOURCE_METADATA_BYTES {
            return Err(format!("source_metadata exceeds {} bytes", MAX_SOURCE_METADATA_BYTES));
        }
    }
    let prepared = prepare_peaks(peaks)?;
    let spike = match spike_adjustments {
        Some(policy) => Some(SpikeState::new(prepared.clone(), policy)?),
        None => None,
    };
    Ok((definition, prepared, spike))
}

/// One marker of the sample loop. Returns the marker result and the assigned candidate peak indices.
fn process_marker(
    marker: &Value,
    source: &[Value],
    ladder: &Value,
    policy: &str,
    pair_checks: &mut u64,
    spike: &mut Option<SpikeState>,
) -> Res<(Value, Vec<i64>)> {
    let (offset_record, window, offset_metadata) = marker_offsets(marker, ladder, policy)?;
    let low = num_of(Some(&window[0])).unwrap_or(f64::NAN);
    let high = num_of(Some(&window[1])).unwrap_or(f64::NAN);
    let candidates: Vec<Value> = source
        .iter()
        .filter(|p| {
            p["channel"] == marker["channel"] && {
                let size = num_of(Some(&p["size_bp"])).unwrap_or(f64::NAN);
                low <= size && size <= high
            }
        })
        .cloned()
        .collect();
    if candidates.len() > MAX_MARKER_PEAKS {
        return Err(format!("marker exceeds {} candidate peaks", MAX_MARKER_PEAKS));
    }
    let ranges = marker["parameters"]["backward_stutter_ranges"].as_array().map_or(0, |a| a.len())
        + marker["parameters"]["forward_stutter_ranges"].as_array().map_or(0, |a| a.len());
    let n = candidates.len() as u64;
    let pairs = if n > 0 { n * (n - 1) / 2 } else { 0 };
    *pair_checks += pairs * (1 + ranges as u64) + n * n;
    if *pair_checks > MAX_PAIR_RANGE_CHECKS {
        return Err("sample exceeds bounded pair/range work budget".into());
    }
    let mut marker_result = pick_marker(marker, &candidates, &offset_record, &window, &offset_metadata)?;
    if let Some(state) = spike.as_mut() {
        let map = marker_result.as_object_mut().ok_or("marker result must be an object")?;
        state.after_sample_marker(map)?;
    }
    let assigned = candidates.iter().map(|p| p["peak_index"].as_i64().unwrap()).collect();
    Ok((marker_result, assigned))
}

/// Python `call_sample` for a parsed kit and sized peaks. Validation failures return a rejected result.
pub fn call_sample(
    peaks: &Value,
    kit: &Value,
    ladder_offsets: &Value,
    sizing_accepted: &Value,
    source_metadata: Option<&Value>,
    missing_offset_policy: &Value,
    spike_adjustments: Option<&Value>,
) -> Res<Value> {
    let mut diagnostics = Map::new();
    diagnostics.insert("method".into(), json!("explicit_kit_ordered_sample_filters"));
    diagnostics.insert("input_order".into(), json!("stable_size_bp_then_input_index"));
    diagnostics.insert("offset_missing_policy".into(), missing_offset_policy.clone());
    diagnostics.insert("stutter_ranges".into(), json!("explicit_effective_ranges"));
    let mut result = Map::new();
    result.insert("status".into(), json!("rejected"));
    result.insert("reasons".into(), json!([]));
    result.insert("markers".into(), json!([]));
    result.insert("research_only".into(), json!(true));
    result.insert("external_validation".into(), json!(false));
    result.insert("original_runtime_validated".into(), json!(false));
    result.insert("source_metadata".into(), Value::Null);
    result.insert("unassigned_peak_indices".into(), json!([]));
    result.insert("diagnostics".into(), Value::Object(diagnostics));

    let prepared_call = prepare_call(peaks, kit, sizing_accepted, source_metadata, missing_offset_policy, spike_adjustments);
    let (definition, prepared, mut spike) = match prepared_call {
        Ok(v) => v,
        Err(e) => {
            result["reasons"].as_array_mut().unwrap().push(json!(e));
            return Ok(Value::Object(result));
        }
    };
    result.insert(
        "source_metadata".into(),
        source_metadata.map(|m| m.clone()).unwrap_or(Value::Null),
    );
    result.insert("kit_name".into(), definition["name"].clone());

    let policy = missing_offset_policy.as_str().unwrap_or("reject").to_string();
    let mut pair_checks: u64 = 0;
    let mut assigned: Vec<i64> = Vec::new();
    let mut markers_out: Vec<Value> = Vec::new();
    for marker in definition["markers"].as_array().cloned().unwrap_or_default() {
        let source: Vec<Value> = match &spike {
            Some(state) => state.peaks().to_vec(),
            None => prepared.clone(),
        };
        match process_marker(&marker, &source, ladder_offsets, &policy, &mut pair_checks, &mut spike) {
            Ok((marker_result, indices)) => {
                assigned.extend(indices);
                markers_out.push(marker_result);
            }
            Err(e) => {
                let rejected = json!({
                    "name": marker["name"], "channel": marker["channel"], "status": "rejected",
                    "reasons": [e], "calls": [], "stutter_calls": [], "peak_review": [], "events": [],
                });
                markers_out.push(rejected);
            }
        }
    }

    // Original object-reference overlap becomes explicit input-index identity.
    let mut by_peak: Vec<(i64, Vec<(usize, usize)>)> = Vec::new();
    for (mi, marker) in markers_out.iter().enumerate() {
        for (ci, call) in marker["calls"].as_array().map(|c| c.as_slice()).unwrap_or(&[]).iter().enumerate() {
            let idx = call["peak_index"].as_i64().unwrap();
            match by_peak.iter_mut().find(|(p, _)| *p == idx) {
                Some((_, list)) => list.push((mi, ci)),
                None => by_peak.push((idx, vec![(mi, ci)])),
            }
        }
    }
    for (peak_index, calls) in &by_peak {
        if calls.len() > 1 {
            let names: Vec<Value> = calls.iter().map(|(mi, _)| markers_out[*mi]["name"].clone()).collect();
            for &(mi, ci) in calls {
                let flags = markers_out[mi]["calls"][ci]["flags"].as_array_mut().unwrap();
                if !flags.iter().any(|f| f == "cross_marker_overlap") {
                    flags.push(json!("cross_marker_overlap"));
                }
                markers_out[mi]["events"].as_array_mut().unwrap().push(json!({
                    "peak_index": peak_index,
                    "reason": "cross_marker_overlap",
                    "markers": names,
                }));
            }
        }
    }

    let mut unassigned: Vec<i64> = prepared
        .iter()
        .map(|p| p["peak_index"].as_i64().unwrap())
        .filter(|i| !assigned.contains(i))
        .collect();
    unassigned.sort();
    result.insert("unassigned_peak_indices".into(), json!(unassigned));

    let rejected_names: Vec<String> = markers_out
        .iter()
        .filter(|m| m["status"] == json!("rejected"))
        .map(|m| m["name"].as_str().unwrap_or("").to_string())
        .collect();
    if !rejected_names.is_empty() {
        result["reasons"]
            .as_array_mut()
            .unwrap()
            .push(json!(format!("rejected markers: {}", rejected_names.join(", "))));
    }
    let total = markers_out.len();
    if total > 0 && rejected_names.is_empty() {
        result.insert("status".into(), json!("tentative"));
    } else if rejected_names.len() < total {
        result.insert("status".into(), json!("partial"));
    }
    result["diagnostics"]
        .as_object_mut()
        .unwrap()
        .insert("pair_range_work_upper_bound".into(), json!(pair_checks));
    result.insert("markers".into(), Value::Array(markers_out));
    if let Some(state) = spike.as_mut() {
        let summary = state.finish_sample(result["markers"].as_array().unwrap())?;
        result.insert("spike_adjustments".into(), summary);
    }
    Ok(Value::Object(result))
}
