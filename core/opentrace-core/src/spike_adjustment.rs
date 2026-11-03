//! Port of `genemapper_open/spike_adjustment.py`: current-spike transitions on fresh call observations.
//! `SpikeState` mutates the prepared peaks' flags in place, as the Python original does, so later markers in the
//! same sample see the updated flags (documented contract).
use crate::allele_calling::prepare_peaks;
use serde_json::{json, Map, Value};
use std::collections::HashMap;

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

/// Python `f in peak['flags']` for a flag list or a flag mapping.
fn has_flag(flags: Option<&Value>, name: &str) -> bool {
    match flags {
        Some(Value::Array(a)) => a.iter().any(|x| x.as_str() == Some(name)),
        Some(Value::Object(m)) => m.contains_key(name),
        _ => false,
    }
}

/// Python `_number` (quality_control) with explicit bounds.
fn bounded(v: Option<&Value>, name: &str, low: f64, high: f64) -> Res<f64> {
    match v.and_then(num) {
        Some(x) if low <= x && x <= high => Ok(x),
        _ => Err(format!("{}: finite bounded number required", name)),
    }
}

fn boolean(v: Option<&Value>, name: &str) -> Res<bool> {
    match v {
        Some(Value::Bool(b)) => Ok(*b),
        _ => Err(format!("{}: explicit boolean required", name)),
    }
}

/// Python `validate_adjustments`: exact key set, boolean switches and a bounded ladder height ratio.
pub fn validate_adjustments(policy: &Value) -> Res<Value> {
    let msg = "Explicit sample_cutoff, sample_marker_envelope and ladder_height_ratio required";
    let map = policy.as_object().ok_or(msg)?;
    if map.len() != 3 || !["sample_cutoff", "sample_marker_envelope", "ladder_height_ratio"].iter().all(|k| map.contains_key(*k)) {
        return Err(msg.into());
    }
    boolean(map.get("sample_cutoff"), "sample_cutoff")?;
    boolean(map.get("sample_marker_envelope"), "sample_marker_envelope")?;
    bounded(map.get("ladder_height_ratio"), "ladder_height_ratio", 0.0, 1e12)?;
    Ok(Value::Object(map.clone()))
}

struct Entry {
    peak_index: i64,
    initial: bool,
    current: bool,
    events: Vec<Value>,
}

/// Python `SpikeState`: bounded internal state over normalized call_sample peak records.
pub struct SpikeState {
    policy: Value,
    peaks: Vec<Value>,
    pos: HashMap<i64, usize>,
    states: Vec<Entry>,
}

impl SpikeState {
    pub fn new(prepared: Vec<Value>, policy: &Value) -> Res<SpikeState> {
        let policy = validate_adjustments(policy)?;
        let mut pos = HashMap::new();
        let mut states = Vec::with_capacity(prepared.len());
        for (k, p) in prepared.iter().enumerate() {
            let idx = p["peak_index"].as_i64().unwrap();
            pos.insert(idx, k);
            let spike = has_flag(p.get("flags"), "spike");
            states.push(Entry { peak_index: idx, initial: spike, current: spike, events: Vec::new() });
        }
        Ok(SpikeState { policy, peaks: prepared, pos, states })
    }

    /// Prepared peaks in their current (possibly cleared) state.
    pub fn peaks(&self) -> &[Value] {
        &self.peaks
    }

    fn position(&self, peak_index: i64) -> Res<usize> {
        self.pos.get(&peak_index).copied().ok_or_else(|| format!("unknown peak index {}", peak_index))
    }

    fn clear(&mut self, peak_index: i64, reason: &str, details: Vec<(&str, Value)>) -> Res<()> {
        let k = self.position(peak_index)?;
        if self.states[k].current {
            self.states[k].current = false;
            let mut event = Map::new();
            event.insert("reason".into(), json!(reason));
            for (name, value) in details {
                event.insert(name.into(), value);
            }
            self.states[k].events.push(Value::Object(event));
            if let Some(Value::Array(flags)) = self.peaks[k].as_object_mut().and_then(|p| p.get_mut("flags")) {
                if let Some(at) = flags.iter().position(|f| f.as_str() == Some("spike")) {
                    flags.remove(at);
                }
            }
        }
        Ok(())
    }

