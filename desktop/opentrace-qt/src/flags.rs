//! Process quality values of the Samples and Genotypes tables (port of the web `sampleFlags`, `genoRows`,
//! `mixFlag`, `accFlag`, `omrPeaks`, `ccFlag`): pass / check / low / override / na per sample and per marker.
use crate::backend::Sample;
use crate::edits::{active, effective, Effective};
use crate::genotype::{code, sample_pqv};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

pub type Flags = BTreeMap<&'static str, &'static str>;

pub struct MarkerRow {
    pub marker: String,
    pub channel: String,
    pub calls: Vec<Value>,
    pub flags: Flags,
    pub gq: Option<f64>,
    pub reason: String,
    pub mec: String,
}

fn is_amel(n: &str) -> bool {
    n.eq_ignore_ascii_case("AMEL") || n.eq_ignore_ascii_case("Amelogenin")
}

fn cc_flag(rep: &Value, marker: &str) -> &'static str {
    let row = rep["quality"]["controls"]["markers"].as_array().and_then(|a| a.iter().find(|x| x["marker"] == marker));
    match row {
        Some(r) if r["comparison_performed"].as_bool().unwrap_or(false) => {
            if r["cc"].as_i64() == Some(0) || r["cc"].as_f64() == Some(0.0) {
                "pass"
            } else {
                "low"
            }
        }
        _ => "na",
    }
}

fn label_str(v: &Value) -> String {
    crate::kits::js_num(v)
}

/// ACC: Amelogenin cross check, one flag for AMEL and every Y marker of the panel.
fn acc_flag(s: &Sample, eff: &Effective, kit: Option<&Value>) -> &'static str {
    if s.report.is_none() || s.stype == "ladder" || s.stype == "negative" {
        return "na";
    }
    let Some(kit) = kit else { return "na" };
    let markers = kit["markers"].as_array().cloned().unwrap_or_default();
    let Some(amel) = markers.iter().find(|m| is_amel(m["name"].as_str().unwrap_or(""))) else { return "na" };
    let ys: Vec<&Value> = markers.iter().filter(|m| m["y_marker"].as_bool().unwrap_or(false) && !is_amel(m["name"].as_str().unwrap_or(""))).collect();
    if ys.is_empty() {
        return "na";
    }
    let amel_name = amel["name"].as_str().unwrap_or("");
    let mut names: Vec<&str> = vec![amel_name];
    names.extend(ys.iter().map(|m| m["name"].as_str().unwrap_or("")));
    if names.iter().any(|n| eff.edited.contains(*n)) {
        return "na";
    }
    let mut amel_labels: Vec<String> = eff.calls.iter().filter(|c| c["marker"] == amel_name && truthy_label(&c["label"])).map(|c| label_str(&c["label"]).to_uppercase()).collect();
    amel_labels.sort();
    let amel_labels = amel_labels.join(",");
    let y_has: Vec<bool> = ys.iter().map(|m| eff.calls.iter().any(|c| c["marker"] == m["name"] && truthy_label(&c["label"]))).collect();
    if amel_labels == "X,Y" {
        return if y_has.iter().all(|b| *b) { "pass" } else { "check" };
    }
    if amel_labels == "X" || amel_labels == "X,X" {
        return if y_has.iter().any(|b| *b) { "check" } else { "pass" };
    }
    "check"
}

fn truthy_label(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::String(s) => !s.is_empty(),
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::Bool(b) => *b,
        _ => true,
    }
}

