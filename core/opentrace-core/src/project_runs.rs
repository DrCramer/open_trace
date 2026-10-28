//! Port of `genemapper_open/project_runs.py`: persistent run revisions over compact sized observations.
use crate::control_workflow::attach_control_result;
use crate::kit_definition::load_kit_definition;
use crate::ladder_quality_workflow::load_ladder_quality_definition;
use crate::marker_quality_workflow::{attach_marker_quality, load_marker_quality_definition};
use crate::project_store::{canonical_d, digest, se, text, ProjectStore, SErr, MAX_RUN_INPUT_BYTES, R};
use crate::pyjson::{self, LoadErr};
use crate::quality_control::labels;
use crate::run_workflow::{ladder_entries, run_str_batch_workflow, RunOptions};
use crate::str_workflow::compact_analysis;
use serde_json::{json, Map, Value};
use std::collections::HashSet;

fn ve(e: String) -> SErr {
    se(e)
}

fn is_int(v: &Value) -> bool {
    matches!(v, Value::Number(n) if n.is_i64() || n.is_u64())
}

fn keys_are(m: &Map<String, Value>, keys: &[&str]) -> bool {
    m.len() == keys.len() && keys.iter().all(|k| m.contains_key(*k))
}

/// `prepare_run_inputs(source)`: validated, normalized version-1 run inputs.
pub fn prepare_run_inputs(source: &Value) -> R<Value> {
    let parsed;
    let source = if let Value::String(s) = source {
        if s.len() > MAX_RUN_INPUT_BYTES {
            return Err(se("Run inputs exceed16MiB"));
        }
        parsed = match pyjson::loads(s, true) {
            Ok(v) => v,
            Err(LoadErr::Dup(_)) => return Err(se("Duplicate run input field")),
            Err(e) => return Err(se(e.message())),
        };
        &parsed
    } else {
        source
    };
    let fields = ["schema_version", "kit", "ladder_quality_definition", "missing_offset_policy", "ladders", "samples"];
    let m = match source {
        Value::Object(m) if keys_are(m, &fields) && is_int(&m["schema_version"]) && m["schema_version"].as_i64() == Some(1) => m,
        _ => return Err(se("Explicit version1 run inputs with all fields required")),
    };
    if !matches!(m["missing_offset_policy"].as_str(), Some("reject") | Some("forward_fill")) {
        return Err(se("Invalid missing offset policy"));
    }
    let ladders = match &m["ladders"] {
        Value::Array(a) if a.len() <= 128 => a,
        _ => return Err(se("Explicit0..128 ladders required")),
    };
    if !ladders.is_empty() {
        ladder_entries(&m["ladders"], "analysis").map_err(ve)?;
    }
    let samples = match &m["samples"] {
        Value::Array(a) if a.len() <= 128 => a,
        _ => return Err(se("Explicit0..128 samples required")),
    };
    let kit = load_kit_definition(&m["kit"]).map_err(ve)?;
    let lqd = load_ladder_quality_definition(&m["ladder_quality_definition"]).map_err(ve)?;
    let mut out_ladders: Vec<Value> = Vec::new();
    let mut names: HashSet<String> = HashSet::new();
    for entry in ladders {
        let id = entry["ladder_id"].as_str().unwrap().to_lowercase();
        if !names.insert(id) {
            return Err(se("Persistent run ladder names must be unique ignoring ASCII case"));
        }
        if !entry["analysis"].is_object() {
            return Err(se("Sized ladder report required"));
        }
        out_ladders.push(json!({"ladder_id": entry["ladder_id"], "analysis": compact_analysis(&entry["analysis"])}));
    }
    let mut keys: HashSet<String> = HashSet::new();
    let mut out_samples: Vec<Value> = Vec::new();
    for entry in samples {
        let allowed = ["sample_key", "analysis", "marker_quality_definition", "control_definition"];
        let em = match entry {
            Value::Object(e) if e.contains_key("sample_key") && e.contains_key("analysis") && e.keys().all(|k| allowed.contains(&k.as_str())) => e,
            _ => return Err(se("Run sample requires sample_key/analysis and optional explicit quality definitions")),
        };
        let key = text(&em["sample_key"], "sample_key", 256)?;
        if keys.contains(&key) {
            return Err(se("Run sample_key must be unique"));
        }
        if !em["analysis"].is_object() {
            return Err(se("Sized sample report required"));
        }
        keys.insert(key.clone());
        let control = em.get("control_definition").cloned().unwrap_or(Value::Null);
        if !control.is_null() {
            let c = match &control {
                Value::Object(c) if keys_are(c, &["schema_version", "sample_type", "markers"]) && is_int(&c["schema_version"]) && c["schema_version"].as_i64() == Some(1) => c,
                _ => return Err(se("Explicit version1 control definition object required")),
            };
            let st_ok = matches!(c["sample_type"].as_str(), Some("Positive Control") | Some("Negative Control") | Some("Sample"));
            let markers = match &c["markers"] {
                Value::Object(mm) if st_ok && mm.len() <= 128 => mm,
                _ => return Err(se("Explicit bounded control type/marker definitions required")),
            };
            for (name, recipe) in markers {
                text(&json!(name), "control marker", 128)?;
                let r = match recipe {
                    Value::Object(r) if keys_are(r, &["expected_alleles", "quality_sensor", "is_y_marker"]) => r,
                    _ => return Err(se("Explicit control marker recipe required")),
                };
                if !r["quality_sensor"].is_boolean() || !r["is_y_marker"].is_boolean() {
                    return Err(se("Explicit control booleans required"));
                }
                labels(&r["expected_alleles"], "expected_alleles").map_err(ve)?;
            }
        }
        let mq = em.get("marker_quality_definition").cloned().unwrap_or(Value::Null);
        let mq = if mq.is_null() { Value::Null } else { load_marker_quality_definition(&mq).map_err(ve)? };
        out_samples.push(json!({"sample_key": key, "analysis": compact_analysis(&em["analysis"]),
            "marker_quality_definition": mq, "control_definition": control}));
    }
    let normalized = json!({"schema_version": 1, "kit": kit, "ladder_quality_definition": lqd,
        "missing_offset_policy": m["missing_offset_policy"], "ladders": out_ladders, "samples": out_samples});
    canonical_d(&normalized, MAX_RUN_INPUT_BYTES)?;
    Ok(normalized)
}