    /// Python `marker_metric`: records the phase-specific spike metric on the marker.
    fn marker_metric(&self, marker: &mut Map<String, Value>, phase: &str, details: Vec<(&str, Value)>) -> Res<()> {
        let reviews = marker
            .get("peak_review")
            .and_then(|r| r.as_array())
            .cloned()
            .ok_or("peak_review must be a list")?;
        let mut remaining = Vec::new();
        for review in &reviews {
            let idx = review["peak_index"].as_i64().unwrap();
            if self.states[self.position(idx)?].current {
                remaining.push(json!(idx));
            }
        }
        let entry: Vec<Value> = reviews
            .iter()
            .filter(|p| has_flag(p.get("flags"), "spike"))
            .map(|p| p["peak_index"].clone())
            .collect();
        let metric = if remaining.is_empty() { 0 } else { 1 };
        let mut state = Map::new();
        state.insert("phase".into(), json!(phase));
        state.insert("entry_spike_indices".into(), Value::Array(entry));
        state.insert("remaining_peak_indices_at_metric".into(), Value::Array(remaining));
        state.insert("metric".into(), json!(metric));
        for (name, value) in details {
            state.insert(name.into(), value);
        }
        marker.insert("spike_state".into(), Value::Object(state));
        Ok(())
    }

    /// Python `after_sample_marker`: clears spikes at or below the sample cutoff, then records the metric.
    pub fn after_sample_marker(&mut self, marker: &mut Map<String, Value>) -> Res<()> {
        let cutoff = marker.get("cutoff_height").cloned().unwrap_or(Value::Null);
        let reviews = marker
            .get("peak_review")
            .and_then(|r| r.as_array())
            .cloned()
            .ok_or("peak_review must be a list")?;
        let candidates = reviews
            .iter()
            .filter(|p| !has_flag(p.get("flags"), "spike") && !has_flag(p.get("flags"), "pullup"))
            .count();
        let sample_cutoff = self.policy["sample_cutoff"] == json!(true);
        if sample_cutoff && candidates > 0 && !cutoff.is_null() && num(&cutoff).map_or(false, |c| c > 0.0) {
            let c = num(&cutoff).unwrap();
            for review in &reviews {
                let idx = review["peak_index"].as_i64().unwrap();
                if num(&review["height"]).unwrap_or(f64::NAN) <= c {
                    let name = marker.get("name").cloned().unwrap_or(Value::Null);
                    self.clear(
                        idx,
                        "sample_height_at_or_below_cutoff",
                        vec![("marker", name), ("cutoff_height", cutoff.clone())],
                    )?;
                }
            }
        }
        self.marker_metric(
            marker,
            "after_marker_cutoff_before_channel_envelope",
            vec![("cutoff_height", cutoff), ("candidate_count", json!(candidates))],
        )
    }

