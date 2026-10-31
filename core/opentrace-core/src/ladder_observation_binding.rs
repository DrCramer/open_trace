//! Port of `genemapper_open/ladder_observation_binding.py`.
use crate::allele_calling::prepare_peaks;
use crate::kit_definition::load_kit_definition;
use crate::pullup_quality::digest;
use crate::spike_adjustment::adjust_ladder_spikes;
use serde_json::{json, Map, Value};
use std::collections::{BTreeSet, HashMap};

pub type Res<T> = Result<T, String>;

const KEY: &str = "observation_binding";
const SCOPE: &str = "consistency_only_not_caller_authentication";

fn dig(v: &Value) -> Res<String> {
    digest(v).map_err(|_| "JSON-compatible finite ladder binding inputs required".to_string())
}

fn without_key(m: &Map<String, Value>, key: &str) -> Map<String, Value> {
    m.iter().filter(|(k, _)| k.as_str() != key).map(|(k, v)| (k.clone(), v.clone())).collect()
}

fn without_binding(observations: &Value) -> Res<Map<String, Value>> {
    let m = observations.as_object().ok_or("Ladder observations must be an object")?;
    let result = without_key(m, KEY);
    match result.get("markers") {
        Some(Value::Array(a)) if a.iter().all(|x| x.is_object()) => Ok(result),
        _ => Err("Explicit ladder observation marker list required".into()),
    }
}