/// Genotypes table rows (one per marker of the allele calling) with their PQV flags.
pub fn marker_rows(s: &Sample, kit: Option<&Value>, num_alleles: usize) -> Vec<MarkerRow> {
    let Some(rep) = &s.report else { return Vec::new() };
    if rep["allele_calling"].is_null() {
        return Vec::new();
    }
    let eff = effective(s);
    let gq: HashMap<String, &Value> = rep["quality"]["marker_gq"]["markers"].as_array().into_iter().flatten().filter_map(|m| Some((m["marker"].as_str()?.to_string(), m))).collect();
    let acc = acc_flag(s, &eff, kit);
    let kit_m: HashMap<String, &Value> = kit.and_then(|k| k["markers"].as_array()).into_iter().flatten().filter_map(|m| Some((m["name"].as_str()?.to_string(), m))).collect();
    let markers: Vec<Value> = match &rep["allele_calling"]["markers"] {
        Value::Array(a) => a.clone(),
        Value::Object(o) => o.values().cloned().collect(),
        _ => Vec::new(),
    };
    markers
        .iter()
        .map(|m| {
            let name = m["name"].as_str().unwrap_or("").to_string();
            let calls: Vec<Value> = eff.calls.iter().filter(|c| c["marker"] == name.as_str()).cloned().collect();
            let g = gq.get(&name);
            let metrics = g.map(|g| &g["score"]["metrics"]);
            let f = |k: &str| -> &'static str {
                match metrics.map(|mm| &mm[k]) {
                    Some(v) if v.as_f64() == Some(1.0) => "check",
                    Some(v) if v.as_f64() == Some(0.0) => "pass",
                    _ => "na",
                }
            };
            let mut fl = Flags::new();
            fl.insert("OS", f("offscale"));
            fl.insert("BIN", f("binning"));
            fl.insert("PHR", f("ratio"));
            fl.insert("LPH", f("absolute_height"));
            fl.insert("MPH", f("high_height"));
            fl.insert("SPK", f("spike"));
            fl.insert("AN", f("allele_number"));
            fl.insert("BD", f("broad"));
            fl.insert("OVL", f("overlap"));
            let y_marker = kit_m.get(&name).is_some_and(|k| k["y_marker"].as_bool().unwrap_or(false));
            fl.insert("ACC", if is_amel(&name) || y_marker { acc } else { "na" });
            fl.insert("CC", cc_flag(rep, &name));
            fl.insert("ADO", if s.stype == "ladder" { "na" } else if calls.iter().filter(|c| truthy_label(&c["label"])).count() > num_alleles { "check" } else { "pass" });
            fl.insert("PLI", if eff.pli.contains(&name) { "check" } else { "na" });
            fl.insert("ME", if eff.edited.contains(&name) { "check" } else { "pass" });
            fl.insert(
                "GQ",
                if eff.gq_override.contains(&name) {
                    "override"
                } else {
                    g.map(|g| code(&g["category"]["code"])).unwrap_or("na")
                },
            );
            MarkerRow {
                channel: m["channel"].as_str().unwrap_or("").to_string(),
                gq: if eff.gq_override.contains(&name) { Some(1.0) } else { g.and_then(|g| g["score"]["gq"].as_f64()) },
                reason: eff.reasons.get(&name).cloned().unwrap_or_default(),
                mec: eff.mec.get(&name).cloned().unwrap_or_default(),
                marker: name,
                calls,
                flags: fl,
            }
        })
        .collect()
}