    /// Python `finish_sample`: channel envelopes over tentative markers, then the envelope clearing rule.
    pub fn finish_sample(&mut self, markers: &[Value]) -> Res<Value> {
        let mut envelopes: Vec<(String, Value, Value)> = Vec::new();
        for marker in markers {
            if marker["status"].as_str() != Some("tentative") {
                continue;
            }
            let window = marker["window_bp"].as_array().ok_or("window_bp must be a pair")?;
            let channel = marker["channel"].as_str().unwrap_or("").to_string();
            let mut low = window[0].clone();
            let mut high = window[1].clone();
            if let Some((_, env_low, env_high)) = envelopes.iter().find(|(c, _, _)| *c == channel) {
                // Python min(low, env_low) / max(high, env_high): the first argument wins ties.
                if num(env_low).unwrap() < num(&low).unwrap() {
                    low = env_low.clone();
                }
                if num(env_high).unwrap() > num(&high).unwrap() {
                    high = env_high.clone();
                }
            }
            match envelopes.iter_mut().find(|(c, _, _)| *c == channel) {
                Some(entry) => {
                    entry.1 = low;
                    entry.2 = high;
                }
                None => envelopes.push((channel, low, high)),
            }
        }
        if self.policy["sample_marker_envelope"] == json!(true) {
            for k in 0..self.peaks.len() {
                let channel = self.peaks[k]["channel"].as_str().unwrap_or("").to_string();
                let size = num(&self.peaks[k]["size_bp"]).unwrap_or(f64::NAN);
                let idx = self.peaks[k]["peak_index"].as_i64().unwrap();
                if let Some((_, low, high)) = envelopes.iter().find(|(c, _, _)| *c == channel) {
                    let (lo, hi) = (num(low).unwrap(), num(high).unwrap());
                    if !(lo <= size && size <= hi) {
                        let bounds = Value::Array(vec![low.clone(), high.clone()]);
                        self.clear(idx, "outside_channel_marker_envelope", vec![("window_bp", bounds)])?;
                    }
                }
            }
        }
        let mut channel_envelopes = Map::new();
        for (channel, low, high) in envelopes {
            channel_envelopes.insert(channel, Value::Array(vec![low, high]));
        }
        Ok(self.summary("sample", vec![("channel_envelopes", Value::Object(channel_envelopes))]))
    }

    /// Python `summary`: per-peak states in ascending peak_index order plus counts.
    pub fn summary(&self, kind: &str, details: Vec<(&str, Value)>) -> Value {
        let mut order: Vec<usize> = (0..self.states.len()).collect();
        order.sort_by_key(|&k| self.states[k].peak_index);
        let states: Vec<Value> = order
            .iter()
            .map(|&k| {
                let s = &self.states[k];
                json!({"peak_index": s.peak_index, "initial_spike": s.initial, "current_spike": s.current,
                       "events": s.events})
            })
            .collect();
        let initial = order.iter().filter(|&&k| self.states[k].initial).count();
        let current = order.iter().filter(|&&k| self.states[k].current).count();
        let cleared = order.iter().filter(|&&k| self.states[k].initial && !self.states[k].current).count();
        let mut out = Map::new();
        out.insert("status".into(), json!("evaluated_subset"));
        out.insert("sample_kind".into(), json!(kind));
        out.insert("policy".into(), self.policy.clone());
        out.insert("initial_spikes".into(), json!(initial));
        out.insert("current_spikes".into(), json!(current));
        out.insert("cleared_spikes".into(), json!(cleared));
        out.insert("peak_states".into(), Value::Array(states));
        out.insert("calling_repeated".into(), json!(false));
        out.insert("marker_order".into(), json!("kit_declared_order"));
        out.insert("scope".into(), json!("current_spike_boolean_on_sized_kit_channels"));
        out.insert("external_validation".into(), json!(false));
        out.insert("original_runtime_validated".into(), json!(false));
        for (name, value) in details {
            out.insert(name.into(), value);
        }
        Value::Object(out)
    }
}

