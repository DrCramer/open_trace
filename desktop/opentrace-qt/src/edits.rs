//! Label edits of a sample (port of the web `effective` / `pushEdit`): the analysis report stays as the core made
//! it; user edits are an ordered list (with reason, user and time) applied on top, so every change can be shown,
//! undone and audited. The edit objects have the same fields as in the web project.
use crate::backend::{Inner, Sample};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};

const MEC_ACTIONS: [&str; 5] = ["delete", "to_artifact", "add_artifact", "rename_artifact", "delete_artifact"];

#[derive(Default)]
pub struct Effective {
    pub calls: Vec<Value>,
    pub artifacts: Vec<Value>,
    pub edited: BTreeSet<String>,
    pub gq_override: BTreeSet<String>,
    pub cgq_override: bool,
    pub pli: BTreeSet<String>,
    pub reasons: BTreeMap<String, String>,
    pub mec: BTreeMap<String, String>,
}

pub fn active(s: &Sample) -> impl Iterator<Item = &Value> {
    s.edits.iter().filter(|e| !e["undone"].as_bool().unwrap_or(false))
}

fn id_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Calls and artifacts after the active edits (calls sorted by size), as the tables and the plot window show them.
pub fn effective(s: &Sample) -> Effective {
    let mut out = Effective::default();
    let Some(rep) = &s.report else { return out };
    let mut calls: Vec<Value> = rep["calls"].as_array().cloned().unwrap_or_default();
    let mut arts: Vec<(String, Value)> = Vec::new();
    let pos = |calls: &Vec<Value>, id: &str| calls.iter().position(|c| id_of(&c["id"]) == id);
    for e in active(s) {
        let action = e["action"].as_str().unwrap_or("");
        let marker = e["marker"].as_str().unwrap_or("").to_string();
        let eid = id_of(&e["id"]);
        if action == "override_cgq" {
            out.cgq_override = true;
            continue;
        }
        if action == "pli" {
            if e["new"] == "on" {
                out.pli.insert(marker);
            } else {
                out.pli.remove(&marker);
            }
            continue;
        }
        out.edited.insert(marker.clone());
        let reason = e["reason"].as_str().unwrap_or("").to_string();
        if MEC_ACTIONS.contains(&action) {
            out.mec.insert(marker.clone(), reason);
        } else if action != "override_gq" {
            out.reasons.insert(marker.clone(), reason);
        }
        let call_id = id_of(&e["callId"]);
        let art_id = id_of(&e["artifactId"]);
        match action {
            "override_gq" => {
                out.gq_override.insert(marker.clone());
            }
            "rename" => {
                if let Some(i) = pos(&calls, &call_id) {
                    calls[i]["label"] = e["new"].clone();
                    calls[i]["user_edited"] = json!(true);
                }
            }
            "delete" => {
                if let Some(i) = pos(&calls, &call_id) {
                    calls.remove(i);
                }
            }
            "add" => {
                let mut c = e["peak"].as_object().cloned().unwrap_or_default();
                c.insert("id".into(), json!(format!("u{eid}")));
                c.insert("marker".into(), json!(marker));
                c.insert("label".into(), e["new"].clone());
                c.insert("flags".into(), json!(["user_added"]));
                c.insert("user_edited".into(), json!(true));
                calls.push(Value::Object(c));
            }
            "move" => {
                if let Some(i) = pos(&calls, &call_id) {
                    if let (Some(c), Some(p)) = (calls[i].as_object_mut(), e["peak"].as_object()) {
                        for (k, v) in p {
                            c.insert(k.clone(), v.clone());
                        }
                        c.insert("label".into(), e["new"].clone());
                        c.insert("user_edited".into(), json!(true));
                    }
                }
            }
            "to_artifact" => {
                if let Some(i) = pos(&calls, &call_id) {
                    let c = calls.remove(i);
                    arts.push((
                        format!("a{eid}"),
                        json!({"id": format!("a{eid}"), "channel": c["channel"], "size_bp": c["size_bp"], "height": c["height"], "scan": c["scan"],
                               "marker": c["marker"], "label": e["new"], "from": c["id"], "orig": c["label"]}),
                    ));
                }
            }
            "add_artifact" => {
                let mut a = e["peak"].as_object().cloned().unwrap_or_default();
                a.insert("id".into(), json!(format!("a{eid}")));
                a.insert("marker".into(), json!(marker));
                a.insert("label".into(), e["new"].clone());
                arts.retain(|(k, _)| *k != format!("a{eid}"));
                arts.push((format!("a{eid}"), Value::Object(a)));
            }
            "rename_artifact" => {
                if let Some((_, a)) = arts.iter_mut().find(|(k, _)| *k == art_id) {
                    a["label"] = e["new"].clone();
                }
            }
            "delete_artifact" => arts.retain(|(k, _)| *k != art_id),
            "artifact_to_allele" => {
                if let Some(p) = arts.iter().position(|(k, _)| *k == art_id) {
                    let (_, a) = arts.remove(p);
                    calls.push(json!({"channel": a["channel"], "size_bp": a["size_bp"], "height": a["height"], "scan": a["scan"], "id": format!("u{eid}"),
                                      "marker": marker, "label": e["new"], "flags": ["user_added"], "user_edited": true}));
                }
            }
            _ => {}
        }
    }
    calls.sort_by(|a, b| a["size_bp"].as_f64().unwrap_or(0.0).total_cmp(&b["size_bp"].as_f64().unwrap_or(0.0)));
    out.calls = calls;
    out.artifacts = arts.into_iter().map(|(_, a)| a).collect();
    out
}

