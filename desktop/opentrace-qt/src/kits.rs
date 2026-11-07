//! Panel Manager store (port of the web `kits.js`): kits imported from GeneMapper panels / bins / stutter text files.
//! The parsed structures (as returned by the core's `import-gm-kit`) are the source of truth; GeneMapper text is
//! regenerated from them with the same writers as the web UI, so the analysis kit the core builds from them is the
//! same in both interfaces.
use serde_json::{json, Map, Value};

#[derive(Default, Clone)]
pub struct KitDb {
    pub kits: Vec<Value>, // {name, type, comment, panelsVersion, panels:[{name, comment, markers}], binSets:[{name, version, panels}], stutter}
}

/// JavaScript `String(number)` for the numbers of the kit files (integers without ".0", others shortest form).
pub fn js_num(v: &Value) -> String {
    match v {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                return i.to_string();
            }
            let f = n.as_f64().unwrap_or(0.0);
            if f.fract() == 0.0 && f.abs() < 1e21 {
                format!("{}", f as i64)
            } else {
                let s = format!("{}", f);
                s
            }
        }
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".into(),
        other => other.to_string(),
    }
}

fn num(v: &Value) -> String {
    let s = js_num(v);
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s
    } else {
        s + ".0"
    }
}

fn strs(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().map(js_num).collect()).unwrap_or_default()
}

fn list(a: &[String], trailing: bool) -> String {
    if a.is_empty() {
        "none".into()
    } else if trailing {
        a.join(", ") + ", "
    } else {
        a.join(",")
    }
}

fn s_or<'a>(v: &'a Value, d: &'a str) -> &'a str {
    match v.as_str() {
        Some(s) if !s.is_empty() => s,
        _ => d,
    }
}

pub fn panels_text(kit: &Value) -> String {
    let panels = kit["panels"].as_array().cloned().unwrap_or_default();
    let any_qc = panels.iter().any(|p| p["markers"].as_array().is_some_and(|ms| ms.iter().any(|m| m.get("internal_qc").is_some())));
    let mut rows: Vec<Vec<String>> = vec![
        vec!["Version".into(), s_or(&kit["panelsVersion"], "GMID-X v 1.4").into()],
        vec!["Kit type:".into(), s_or(&kit["type"], "MICROSATELLITE").into()],
        vec!["Chemistry Kit".into(), kit["name"].as_str().unwrap_or("").into(), s_or(&kit["comment"], "null").into()],
    ];
    for p in &panels {
        rows.push(vec!["Panel".into(), p["name"].as_str().unwrap_or("").into(), s_or(&p["comment"], "null").into()]);
        for m in p["markers"].as_array().into_iter().flatten() {
            let ctrl = strs(&m["control_alleles"]);
            let mut r = vec![
                m["name"].as_str().unwrap_or("").to_string(),
                js_num(&m["dye"]),
                num(&m["min_bp"]),
                num(&m["max_bp"]),
                if ctrl.is_empty() { "none".into() } else { ctrl.join(",") },
                js_num(&m["repeat"]),
                s_or(&m["option"], "none").to_string(),
                (m["y_marker"].as_bool().unwrap_or(false)).to_string(),
            ];
            if any_qc {
                r.push(m["internal_qc"].as_bool().unwrap_or(false).to_string());
            }
            r.push(list(&strs(&m["ladder_alleles"]), true));
            rows.push(r);
        }
    }
    rows.iter().map(|r| r.join("\t")).collect::<Vec<_>>().join("\r\n") + "\r\n"
}

pub fn bins_text(kit: &Value, set: &Value) -> String {
    let mut rows: Vec<Vec<String>> = vec![
        vec!["Version".into(), s_or(&set["version"], "GMID-X v 1.2").into()],
        vec!["Chemistry Kit".into(), kit["name"].as_str().unwrap_or("").into()],
        vec!["BinSet Name".into(), set["name"].as_str().unwrap_or("").into()],
    ];
    for (panel, markers) in set["panels"].as_object().into_iter().flatten() {
        rows.push(vec!["Panel Name".into(), panel.clone()]);
        for (marker, bins) in markers.as_object().into_iter().flatten() {
            rows.push(vec!["Marker Name".into(), marker.clone()]);
            for b in bins.as_array().into_iter().flatten() {
                let mut r = vec![js_num(&b["label"]), js_num(&b["center_bp"]), js_num(&b["left_bp"]), js_num(&b["right_bp"])];
                if b["virtual"].as_bool().unwrap_or(false) {
                    r.push("virtual".into());
                }
                rows.push(r);
            }
        }
    }
    rows.iter().map(|r| r.join("\t")).collect::<Vec<_>>().join("\r\n") + "\r\n"
}

