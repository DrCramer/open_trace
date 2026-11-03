//! Port of `genemapper_open/project_api.py`: stateless browser project actions, validated in SQLite memory only.
use crate::project_runs::{change_run_ladder, create_project_run, revise_project_run};
use crate::project_store::{iso_parse, se, ProjectStore, Review, R};
use crate::pullup_quality::{py_eq, truthy};
use serde_json::{json, Map, Value};

const RUN_ACTIONS: [&str; 5] = ["run-create", "run-revise", "run-replace-ladder", "run-remove-ladder", "run-select"];

fn get(m: &Map<String, Value>, k: &str) -> Value {
    m.get(k).cloned().unwrap_or(Value::Null)
}

pub fn project_action(request: &Value) -> R<Value> {
    let req = match request {
        Value::Object(m) => m,
        _ => return Err(se("Project request must be an object")),
    };
    let action_v = get(req, "action");
    let action = match action_v.as_str() {
        Some(a) if ["new", "import", "save", "select", "review", "undo"].contains(&a) || RUN_ACTIONS.contains(&a) => a.to_string(),
        _ => return Err(se("Unknown project action")),
    };
    let mut store = if action == "new" { ProjectStore::memory(&get(req, "name"))? } else { ProjectStore::from_json(&get(req, "document"))? };
    if RUN_ACTIONS.contains(&action.as_str()) && store.project()?["schema_version"] == json!(1) {
        store = store.upgraded()?;
    }
    let mut selected: Value = if action == "new" || action == "import" { Value::Null } else { get(req, "analysis_id") };
    let mut selected_run = get(req, "run_id");
    let mut selected_revision = get(req, "revision_id");
    let mut change = Value::Null;
    if action == "save" {
        let report = match req.get("report") {
            Some(Value::Object(r)) if py_eq(&get(r, "schema_version"), &json!(1)) => r,
            _ => return Err(se("A version1 compact STR workflow report is required")),
        };
        let status = get(report, "status");
        if !matches!(status.as_str(), Some("tentative") | Some("partial") | Some("rejected")) {
            return Err(se("STR workflow status must remain tentative, partial or rejected"));
        }
        let fields = [
            "schema_version", "source_sha256", "analysis_version", "status", "reasons", "calls", "peaks", "kit_identity",
            "sample_identity", "ladder_identity", "sizing", "ladder", "allele_calling", "quality", "provenance", "research_only",
        ];
        let mut result = Map::new();
        for k in fields {
            if let Some(v) = report.get(k) {
                result.insert(k.into(), v.clone());
            }
        }
        let source = get(report, "source_sha256");
        let samples = store.list_samples()?;
        let found = samples.iter().find(|s| s["source_sha256"] == source).map(|s| s["sample_id"].clone());
        let identifier = match found {
            Some(i) => i,
            None => json!(store.add_sample(&source, &json!({"kind": "research STR sample"}), &Value::Null, &Value::Null)?),
        };
        let id = store.add_analysis(
            &identifier,
            &source,
            &json!({"name": "OpenTrace STR", "version": get(report, "analysis_version")}),
            &get(report, "settings"),
            &Value::Object(result),
            &status,
            &Value::Null,
            &Value::Null,
        )?;
        selected = json!(id);
    } else if action == "review" {
        let (ra, actor, reason, tid, val) = (get(req, "review_action"), get(req, "actor"), get(req, "reason"), get(req, "target_id"), get(req, "value"));
        let mut rv = Review::new(&selected, &ra, &actor, &reason);
        rv.target_kind = req.get("target_kind").cloned().unwrap_or(json!("analysis"));
        rv.target_id = &tid;
        rv.value = &val;
        store.append_review(&rv)?;
    } else if action == "undo" {
        store.undo_review(&selected, &get(req, "event_id"), &get(req, "actor"), &get(req, "reason"))?;
    } else if RUN_ACTIONS.contains(&action.as_str()) {
        match action.as_str() {
            "run-create" => change = create_project_run(&store, &get(req, "name"), &get(req, "inputs"), &get(req, "actor"), &get(req, "reason"))?,
            "run-revise" => {
                change = revise_project_run(&store, &selected_run, &get(req, "parent_revision_id"), &get(req, "inputs"), &get(req, "actor"), &get(req, "reason"))?
            }
            "run-replace-ladder" | "run-remove-ladder" => {
                change = change_run_ladder(
                    &store,
                    &selected_run,
                    &get(req, "parent_revision_id"),
                    &get(req, "ladder_id"),
                    &get(req, "analysis"),
                    &json!(action == "run-remove-ladder"),
                    &get(req, "actor"),
                    &get(req, "reason"),
                )?
            }
            _ => {}
        }
        if truthy(&change) {
            selected_run = change["run_id"].clone();
            selected_revision = change["revision_id"].clone();
            selected = match change["outputs"].as_array() {
                Some(o) if !o.is_empty() => o[0]["analysis_id"].clone(),
                _ => Value::Null,
            };
        }
    }
    let runs = store.list_runs()?;
    if selected_run.is_null() && !runs.is_empty() {
        selected_run = runs[0]["run_id"].clone();
    }
    let revisions = if truthy(&selected_run) { store.list_run_revisions(&selected_run)? } else { vec![] };
    let revision = if truthy(&selected_revision) {
        store.get_run_revision(&selected_revision)?
    } else if let Some(l) = revisions.last() {
        l.clone()
    } else {
        Value::Null
    };
    if !revision.is_null() && revision["run_id"] != selected_run {
        return Err(se("Selected revision belongs to another run"));
    }
    if action == "run-select" {
        selected = match revision.get("outputs").and_then(Value::as_array) {
            Some(o) if !o.is_empty() => o[0]["analysis_id"].clone(),
            _ => Value::Null,
        };
    }
    let mut analyses = store.list_analyses()?;
    let mut keyed: Vec<(i128, String, Value)> = Vec::new();
    for a in analyses.drain(..) {
        let ts = a["created_at"].as_str().unwrap_or("").replace('Z', "+00:00");
        let t = iso_parse(&ts).map_err(|_| se("Invalid isoformat string"))?.ok_or_else(|| se("naive datetime"))?;
        keyed.push((t, a["analysis_id"].as_str().unwrap_or("").to_string(), a));
    }
    keyed.sort_by(|x, y| (y.0, &y.1).cmp(&(x.0, &x.1)));
    let analyses: Vec<Value> = keyed.into_iter().map(|k| k.2).collect();
    if selected.is_null() && !analyses.is_empty() {
        selected = analyses[0]["analysis_id"].clone();
    }
    let summary: Vec<Value> = analyses
        .iter()
        .map(|a| json!({"analysis_id": a["analysis_id"], "sample_id": a["sample_id"], "status": a["status"], "created_at": a["created_at"], "result_sha256": a["result_sha256"]}))
        .collect();
    let (effective, events) = if truthy(&selected) {
        (store.effective_analysis(&selected)?, Value::Array(store.review_events(&selected)?))
    } else {
        (Value::Null, json!([]))
    };
    Ok(json!({
        "document": store.export_json()?, "project": Value::Object(store.project()?), "analyses": summary,
        "selected_analysis_id": selected, "effective": effective, "events": events, "runs": runs,
        "selected_run_id": selected_run, "run_revisions": revisions, "selected_run_revision": revision, "run_change": change,
    }))
}