fn immutable(observations: &Value) -> Res<Value> {
    let result = without_binding(observations)?;
    let mut result = without_key(&result, "spike_adjustments");
    let markers: Vec<Value> = result["markers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| Value::Object(without_key(m.as_object().unwrap(), "spike_state")))
        .collect();
    result.insert("markers".into(), Value::Array(markers));
    Ok(Value::Object(result))
}

fn index_of(p: &Value) -> usize {
    p["peak_index"].as_u64().unwrap() as usize
}

fn sources(peaks: &Value, initial: Option<&HashMap<usize, bool>>) -> Res<Vec<Value>> {
    let mut prepared = prepare_peaks(peaks)?;
    prepared.sort_by_key(|p| index_of(p));
    let mut out = Vec::new();
    for peak in prepared {
        let index = index_of(&peak);
        let mut flags: BTreeSet<String> =
            peak["flags"].as_array().unwrap().iter().map(|f| f.as_str().unwrap().to_string()).collect();
        if let Some(init) = initial {
            flags.remove("spike");
            if init[&index] {
                flags.insert("spike".into());
            }
        }
        let source = peaks[index].as_object().unwrap();
        let mut r = peak.as_object().unwrap().clone();
        r.insert("flags".into(), Value::Array(flags.into_iter().map(Value::String).collect()));
        r.insert("id".into(), json!({"present": source.contains_key("id"), "value": source.get("id").cloned().unwrap_or(Value::Null)}));
        r.insert(
            "offscaled".into(),
            json!({"present": source.contains_key("offscaled"), "value": source.get("offscaled").cloned().unwrap_or(Value::Null)}),
        );
        out.push(Value::Object(r));
    }
    Ok(out)
}

fn check_digest(actual: &Value, expected: &str, name: &str) -> Res<()> {
    if dig(actual)? != expected {
        return Err(format!("Bound ladder {} changed or is stale", name));
    }
    Ok(())
}

pub fn bind(peaks: &Value, kit: &Value, matching: &Value, observations: &Value) -> Res<Value> {
    let srcs = sources(peaks, None)?;
    let mut result = without_binding(observations)?;
    let markers_have_state = result["markers"].as_array().unwrap().iter().any(|m| m.as_object().unwrap().contains_key("spike_state"));
    let peaks_have_meta = peaks.as_array().unwrap().iter().any(|p| p.as_object().map_or(false, |o| o.contains_key("spike_adjustment")));
    if result.contains_key("spike_adjustments") || markers_have_state || peaks_have_meta {
        return Err("Bind fresh ladder observations before spike adjustment".into());
    }
    if !matching.is_object() {
        return Err("Explicit ladder matching object required".into());
    }
    let source_sha = dig(&Value::Array(srcs))?;
    let kit_sha = dig(&load_kit_definition(kit)?)?;
    let matching_sha = dig(matching)?;
    let observations_sha = dig(&Value::Object(result.clone()))?;
    result.insert(
        KEY.into(),
        json!({
            "schema_version": 1, "scope": SCOPE, "source_sha256": source_sha, "kit_sha256": kit_sha,
            "matching_sha256": matching_sha, "observations_sha256": observations_sha,
        }),
    );
    Ok(Value::Object(result))
}

fn flag_map(peaks: &Value) -> Res<HashMap<usize, bool>> {
    Ok(prepare_peaks(peaks)?
        .iter()
        .map(|p| (index_of(p), p["flags"].as_array().unwrap().iter().any(|f| f == "spike")))
        .collect())
}

fn validate_adjusted(peaks: &Value, observations: &Map<String, Value>, binding: &Map<String, Value>) -> Res<()> {
    let summary = match &observations["spike_adjustments"] {
        Value::Object(s) => s,
        _ => return Err("Explicit ladder spike adjustment summary required".into()),
    };
    let plist = peaks.as_array().unwrap();
    let states = match summary.get("peak_states") {
        Some(Value::Array(s)) if s.len() == plist.len() => s,
        _ => return Err("Complete spike adjustment source states required".into()),
    };
    let mut initial: HashMap<usize, bool> = HashMap::new();
    let mut current: HashMap<usize, bool> = HashMap::new();
    let mut by_index: HashMap<usize, &Value> = HashMap::new();
    for state in states {
        let sm = state.as_object().ok_or("Explicit spike adjustment peak state required")?;
        let index = match sm.get("peak_index") {
            Some(Value::Number(n)) => n.as_i64(),
            _ => None,
        };
        let index = match index {
            Some(i) if i >= 0 && (i as usize) < plist.len() && !initial.contains_key(&(i as usize)) => i as usize,
            _ => return Err("Unique exact spike adjustment input indices required".into()),
        };
        match (sm.get("initial_spike"), sm.get("current_spike")) {
            (Some(Value::Bool(a)), Some(Value::Bool(b))) => {
                initial.insert(index, *a);
                current.insert(index, *b);
            }
            _ => return Err("Explicit initial/current spike booleans required".into()),
        }
        by_index.insert(index, state);
    }
    check_digest(&Value::Array(sources(peaks, Some(&initial))?), binding["source_sha256"].as_str().unwrap(), "source inputs")?;
    let source_states = flag_map(peaks)?;
    if source_states != initial && source_states != current {
        return Err("Source spike flags do not match a complete bound phase".into());
    }
    let has_metadata = plist.iter().any(|p| p.as_object().unwrap().contains_key("spike_adjustment"));
    if has_metadata {
        if source_states != current || plist.iter().any(|p| !p.as_object().unwrap().contains_key("spike_adjustment")) {
            return Err("Applied source spike metadata requires the complete current phase".into());
        }
        for (index, peak) in plist.iter().enumerate() {
            if dig(&peak["spike_adjustment"])? != dig(by_index[&index])? {
                return Err("Source spike adjustment metadata differs from summary".into());
            }
        }
    }
    let normalized: HashMap<usize, Value> = prepare_peaks(peaks)?.into_iter().map(|p| (index_of(&p), p)).collect();
    let mut restored: Vec<Value> = Vec::new();
    for (index, peak) in plist.iter().enumerate() {
        let mut pm = without_key(peak.as_object().unwrap(), "spike_adjustment");
        let mut flags: Vec<Value> = normalized[&index]["flags"].as_array().unwrap().iter().filter(|f| *f != "spike").cloned().collect();
        pm.insert("spike".into(), json!(initial[&index]));
        if initial[&index] {
            flags.push(json!("spike"));
        }
        pm.insert("flags".into(), Value::Array(flags));
        restored.push(Value::Object(pm));
    }
    let policy = summary.get("policy").cloned().unwrap_or(Value::Null);
    let replayed = adjust_ladder_spikes(&Value::Array(restored), &immutable(&Value::Object(observations.clone()))?, &policy)
        .map_err(|_| "Invalid bound ladder spike adjustment phase".to_string())?;
    if dig(&replayed)? != dig(&Value::Object(without_binding(&Value::Object(observations.clone()))?))? {
        return Err("Ladder spike adjustment phase differs from the declared transition".into());
    }
    Ok(())
}

fn hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

pub fn validate(peaks: &Value, kit: &Value, matching: &Value, observations: &Value) -> Res<Value> {
    let srcs = sources(peaks, None)?;
    let mut result = without_binding(observations)?;
    let fields = ["schema_version", "scope", "source_sha256", "kit_sha256", "matching_sha256", "observations_sha256"];
    let binding = match observations.get(KEY) {
        Some(Value::Object(b))
            if b.len() == fields.len()
                && fields.iter().all(|k| b.contains_key(*k))
                && matches!(b.get("schema_version"), Some(Value::Number(n)) if n.as_i64() == Some(1))
                && b.get("scope") == Some(&json!(SCOPE)) =>
        {
            b
        }
        _ => return Err("Versioned ladder observation consistency binding required".into()),
    };
    for key in ["source_sha256", "kit_sha256", "matching_sha256", "observations_sha256"] {
        if !matches!(&binding[key], Value::String(s) if hex64(s)) {
            return Err("Valid ladder observation consistency digests required".into());
        }
    }
    check_digest(&load_kit_definition(kit)?, binding["kit_sha256"].as_str().unwrap(), "kit")?;
    check_digest(matching, binding["matching_sha256"].as_str().unwrap(), "matching")?;
    check_digest(&immutable(&Value::Object(result.clone()))?, binding["observations_sha256"].as_str().unwrap(), "observation structure")?;
    if result.contains_key("spike_adjustments") {
        validate_adjusted(peaks, &result, binding)?;
    } else {
        let state = result["markers"].as_array().unwrap().iter().any(|m| m.as_object().unwrap().contains_key("spike_state"));
        let meta = peaks.as_array().unwrap().iter().any(|p| p.as_object().unwrap().contains_key("spike_adjustment"));
        if state || meta {
            return Err("Spike phase metadata requires a checked adjustment summary".into());
        }
        check_digest(&Value::Array(srcs), binding["source_sha256"].as_str().unwrap(), "source inputs")?;
    }
    result.insert(KEY.into(), Value::Object(binding.clone()));
    Ok(Value::Object(result))
}