pub fn stutter_text(kit: &Value) -> Option<String> {
    let st = kit.get("stutter").filter(|s| s.is_object())?;
    let mut rows: Vec<Vec<String>> = vec![
        vec!["Version".into(), s_or(&st["version"], "SSFv1.2").into()],
        vec!["Chemistry Kit".into(), kit["name"].as_str().unwrap_or("").into()],
    ];
    for (panel, markers) in st["panels"].as_object().into_iter().flatten() {
        rows.push(vec!["Panel Name".into(), panel.clone()]);
        for (marker, items) in markers.as_object().into_iter().flatten() {
            rows.push(vec!["Marker Name".into(), marker.clone()]);
            for s in items.as_array().into_iter().flatten() {
                rows.push(vec![
                    js_num(&s["ratio"]),
                    js_num(&s["min_bp"]),
                    js_num(&s["max_bp"]),
                    if s["direction"] == "plus" { "Plus".into() } else { "Minus".into() },
                    String::new(),
                ]);
            }
        }
    }
    Some(rows.iter().map(|r| r.join("\t")).collect::<Vec<_>>().join("\r\n") + "\r\n")
}

/// Choice of a kit panel: key = kit + U+0001 + panel (as the web UI stores it in the project).
pub struct PanelChoice {
    pub key: String,
    pub kit: String,
    pub panel: String,
    pub bin_sets: Vec<String>,
}

/// What the core needs to build the analysis kit of a panel (`import-gm-kit` request).
pub struct KitRequest {
    pub panels: String,
    pub bins: String,
    pub stutter: Option<String>,
    pub panel: String,
    pub bin_set: String,
    pub bin_set_substituted: bool,
}

impl KitDb {
    pub fn from_json(v: &Value) -> KitDb {
        if v["schema"] != 1 {
            return KitDb::default();
        }
        KitDb { kits: v["kits"].as_array().cloned().unwrap_or_default() }
    }

    pub fn to_json(&self) -> Value {
        json!({"schema": 1, "kits": self.kits})
    }

    fn kit_mut(&mut self, name: &str) -> Option<&mut Value> {
        self.kits.iter_mut().find(|k| k["name"] == name)
    }

    /// Merges a parsed panels document (`import-gm-kit` -> `panels`); returns the panel names added or replaced.
    pub fn add_panels(&mut self, doc: &Value) -> Vec<String> {
        let mut added = Vec::new();
        for k in doc["kits"].as_array().into_iter().flatten() {
            let name = k["name"].as_str().unwrap_or("").to_string();
            if self.kit_mut(&name).is_none() {
                self.kits.push(json!({"name": name, "type": doc["kit_type"].as_str().unwrap_or("MICROSATELLITE"), "comment": "",
                    "panelsVersion": doc["version"], "panels": [], "binSets": [], "stutter": null}));
            }
            let kit = self.kit_mut(&name).unwrap();
            for p in k["panels"].as_array().into_iter().flatten() {
                let copy = json!({"name": p["name"], "comment": "", "markers": p["markers"]});
                let panels = kit["panels"].as_array_mut().unwrap();
                match panels.iter().position(|x| x["name"] == p["name"]) {
                    Some(i) => panels[i] = copy,
                    None => panels.push(copy),
                }
                added.push(p["name"].as_str().unwrap_or("").to_string());
            }
            kit["panelsVersion"] = doc["version"].clone();
        }
        added
    }