pub fn now_iso() -> String {
    // UTC ISO-8601 like JavaScript Date.toISOString (no external crate)
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs() as i64;
    let ms = d.subsec_millis();
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (h, m, s) = (rem / 3600, rem % 3600 / 60, rem % 60);
    // civil from days (Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}.{ms:03}Z")
}

impl Inner {
    /// Adds one edit (spec: action, marker, callId / artifactId, peak, new) with its reason; `pushEdit` of the web UI.
    pub fn apply_edit(&self, i: usize, spec: &Value, reason: &str, user: &str) -> Result<(), String> {
        if reason.chars().count() > 500 {
            return Err("reason too long".into());
        }
        let mut list = self.samples.lock().unwrap();
        let s = list.get_mut(i).ok_or("no sample")?;
        if s.report.is_none() {
            return Err("the sample has no genotyping result".into());
        }
        let action = spec["action"].as_str().unwrap_or("");
        const ACTIONS: [&str; 12] = ["rename", "delete", "add", "move", "to_artifact", "add_artifact", "rename_artifact", "delete_artifact", "artifact_to_allele", "override_gq", "override_cgq", "pli"];
        if !ACTIONS.contains(&action) {
            return Err(format!("unknown edit {action}"));
        }
        let label = spec["new"].clone();
        if let Some(l) = label.as_str() {
            if l.trim().is_empty() && action != "pli" || l.chars().count() > 40 {
                return Err("label must be 1..40 characters".into());
            }
        }
        let eff = effective(s);
        let call_id = id_of(&spec["callId"]);
        let art_id = id_of(&spec["artifactId"]);
        let old = if !call_id.is_empty() {
            eff.calls.iter().find(|c| id_of(&c["id"]) == call_id).cloned()
        } else if !art_id.is_empty() {
            eff.artifacts.iter().find(|a| id_of(&a["id"]) == art_id).cloned()
        } else {
            None
        };
        let id = s.edits.iter().filter_map(|e| e["id"].as_i64()).max().unwrap_or(0) + 1;
        let gone = action == "delete" || action == "delete_artifact";
        s.edits.push(json!({
            "id": id, "at": now_iso(), "user": user, "reason": reason, "action": action,
            "marker": spec.get("marker").filter(|m| !m.is_null()).cloned().unwrap_or(Value::Null),
            "callId": if call_id.is_empty() { Value::Null } else { spec["callId"].clone() },
            "artifactId": if art_id.is_empty() { Value::Null } else { spec["artifactId"].clone() },
            "peak": spec.get("peak").cloned().unwrap_or(Value::Null),
            "old": old.as_ref().map(|o| o["label"].clone()).unwrap_or(Value::Null),
            "new": if gone { Value::Null } else { label },
            "size_bp": old.as_ref().map(|o| o["size_bp"].clone()).or_else(|| spec["peak"].get("size_bp").cloned()).unwrap_or(Value::Null),
        }));
        Ok(())
    }