fn calculate(inputs: &Value) -> R<Vec<Value>> {
    let samples: Vec<Value> = inputs["samples"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| json!({"sample_key": r["sample_key"], "analysis": r["analysis"]}))
        .collect();
    let opts = RunOptions {
        ladder_quality_definition: &inputs["ladder_quality_definition"],
        missing_offset_policy: inputs["missing_offset_policy"].as_str().unwrap(),
        marker_gate: false,
        homozygous_min_height: json!(200.0),
        heterozygous_min_height: json!(100.0),
    };
    let outputs = run_str_batch_workflow(&Value::Array(samples), &inputs["ladders"], &inputs["kit"], &opts).map_err(ve)?;
    let mut outputs = outputs.as_array().cloned().unwrap_or_default();
    for (entry, output) in inputs["samples"].as_array().unwrap().iter().zip(outputs.iter_mut()) {
        let mut report = output["report"].clone();
        if !entry["control_definition"].is_null() {
            report = attach_control_result(&report, &entry["control_definition"]).map_err(ve)?;
        }
        if !entry["marker_quality_definition"].is_null() {
            report = attach_marker_quality(&report, &entry["marker_quality_definition"]).map_err(ve)?;
        }
        output["report"] = report;
    }
    Ok(outputs)
}

fn persist(store: &ProjectStore, run_id: &str, revision_id: &str, parent: Option<&Value>, inputs: &Value, calculated: Vec<Value>, actor: &Value, reason: &Value) -> R<Value> {
    let previous: Vec<(String, Value)> = parent
        .map(|p| {
            p["outputs"].as_array().unwrap().iter().map(|r| (r["sample_key"].as_str().unwrap().to_string(), r["analysis_id"].clone())).collect()
        })
        .unwrap_or_default();
    let inputs_sha = digest(&canonical_d(inputs, MAX_RUN_INPUT_BYTES)?);
    let mut outputs: Vec<Value> = Vec::new();
    let mut records = store.list_samples()?;
    for output in calculated {
        let key = output["sample_key"].as_str().unwrap().to_string();
        let mut report = output["report"].clone();
        let source = report["source_sha256"].clone();
        let prev_id = previous.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone()).unwrap_or(Value::Null);
        report["settings"]["project_run"] = json!({"run_id": run_id, "revision_id": revision_id,
            "parent_revision_id": parent.map_or(Value::Null, |p| p["revision_id"].clone()), "sample_key": key, "inputs_sha256": inputs_sha});
        report["provenance"]["project_run_reanalysis"] = json!({
            "policy": "new_immutable_results_for_all_registered_samples", "previous_analysis_id": prev_id,
            "manual_reviews": "retained_on_previous_analysis; not_carried_forward",
            "sizing_repeated": false, "source_kind": "caller_supplied_compact_sized_observations"});
        let metadata = json!({"kind": "run-dependent sample", "run_id": run_id, "sample_key": key});
        let found = records.iter().find(|r| r["source_sha256"] == source && r["metadata"] == metadata).map(|r| r["sample_id"].clone());
        let sample_id = match found {
            Some(s) => s,
            None => {
                let id = store.add_sample(&source, &metadata, &Value::Null, &Value::Null)?;
                records.push(json!({"sample_id": id, "source_sha256": source, "metadata": metadata}));
                json!(id)
            }
        };
        let analysis_id = store.add_analysis(
            &sample_id,
            &source,
            &json!({"name": "OpenTrace Run", "version": report["analysis_version"]}),
            &report["settings"],
            &report,
            &report["status"],
            &Value::Null,
            &Value::Null,
        )?;
        outputs.push(json!({"sample_key": key, "analysis_id": analysis_id, "previous_analysis_id": prev_id}));
    }
    let identifier = store.append_run_revision(
        &json!(run_id),
        inputs,
        &Value::Array(outputs.clone()),
        &parent.map_or(Value::Null, |p| p["revision_id"].clone()),
        actor,
        reason,
        &json!(revision_id),
        &Value::Null,
    )?;
    store.export_json()?;
    Ok(json!({"run_id": run_id, "revision_id": identifier, "outputs": outputs}))
}