    /// Bin set of a kit (the document's kit when it exists, else `kit_name`); returns (kit, bin set).
    pub fn add_bin_set(&mut self, kit_name: &str, doc: &Value) -> Result<(String, String), String> {
        let doc_kit = doc["kit"].as_str().unwrap_or("");
        let target = if !doc_kit.is_empty() && self.kits.iter().any(|k| k["name"] == doc_kit) { doc_kit.to_string() } else { kit_name.to_string() };
        let kit = self.kit_mut(&target).ok_or("import the panels of the kit first")?;
        let name = doc["bin_set"].as_str().map(String::from).unwrap_or_else(|| format!("{}_Bins", target));
        let set = json!({"name": name, "version": doc["version"], "panels": doc["panels"]});
        let sets = kit["binSets"].as_array_mut().unwrap();
        match sets.iter().position(|x| x["name"] == name.as_str()) {
            Some(i) => sets[i] = set,
            None => sets.push(set),
        }
        Ok((target, name))
    }

    pub fn set_stutter(&mut self, kit_name: &str, doc: &Value) -> Result<String, String> {
        let kit = self.kit_mut(kit_name).ok_or("import the panels of the kit first")?;
        kit["stutter"] = json!({"version": doc["version"], "panels": doc["panels"]});
        Ok(kit_name.to_string())
    }

    pub fn remove_kit(&mut self, name: &str) {
        self.kits.retain(|k| k["name"] != name);
    }

    pub fn choices(&self) -> Vec<PanelChoice> {
        let mut out = Vec::new();
        for kit in &self.kits {
            let kname = kit["name"].as_str().unwrap_or("");
            for p in kit["panels"].as_array().into_iter().flatten() {
                let pname = p["name"].as_str().unwrap_or("");
                let bin_sets = kit["binSets"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|s| s["panels"].get(pname).is_some())
                    .filter_map(|s| s["name"].as_str().map(String::from))
                    .collect();
                out.push(PanelChoice { key: format!("{kname}\u{1}{pname}"), kit: kname.into(), panel: pname.into(), bin_sets });
            }
        }
        out
    }

    /// Bin set: the analysis method's bin set when the kit has it for this panel, else the first one (web `request`).
    pub fn request(&self, key: &str, bin_set_name: &str) -> Option<KitRequest> {
        let (kit_name, panel) = key.split_once('\u{1}')?;
        let kit = self.kits.iter().find(|k| k["name"] == kit_name)?;
        if !kit["panels"].as_array()?.iter().any(|p| p["name"] == panel) {
            return None;
        }
        let usable: Vec<&Value> = kit["binSets"].as_array()?.iter().filter(|s| s["panels"].get(panel).is_some()).collect();
        let set = usable.iter().find(|s| s["name"] == bin_set_name).or(usable.first())?;
        let set_name = set["name"].as_str().unwrap_or("").to_string();
        Some(KitRequest {
            panels: panels_text(kit),
            bins: bins_text(kit, set),
            stutter: stutter_text(kit),
            panel: panel.to_string(),
            bin_set_substituted: !bin_set_name.is_empty() && set_name != bin_set_name && usable.len() > 1,
            bin_set: set_name,
        })
    }

    /// One-line descriptions for the Panel Manager list.
    pub fn summary(&self) -> Vec<String> {
        let mut out = Vec::new();
        for kit in &self.kits {
            let panels: Vec<String> = kit["panels"].as_array().into_iter().flatten().map(|p| format!("{} ({} маркеров)", p["name"].as_str().unwrap_or(""), p["markers"].as_array().map(|m| m.len()).unwrap_or(0))).collect();
            let sets: Vec<String> = kit["binSets"].as_array().into_iter().flatten().filter_map(|s| s["name"].as_str().map(String::from)).collect();
            out.push(format!(
                "{}\t{}\t{}\t{}",
                kit["name"].as_str().unwrap_or(""),
                panels.join("; "),
                if sets.is_empty() { "—".into() } else { sets.join("; ") },
                if kit["stutter"].is_object() { "есть" } else { "—" }
            ));
        }
        out
    }
}

#[allow(dead_code)]
pub fn empty_obj() -> Map<String, Value> {
    Map::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_like_javascript() {
        assert_eq!(js_num(&json!(100.0)), "100");
        assert_eq!(js_num(&json!(100.5)), "100.5");
        assert_eq!(num(&json!(100)), "100.0");
        assert_eq!(num(&json!(0.25)), "0.25");
        assert_eq!(list(&["1".into(), "2".into()], true), "1, 2, ");
        assert_eq!(list(&[], true), "none");
    }
}