/// OMR: peaks between two marker ranges of a dye above the method's tetranucleotide global cut-off.
pub fn omr_peaks(s: &Sample, kit: Option<&Value>, method: Option<&Value>) -> Vec<Value> {
    let (Some(rep), Some(kit)) = (&s.report, kit) else { return Vec::new() };
    if s.stype == "ladder" {
        return Vec::new();
    }
    let cut = method.and_then(|m| m["allele"]["repeats"]["4"]["cutoff"].as_f64()).unwrap_or(0.0);
    let peaks: Vec<&Value> = rep["peaks"].as_array().into_iter().flatten().filter(|p| p["size_bp"].is_number() && !truthy_label(&p["spike"])).collect();
    let mut by_ch: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    for m in kit["markers"].as_array().into_iter().flatten() {
        let ch = m["channel"].as_str().unwrap_or("").to_string();
        if !order.contains(&ch) {
            order.push(ch.clone());
        }
        by_ch.entry(ch).or_default().push(m);
    }
    let mut out = Vec::new();
    for ch in order {
        let mut ms = by_ch.remove(&ch).unwrap_or_default();
        ms.sort_by(|a, b| a["min_bp"].as_f64().unwrap_or(0.0).total_cmp(&b["min_bp"].as_f64().unwrap_or(0.0)));
        let in_ch: Vec<&&Value> = peaks.iter().filter(|p| p["channel"] == ch.as_str()).collect();
        if in_ch.is_empty() {
            continue;
        }
        let top = in_ch.iter().map(|p| p["height"].as_f64().unwrap_or(0.0)).fold(f64::NEG_INFINITY, f64::max);
        let first_max = ms.first().map(|m| m["max_bp"].as_f64().unwrap_or(0.0)).unwrap_or(0.0);
        let last_min = ms.last().map(|m| m["min_bp"].as_f64().unwrap_or(0.0)).unwrap_or(0.0);
        for p in in_ch {
            let size = p["size_bp"].as_f64().unwrap_or(0.0);
            let h = p["height"].as_f64().unwrap_or(0.0);
            if ms.iter().any(|m| size >= m["min_bp"].as_f64().unwrap_or(0.0) && size <= m["max_bp"].as_f64().unwrap_or(0.0)) {
                continue;
            }
            if !(size > first_max && size < last_min) {
                continue;
            }
            if cut < 1.0 && h / top <= cut {
                continue;
            }
            out.push(serde_json::json!({"channel": ch, "size_bp": size, "height": h, "ratio": h / top}));
        }
    }
    out
}

/// Samples table quality values: SOS, SQ, SSPK, MIX, OMR, CGQ (with the CGQ override), SE (sample edited), SQO.
pub fn sample_flags(s: &Sample, kit: Option<&Value>, method: Option<&Value>, num_alleles: usize) -> Flags {
    let q = sample_pqv(s);
    let mut f = Flags::new();
    f.insert("SOS", q.sos);
    f.insert("SQ", q.sq);
    f.insert("SSPK", q.sspk);
    f.insert("MIX", "na");
    f.insert("OMR", "na");
    f.insert("CGQ", q.cgq);
    let eff = s.report.as_ref().map(|_| effective(s));
    if eff.as_ref().is_some_and(|e| e.cgq_override) {
        f.insert("CGQ", "override");
    }
    if s.report.is_some() && q.sq != "low" {
        let eff = eff.as_ref().unwrap();
        // MIX: two or more markers with three or more alleles, or one such marker and two PHR flags, or any edit
        f.insert(
            "MIX",
            if s.stype == "ladder" || eff.calls.is_empty() {
                "na"
            } else {
                let mut per: HashMap<String, usize> = HashMap::new();
                for c in eff.calls.iter().filter(|c| truthy_label(&c["label"])) {
                    *per.entry(c["marker"].as_str().unwrap_or("").to_string()).or_default() += 1;
                }
                let many = per.values().filter(|n| **n >= 3).count();
                let phr = marker_rows(s, kit, num_alleles).iter().filter(|r| r.flags["PHR"] == "check").count();
                if many >= 2 || (many >= 1 && phr >= 2) || active(s).next().is_some() {
                    "check"
                } else {
                    "pass"
                }
            },
        );
        f.insert(
            "OMR",
            if s.stype == "ladder" || kit.is_none() {
                "na"
            } else if active(s).any(|e| e["action"] != "override_cgq" && e["action"] != "pli") {
                "check"
            } else if omr_peaks(s, kit, method).is_empty() {
                "pass"
            } else {
                "check"
            },
        );
    }
    f.insert("SE", if s.report.is_some() { if active(s).next().is_some() { "check" } else { "pass" } } else { "na" });
    f.insert("SQO", if s.analysis.as_ref().is_some_and(|a| truthy_label(&a["sizing"]["sq_override"])) { "check" } else { "na" });
    f
}