pub fn create_project_run(store: &ProjectStore, name: &Value, inputs: &Value, actor: &Value, reason: &Value) -> R<Value> {
    text(name, "run name", 1024)?;
    text(actor, "actor", 256)?;
    text(reason, "reason", 4096)?;
    let inputs = prepare_run_inputs(inputs)?;
    let calculated = calculate(&inputs)?;
    let run_id = crate::project_store::new_uuid();
    let revision_id = crate::project_store::new_uuid();
    store.atomic(|| {
        store.add_run(name, &json!(run_id), &Value::Null)?;
        persist(store, &run_id, &revision_id, None, &inputs, calculated, actor, reason)
    })
}

pub fn revise_project_run(store: &ProjectStore, run_id: &Value, parent_revision_id: &Value, inputs: &Value, actor: &Value, reason: &Value) -> R<Value> {
    text(actor, "actor", 256)?;
    text(reason, "reason", 4096)?;
    store.get_run(run_id)?;
    let history = store.list_run_revisions(run_id)?;
    match history.last() {
        Some(last) if &last["revision_id"] == parent_revision_id => {}
        _ => return Err(se("Run parent revision is no longer current")),
    }
    let parent = history.last().unwrap().clone();
    let inputs = prepare_run_inputs(inputs)?;
    let calculated = calculate(&inputs)?;
    store.atomic(|| {
        let rid = crate::project_store::new_uuid();
        persist(store, run_id.as_str().unwrap(), &rid, Some(&parent), &inputs, calculated, actor, reason)
    })
}

pub fn change_run_ladder(store: &ProjectStore, run_id: &Value, parent_revision_id: &Value, ladder_id: &Value, analysis: &Value, remove: &Value, actor: &Value, reason: &Value) -> R<Value> {
    let remove = match remove {
        Value::Bool(b) => *b,
        _ => return Err(se("Explicit remove boolean required")),
    };
    let parent = store.get_run_revision(parent_revision_id)?;
    if &parent["run_id"] != run_id {
        return Err(se("Run revision belongs to another run"));
    }
    let mut inputs = parent["inputs"].clone();
    let lid = text(ladder_id, "ladder_id", 256)?;
    let want = lid.to_lowercase();
    let matches: Vec<usize> = inputs["ladders"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .filter(|(_, r)| r["ladder_id"].as_str().unwrap().to_lowercase() == want)
        .map(|(i, _)| i)
        .collect();
    if matches.len() != 1 {
        return Err(se("Select one existing run ladder"));
    }
    if remove {
        if !analysis.is_null() {
            return Err(se("Removal must not supply replacement analysis"));
        }
        inputs["ladders"].as_array_mut().unwrap().remove(matches[0]);
    } else {
        if !analysis.is_object() {
            return Err(se("Replacement sized analysis required"));
        }
        inputs["ladders"][matches[0]]["analysis"] = compact_analysis(analysis);
    }
    revise_project_run(store, run_id, parent_revision_id, &inputs, actor, reason)
}