    /// Undo of the latest active edit of the given samples (plot window Edit > Undo).
    pub fn undo_last(&self, rows: &[usize]) -> Option<String> {
        let mut list = self.samples.lock().unwrap();
        let mut best: Option<(usize, usize, String)> = None;
        for &i in rows {
            let Some(s) = list.get(i) else { continue };
            for (k, e) in s.edits.iter().enumerate() {
                if e["undone"].as_bool().unwrap_or(false) {
                    continue;
                }
                let at = e["at"].as_str().unwrap_or("").to_string();
                if best.as_ref().map_or(true, |b| at >= b.2) {
                    best = Some((i, k, at));
                }
            }
        }
        let (i, k, _) = best?;
        let e = &mut list[i].edits[k];
        e["undone"] = json!(true);
        e["undoneAt"] = json!(now_iso());
        Some(format!("{} {}", e["action"].as_str().unwrap_or(""), e["marker"].as_str().unwrap_or("")))
    }

    /// Every edit of the project (newest first) for the Label Edit Viewer: sample index, edit index and the fields.
    pub fn all_edits(&self) -> Vec<(usize, usize, Value, String, String)> {
        let list = self.samples.lock().unwrap();
        let mut out: Vec<(usize, usize, Value, String, String)> = Vec::new();
        for (i, s) in list.iter().enumerate() {
            for (k, e) in s.edits.iter().enumerate() {
                out.push((i, k, e.clone(), s.file.clone(), s.name.clone()));
            }
        }
        out.sort_by(|a, b| b.2["at"].as_str().unwrap_or("").cmp(a.2["at"].as_str().unwrap_or("")));
        out
    }

    pub fn undo_edit(&self, i: usize, k: usize) -> bool {
        let mut list = self.samples.lock().unwrap();
        let Some(e) = list.get_mut(i).and_then(|s| s.edits.get_mut(k)) else { return false };
        if e["undone"].as_bool().unwrap_or(false) {
            return false;
        }
        e["undone"] = json!(true);
        e["undoneAt"] = json!(now_iso());
        true
    }

    /// Re-analysis drops the label edits of these samples (marked undone by "reanalysis", kept for the record).
    pub fn drop_edits_for_reanalysis(&self, rows: &[usize]) {
        let mut list = self.samples.lock().unwrap();
        for &i in rows {
            if let Some(s) = list.get_mut(i) {
                for e in s.edits.iter_mut().filter(|e| !e["undone"].as_bool().unwrap_or(false)) {
                    e["undone"] = json!(true);
                    e["undoneAt"] = json!(now_iso());
                    e["undoneBy"] = json!("reanalysis");
                }
            }
        }
    }
}

#[allow(dead_code)]
pub fn obj() -> Map<String, Value> {
    Map::new()
}
#[allow(dead_code)]
pub type Ids = HashMap<String, usize>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn sample() -> Sample {
        Sample {
            report: Some(Arc::new(json!({"calls": [
                {"id": "c1", "marker": "TH01", "label": "6", "size_bp": 160.0, "channel": "DATA1"},
                {"id": "c2", "marker": "TH01", "label": "OL", "size_bp": 163.0, "channel": "DATA1"},
                {"id": "c3", "marker": "TH01", "label": "9.3", "size_bp": 175.0, "channel": "DATA1"}]}))),
            ..Default::default()
        }
    }

    #[test]
    fn edits_apply_in_order_and_undo() {
        let mut s = sample();
        s.edits.push(json!({"id": 1, "action": "delete", "marker": "TH01", "callId": "c2", "reason": "stutter"}));
        s.edits.push(json!({"id": 2, "action": "rename", "marker": "TH01", "callId": "c3", "new": "9", "reason": "r"}));
        s.edits.push(json!({"id": 3, "action": "add", "marker": "TH01", "peak": {"size_bp": 150.0, "channel": "DATA1"}, "new": "5", "reason": "r"}));
        let e = effective(&s);
        let labels: Vec<String> = e.calls.iter().map(|c| id_of(&c["label"])).collect();
        assert_eq!(labels, ["5", "6", "9"]);
        assert_eq!(e.mec.get("TH01").map(String::as_str), Some("stutter"));
        s.edits[0]["undone"] = json!(true);
        assert_eq!(effective(&s).calls.len(), 4);
    }

    #[test]
    fn iso_time_shape() {
        let t = now_iso();
        assert_eq!(t.len(), 24);
        assert!(t.ends_with('Z') && t.as_bytes()[10] == b'T');
    }
}