/// Python `adjust_ladder_spikes`: clears spikes at or below each channel's ladder height cutoff, per dye channel.
pub fn adjust_ladder_spikes(peaks: &Value, observations: &Value, policy: &Value) -> Res<Value> {
    let prepared = prepare_peaks(peaks)?;
    let mut state = SpikeState::new(prepared, policy)?;
    let mut observed = observations.as_object().cloned().ok_or("observations must be an object")?;
    let mut markers: Vec<Value> = observed
        .get("markers")
        .and_then(|m| m.as_array())
        .cloned()
        .ok_or("markers must be a list")?;
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    for (idx, marker) in markers.iter().enumerate() {
        if marker["status"].as_str() == Some("tentative") {
            let channel = marker["channel"].as_str().unwrap_or("").to_string();
            match groups.iter_mut().find(|(c, _)| *c == channel) {
                Some((_, list)) => list.push(idx),
                None => groups.push((channel, vec![idx])),
            }
        }
    }
    let ratio = num(&state.policy["ladder_height_ratio"]).unwrap_or(f64::NAN);
    let mut thresholds = Map::new();
    for (channel, indices) in groups {
        let mut maximum: Option<f64> = None;
        for &idx in &indices {
            for call in markers[idx]["calls"].as_array().map(|c| c.as_slice()).unwrap_or(&[]) {
                let h = num(&call["height"]).ok_or("call height must be a number")?;
                maximum = Some(match maximum {
                    None => h,
                    Some(m) => if h > m { h } else { m },
                });
            }
        }
        let cutoff = match maximum {
            Some(m) if m > 0.0 && ratio > 0.0 => Some(m * ratio),
            _ => None,
        };
        thresholds.insert(
            channel.clone(),
            json!({
                "max_called_height": maximum.map(fv).unwrap_or(Value::Null),
                "cutoff_height": cutoff.map(fv).unwrap_or(Value::Null),
            }),
        );
        if let Some(c) = cutoff {
            for k in 0..state.peaks.len() {
                let peak_channel = state.peaks[k]["channel"].as_str().unwrap_or("").to_string();
                let height = num(&state.peaks[k]["height"]).unwrap_or(f64::NAN);
                let idx = state.peaks[k]["peak_index"].as_i64().unwrap();
                if peak_channel == channel && height <= c {
                    state.clear(idx, "ladder_height_at_or_below_cutoff", vec![("cutoff_height", fv(c))])?;
                }
            }
        }
        for idx in indices {
            let cutoff_value = cutoff.map(fv).unwrap_or(Value::Null);
            let marker = markers[idx].as_object_mut().ok_or("marker must be an object")?;
            state.marker_metric(marker, "after_ladder_height_cutoff", vec![("cutoff_height", cutoff_value)])?;
        }
    }
    observed.insert("markers".into(), Value::Array(markers));
    let summary = state.summary("ladder", vec![("channel_thresholds", Value::Object(thresholds))]);
    observed.insert("spike_adjustments".into(), summary);
    Ok(Value::Object(observed))
}

/// Python `apply_peak_states`: copies current states onto the peak records, retaining history.
pub fn apply_peak_states(peaks: &Value, summary: &Value) -> Res<Value> {
    let mut result = peaks.as_array().cloned().ok_or("peaks must be a list")?;
    let states = summary["peak_states"].as_array().ok_or("peak_states must be a list")?;
    for st in states {
        let index = st["peak_index"].as_u64().ok_or("peak_index must be an index")? as usize;
        let current = st["current_spike"].clone();
        let peak = result
            .get_mut(index)
            .and_then(|p| p.as_object_mut())
            .ok_or("peak index outside the peak list")?;
        peak.insert("spike_adjustment".into(), st.clone());
        peak.insert("spike".into(), current.clone());
        match peak.get("flags").cloned() {
            Some(Value::Object(mut flags)) => {
                flags.insert("spike".into(), current);
                peak.insert("flags".into(), Value::Object(flags));
            }
            Some(Value::Array(list)) => {
                let mut next: Vec<Value> = list.into_iter().filter(|f| f.as_str() != Some("spike")).collect();
                if current == json!(true) {
                    next.push(json!("spike"));
                }
                peak.insert("flags".into(), Value::Array(next));
            }
            _ => {}
        }
    }
    Ok(Value::Array(result))
}

/// Python `marker_has_spike`: phase-specific binary metric when present, otherwise the review flags.
pub fn marker_has_spike(marker: &Value) -> Res<bool> {
    match marker.get("spike_state") {
        Some(state) if !state.is_null() => {
            let map = state.as_object().ok_or("Explicit binary phase-specific spike metric required")?;
            match map.get("metric") {
                Some(Value::Number(n)) if n.is_i64() && matches!(n.as_i64(), Some(0) | Some(1)) => {
                    Ok(n.as_i64() == Some(1))
                }
                _ => Err("Explicit binary phase-specific spike metric required".into()),
            }
        }
        _ => {
            let reviews = marker.get("peak_review").and_then(|r| r.as_array()).cloned().unwrap_or_default();
            Ok(reviews.iter().any(|p| has_flag(p.get("flags"), "spike")))
        }
    }
}
