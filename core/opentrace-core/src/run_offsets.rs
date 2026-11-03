//! Port of `genemapper_open/run_offsets.py`: per-key run contributions, means and fresh reanalysis replacement.
use crate::kit_definition::load_kit_definition;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};

pub type Res<T> = Result<T, String>;

const ALLOWED_CODES: [f64; 3] = [0.0, -2.0, 0.5];
const KINDS: [&str; 3] = ["bin", "virtual_min", "virtual_max"];

fn fv(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

fn name(v: &Value, what: &str, limit: usize, ascii_only: bool) -> Res<String> {
    match v {
        Value::String(s) if (1..=limit).contains(&s.chars().count()) && (!ascii_only || s.is_ascii()) => Ok(s.clone()),
        _ => Err(format!("Bounded {}{} required", if ascii_only { "ASCII " } else { "" }, what)),
    }
}

fn code(v: &Value) -> Res<()> {
    match v {
        Value::Null => Ok(()),
        Value::Number(n) if n.as_f64().map_or(false, f64::is_finite) => Ok(()),
        _ => Err("Explicit finite quality_code or None required".into()),
    }
}

fn offset(v: &Value) -> Res<f64> {
    match v {
        Value::Number(n) => match n.as_f64() {
            Some(f) if f.is_finite() && f.abs() <= 1_000_000.0 => Ok(f),
            _ => Err("Finite bounded offset required".into()),
        },
        _ => Err("Finite bounded offset required".into()),
    }
}

/// `Math.round(value*1e9)/1e9` with ties toward positive infinity.
pub fn store_offset_bp(v: &Value) -> Res<f64> {
    let product = offset(v)? * 1_000_000_000.0;
    let rounded = (product + 0.5).floor() as i64;
    Ok(rounded as f64 / 1_000_000_000.0)
}

fn mean(values: &[f64]) -> f64 {
    let mut total = 0.0;
    for v in values {
        total += v;
    }
    total / values.len() as f64
}

fn lower(s: &str) -> String {
    s.to_ascii_lowercase()
}

type Identity = (String, String, Option<String>, String);

fn identity(row: &Value) -> Identity {
    (
        row["marker"].as_str().unwrap().to_string(),
        row["key_kind"].as_str().unwrap().to_string(),
        row["bin_label"].as_str().map(String::from),
        lower(row["ladder_id"].as_str().unwrap()),
    )
}

fn state(st: &Value) -> Res<Map<String, Value>> {
    if st.is_null() {
        let mut m = Map::new();
        m.insert("schema_version".into(), json!(1));
        m.insert("contributions".into(), json!([]));
        m.insert("events".into(), json!([]));
        return Ok(m);
    }
    let bad = || "Explicit versioned run offset state required".to_string();
    let m = st.as_object().ok_or_else(bad)?;
    if m.len() != 3 || !["schema_version", "contributions", "events"].iter().all(|k| m.contains_key(*k)) {
        return Err(bad());
    }
    match &m["schema_version"] {
        Value::Number(n) if n.as_i64() == Some(1) => {}
        _ => return Err(bad()),
    }
    let rows = m["contributions"].as_array().ok_or_else(bad)?;
    if !m["events"].is_array() {
        return Err(bad());
    }
    let mut seen: HashSet<Identity> = HashSet::new();
    for row in rows {
        let r = match row {
            Value::Object(r) if r.len() == 6 && ["marker", "key_kind", "bin_label", "ladder_id", "offset_bp", "quality_code"].iter().all(|k| r.contains_key(*k)) => r,
            _ => return Err("Structured run offset contribution required".into()),
        };
        name(&r["marker"], "marker", 128, false)?;
        name(&r["ladder_id"], "ladder_id", 256, true)?;
        let kind = match r["key_kind"].as_str() {
            Some(k) if KINDS.contains(&k) => k,
            _ => return Err("Explicit bin/virtual_min/virtual_max key kind required".into()),
        };
        if kind == "bin" {
            name(&r["bin_label"], "bin label", 128, false)?;
        } else if !r["bin_label"].is_null() {
            return Err("Virtual endpoints must have bin_label=None".into());
        }
        offset(&r["offset_bp"])?;
        code(&r["quality_code"])?;
        if !seen.insert(identity(row)) {
            return Err("Duplicate per-key case-insensitive ladder contribution".into());
        }
    }
    Ok(m.clone())
}

fn admitted(code: &Value) -> bool {
    matches!(code, Value::Number(n) if n.as_f64().map_or(false, |f| ALLOWED_CODES.contains(&f)))
}

fn unique(items: impl Iterator<Item = String>) -> Vec<Value> {
    let mut seen = HashSet::new();
    items.filter(|s| seen.insert(s.clone())).map(Value::String).collect()
}

fn contributions(m: &mut Map<String, Value>) -> &mut Vec<Value> {
    m.get_mut("contributions").unwrap().as_array_mut().unwrap()
}

fn events(m: &mut Map<String, Value>) -> &mut Vec<Value> {
    m.get_mut("events").unwrap().as_array_mut().unwrap()
}

pub fn remove_run_ladder(st: &Value, ladder_id: &Value) -> Res<Value> {
    let ladder_id = name(ladder_id, "ladder_id", 256, true)?;
    let mut result = state(st)?;
    let folded = lower(&ladder_id);
    let (removed, kept): (Vec<Value>, Vec<Value>) = contributions(&mut result).drain(..).partition(|r| lower(r["ladder_id"].as_str().unwrap()) == folded);
    *contributions(&mut result) = kept;
    events(&mut result).push(json!({
        "action": "remove_ladder", "ladder_id": ladder_id, "removed_contributions": removed.len(),
        "matched_ladder_ids": unique(removed.iter().map(|r| r["ladder_id"].as_str().unwrap().to_string())),
        "identity_comparison": "ASCII_equalsIgnoreCase"}));
    Ok(Value::Object(result))
}

pub fn validate_run_offset_state(st: &Value) -> Res<Value> {
    Ok(Value::Object(state(st)?))
}

pub fn ladder_window_offset(st: &Value, marker: &Value, local_offset_bp: &Value) -> Res<Value> {
    let marker = name(marker, "marker", 128, false)?;
    let st = state(st)?;
    let contributors: Vec<Value> = st["contributions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["marker"] == json!(marker) && r["key_kind"] == json!("virtual_max") && admitted(&r["quality_code"]))
        .cloned()
        .collect();
    let mut values: Vec<f64> = contributors.iter().map(|r| r["offset_bp"].as_f64().unwrap()).collect();
    let mut local = Value::Null;
    if !local_offset_bp.is_null() {
        let l = store_offset_bp(local_offset_bp)?;
        values.push(l);
        local = fv(l);
    }
    Ok(json!({
        "marker": marker, "offset_bp": fv(if values.is_empty() { 0.0 } else { mean(&values) }),
        "run_contributors": contributors, "local_offset_bp": local, "value_count": values.len(),
        "empty_value": fv(0.0), "local_quality_gate_applied": false}))
}

pub fn upsert_run_offsets(st: &Value, ladder_id: &Value, quality_code: &Value, result: &Value) -> Res<Value> {
    let ladder_id = name(ladder_id, "ladder_id", 256, true)?;
    let mut marker_codes: Option<Vec<(String, Value)>> = None;
    let event_code: Value;
    if let Value::Object(m) = quality_code {
        let mut codes = Vec::new();
        for (k, c) in m {
            let n = name(&json!(k), "marker", 128, false)?;
            code(c)?;
            codes.push((n, c.clone()));
        }
        event_code = Value::Object(codes.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
        marker_codes = Some(codes);
    } else {
        code(quality_code)?;
        event_code = quality_code.clone();
    }
    let status = result.get("status").and_then(|s| s.as_str()).filter(|s| ["accepted", "partial", "rejected"].contains(s));
    let markers = result.get("markers").and_then(|m| m.as_object());
    let (status, markers) = match (status, markers) {
        (Some(s), Some(m)) if result.is_object() => (s, m),
        _ => return Err("Explicit ladder matching result and markers required".into()),
    };
    let mut output = state(st)?;
    let mut incoming: Vec<Value> = Vec::new();
    if status == "accepted" || status == "partial" {
        for (marker, record) in markers {
            name(&json!(marker), "marker", 128, false)?;
            let record = record.as_object().ok_or("Explicit ladder marker result required")?;
            if record.get("status") != Some(&json!("accepted")) {
                continue;
            }
            let offsets = record.get("bin_offsets").and_then(|o| o.as_object()).ok_or("Explicit per-bin offset mapping required")?;
            let row_code = match &marker_codes {
                Some(codes) => codes.iter().find(|(k, _)| k == marker).map_or(Value::Null, |(_, v)| v.clone()),
                None => quality_code.clone(),
            };
            for (label, value) in offsets {
                name(&json!(label), "bin label", 128, false)?;
                incoming.push(json!({"marker": marker, "key_kind": "bin", "bin_label": label, "ladder_id": ladder_id,
                    "offset_bp": fv(store_offset_bp(value)?), "quality_code": row_code}));
            }
            for (field, kind) in [("min_offset_bp", "virtual_min"), ("max_offset_bp", "virtual_max")] {
                if let Some(v) = record.get(field).filter(|v| !v.is_null()) {
                    incoming.push(json!({"marker": marker, "key_kind": kind, "bin_label": Value::Null, "ladder_id": ladder_id,
                        "offset_bp": fv(store_offset_bp(v)?), "quality_code": row_code}));
                }
            }
        }
    }
    let mut index: HashMap<Identity, usize> = HashMap::new();
    for (i, row) in contributions(&mut output).iter().enumerate() {
        index.insert(identity(row), i);
    }
    let mut replaced: Vec<String> = Vec::new();
    let supplied = incoming.len();
    for row in incoming {
        let id = identity(&row);
        let contribs = contributions(&mut output);
        if let Some(&pos) = index.get(&id) {
            replaced.push(contribs[pos]["ladder_id"].as_str().unwrap().to_string());
            contribs[pos] = row;
        } else {
            index.insert(id, contribs.len());
            contribs.push(row);
        }
    }
    let replaced_count = replaced.len();
    events(&mut output).push(json!({
        "action": "upsert_keys", "ladder_id": ladder_id, "quality_code": event_code, "matching_status": status,
        "supplied_keys": supplied, "replaced_keys": replaced_count, "replaced_ladder_ids": unique(replaced.into_iter()),
        "absent_keys_preserved": true, "identity_comparison": "ASCII_equalsIgnoreCase"}));
    Ok(Value::Object(output))
}

pub fn aggregate_run_offsets(entries: &Value, kit: &Value) -> Res<Value> {
    let definition = load_kit_definition(kit)?;
    let entries = match entries {
        Value::Array(e) if (1..=128).contains(&e.len()) => e,
        _ => return Err("Explicit list of 1..128 ladder entries required".into()),
    };
    let dmarkers = definition["markers"].as_array().unwrap();
    let mut known: HashMap<String, HashSet<String>> = HashMap::new();
    for m in dmarkers {
        known.insert(m["name"].as_str().unwrap().to_string(), m["bins"].as_array().unwrap().iter().map(|b| b["label"].as_str().unwrap().to_string()).collect());
    }
    let mut st = Value::Object(state(&Value::Null)?);
    let mut latest: HashMap<String, String> = HashMap::new();
    let mut history: Vec<Value> = Vec::new();
    for entry in entries {
        let e = match entry {
            Value::Object(e) if e.len() == 3 && ["ladder_id", "quality_code", "result"].iter().all(|k| e.contains_key(*k)) => e,
            _ => return Err("ladder_id/quality_code/result required".into()),
        };
        let identifier = name(&e["ladder_id"], "ladder_id", 256, true)?;
        let folded = lower(&identifier);
        let previous = latest.get(&folded).cloned();
        let mut withdrawn = 0u64;
        if previous.is_some() {
            st = remove_run_ladder(&st, &json!(identifier))?;
            withdrawn = st["events"].as_array().unwrap().last().unwrap()["removed_contributions"].as_u64().unwrap();
        }
        st = upsert_run_offsets(&st, &json!(identifier), &e["quality_code"], &e["result"])?;
        latest.insert(folded, identifier.clone());
        history.push(json!({
            "ladder_id": identifier, "quality_code": e["quality_code"], "matching_status": e["result"]["status"],
            "replaced_ladder_id": previous.clone().map_or(Value::Null, Value::String), "withdrawn_contributions": withdrawn,
            "case_insensitive_replacement": previous.as_ref().map_or(false, |p| p != &identifier)}));
    }
    let contribs = st["contributions"].as_array().unwrap();
    for row in contribs {
        let m = row["marker"].as_str().unwrap();
        let bad = match known.get(m) {
            None => true,
            Some(bins) => row["key_kind"] == json!("bin") && !bins.contains(row["bin_label"].as_str().unwrap()),
        };
        if bad {
            return Err("Run contribution references unknown kit marker/bin".into());
        }
    }
    let mut pools: HashMap<(String, String, Option<String>), Vec<&Value>> = HashMap::new();
    for row in contribs {
        pools.entry((row["marker"].as_str().unwrap().to_string(), row["key_kind"].as_str().unwrap().to_string(), row["bin_label"].as_str().map(String::from))).or_default().push(row);
    }
    let mean_for = |marker: &str, kind: &str, label: Option<&str>| -> Value {
        let rows: Vec<&Value> = pools.get(&(marker.to_string(), kind.to_string(), label.map(String::from))).cloned().unwrap_or_default();
        let adm: Vec<Value> = rows.iter().filter(|r| admitted(&r["quality_code"])).map(|r| (*r).clone()).collect();
        let excl: Vec<Value> = rows
            .iter()
            .filter(|r| !admitted(&r["quality_code"]))
            .map(|r| {
                let mut m = r.as_object().unwrap().clone();
                m.insert("reason".into(), json!(if r["quality_code"].is_null() { "quality_code_missing" } else { "quality_code_not_admitted" }));
                Value::Object(m)
            })
            .collect();
        let mean_v = if adm.is_empty() { Value::Null } else { fv(mean(&adm.iter().map(|r| r["offset_bp"].as_f64().unwrap()).collect::<Vec<_>>())) };
        json!({"offset_bp": mean_v, "contributors": adm, "excluded": excl})
    };
    let mut markers_out = Map::new();
    for marker in dmarkers {
        let nm = marker["name"].as_str().unwrap();
        let mut bins = Map::new();
        for b in marker["bins"].as_array().unwrap() {
            let label = b["label"].as_str().unwrap();
            bins.insert(label.to_string(), mean_for(nm, "bin", Some(label)));
        }
        let low = mean_for(nm, "virtual_min", None);
        let high = mean_for(nm, "virtual_max", None);
        let mut offsets = Map::new();
        for (label, row) in &bins {
            if !row["offset_bp"].is_null() {
                offsets.insert(label.clone(), row["offset_bp"].clone());
            }
        }
        let usable = !offsets.is_empty() && !low["offset_bp"].is_null() && !high["offset_bp"].is_null();
        let rows_all: Vec<&Value> = bins.values().chain([&low, &high]).collect();
        let sources = unique(rows_all.iter().flat_map(|r| r["contributors"].as_array().unwrap().iter().map(|c| c["ladder_id"].as_str().unwrap().to_string())));
        let missing: Vec<&String> = bins.iter().filter(|(_, r)| r["offset_bp"].is_null()).map(|(l, _)| l).collect();
        markers_out.insert(
            nm.to_string(),
            json!({
                "status": if usable { "accepted" } else { "rejected" }, "bin_offsets": Value::Object(offsets),
                "min_offset_bp": low["offset_bp"], "max_offset_bp": high["offset_bp"], "source_ladders": sources,
                "missing_bin_offsets": missing, "per_key": {"bins": Value::Object(bins), "virtual_min": low, "virtual_max": high}}),
        );
    }
    let accepted = markers_out.values().filter(|r| r["status"] == json!("accepted")).count();
    let status = if accepted == markers_out.len() { "accepted" } else if accepted > 0 { "partial" } else { "rejected" };
    let reasons: Vec<&str> = if status != "accepted" { vec!["Some markers lack admitted direct bins or virtual endpoints"] } else { vec![] };
    Ok(json!({
        "status": status, "reasons": reasons, "markers": Value::Object(markers_out), "state": st,
        "diagnostics": {
            "method": "per_key_unweighted_mean_after_fresh_filename_replacement",
            "admitted_quality_codes": [fv(0.0), fv(-2.0), fv(0.5)], "quality_codes_computed_here": false,
            "identity_comparison": "ASCII_equalsIgnoreCase", "source_contract": "IDX16_ALLELE_CALLING AL-05/AL-07",
            "entry_history": history, "external_validation": false, "original_runtime_validated": false,
            "input_rounding": "Math.round(double_value*1e9)/1e9; ties toward positive infinity",
            "mean_arithmetic": "sequential_double_sum_in_contribution_order",
            "missing_key": "None; no implicit zero or interpolation",
            "marker_usable_policy": "both_virtual_endpoints_and_any_direct_bin"}}))
}
