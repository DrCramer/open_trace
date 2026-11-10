//! Samples and Genotypes tables as the active Table Setting describes them: columns, cell values, the column filters and
//! the sort keys (web `sampleCell`, `genoCell`, `tsFilter`, `sortRows` of project.js).
use crate::backend::{Backend, Sample, Status};
use crate::bridge::ffi::{TableCol, TableRow};
use crate::flags::{Flags, MarkerRow};
use crate::tablesettings as ts;
use serde_json::Value;
use std::cmp::Ordering;

pub fn dye_letter(tag: &str) -> &'static str {
    match tag {
        "DATA1" => "B",
        "DATA2" => "G",
        "DATA3" => "Y",
        "DATA4" => "R",
        "DATA105" => "O",
        "DATA106" => "P",
        _ => "",
    }
}

pub fn type_text(t: &str) -> &'static str {
    match t {
        "ladder" => "Аллельная лестница",
        "positive" => "Положительный контроль",
        "negative" => "Отрицательный контроль",
        _ => "Образец",
    }
}

/// web status keys (the filter of the Status column knows analyzed / unanalyzed / changed)
fn status_key(s: &Sample) -> &'static str {
    match s.status {
        Status::Analyzed => "analyzed",
        Status::Running => "running",
        Status::Failed => "failed",
        Status::NotAnalyzed => "unanalyzed",
    }
}

/// JavaScript `String(Number(x.toFixed(digits)))`
fn fixed(x: f64, digits: usize) -> String {
    let s = format!("{x:.digits$}");
    if s.contains('.') {
        let t = s.trim_end_matches('0').trim_end_matches('.');
        if t == "-0" { "0".into() } else { t.to_string() }
    } else {
        s
    }
}

fn js_text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => crate::kits::js_num(other),
    }
}

fn sample_cell(id: &str, s: &Sample, fl: &Flags) -> String {
    let null = Value::Null;
    let an = s.analysis.as_deref().unwrap_or(&null);
    let info = &an["run_info"];
    match id {
        "status" => status_key(s).into(),
        "sampleFile" => s.file.clone(),
        "sampleName" => s.name.clone(),
        "comments" => js_text(&info["comment"]),
        "sampleType" => type_text(&s.stype).into(),
        "analysisMethod" => s.method.clone(),
        "panel" => s.panel_key.split('\u{1}').nth(1).unwrap_or("").to_string(),
        "sizeStandard" => s.size_std.clone(),
        "runName" => js_text(&info["run_name"]),
        "instrumentType" => js_text(&info["instrument_type"]),
        "instrumentID" => js_text(&info["instrument_id"]),
        "runDate" => js_text(&info["run_date"]),
        "plate" => js_text(&info["plate"]),
        "well" => if info["well"].is_string() { js_text(&info["well"]) } else { s.well.clone() },
        "lane" => js_text(&info["capillary"]),
        "sampleFileNorm" => js_text(&an["normalization"]["sample_file"]),
        "ssNormFactor" => {
            let n = &an["normalization"];
            if !n.is_object() {
                String::new()
            } else if n["sample_file"] == "Yes" && !n["factor"].is_null() {
                js_text(&n["factor"])
            } else if n["sample_file"] == "N/A" {
                String::new()
            } else {
                "N/A".into()
            }
        }
        "specimenCategory" => "No Export".into(),
        "sampleID" | "matrix" | "customControl" | "profileId" => String::new(),
        _ if id.starts_with("UD") => String::new(),
        _ => fl.get(id).copied().unwrap_or("na").to_string(),
    }
}

/// the row's key for enum filters, the displayed text for text filters (web `sampleRaw`)
fn sample_raw(id: &str, s: &Sample, fl: &Flags) -> String {
    match id {
        "status" => match s.status {
            Status::Analyzed => "analyzed".into(),
            _ => "unanalyzed".into(),
        },
        "sampleType" => s.stype.clone(),
        "sampleFileNorm" => match s.analysis.as_deref().map(|a| &a["normalization"]["sample_file"]) {
            Some(v) if v == "Yes" => "yes".into(),
            Some(v) if v == "No" => "no".into(),
            _ => "na".into(),
        },
        _ => sample_cell(id, s, fl),
    }
}

/// the call shown in allele column `i` of a marker row (web `genoCell`: "only binned alleles in their bin positions",
/// "limit to the number of bins")
fn call_at<'a>(r: &'a MarkerRow, i: usize, gts: &Value, kit: Option<&Value>) -> Option<&'a Value> {
    let only = gts["onlyBinned"] == true;
    let limit = gts["limitToBins"] == true;
    if only || limit {
        let bins: Vec<&Value> = kit
            .and_then(|k| k["markers"].as_array())
            .and_then(|ms| ms.iter().find(|m| m["name"] == r.marker.as_str()))
            .and_then(|m| m["bins"].as_array())
            .map(|b| b.iter().collect())
            .unwrap_or_default();
        if only {
            let lab = bins.get(i).map(|b| &b["label"]).filter(|l| !l.is_null())?;
            return r.calls.iter().find(|c| &c["label"] == lab);
        }
        return if bins.is_empty() || i < bins.len() { r.calls.get(i) } else { None };
    }
    r.calls.get(i)
}

struct GenoCtx<'a> {
    s: &'a Sample,
    r: &'a MarkerRow,
    gts: &'a Value,
    kit: Option<&'a Value>,
}

fn geno_cell(id: &str, g: &GenoCtx, i: usize) -> String {
    let (s, r) = (g.s, g.r);
    let c = call_at(r, i, g.gts, g.kit);
    let num = |k: &str, digits: usize| c.and_then(|c| c[k].as_f64()).map(|x| if digits == 0 { format!("{}", (x + 0.5).floor() as i64) } else { fixed(x, digits) }).unwrap_or_default();
    match id {
        "sampleFile" => s.file.clone(),
        "sampleName" => s.name.clone(),
        "runName" => s.analysis.as_deref().map(|a| js_text(&a["run_info"]["run_name"])).unwrap_or_default(),
        "panel" => s.panel_key.split('\u{1}').nth(1).unwrap_or("").to_string(),
        "marker" => r.marker.clone(),
        "dye" => dye_letter(&r.channel).into(),
        "allele" => c.map(|c| js_text(&c["label"])).unwrap_or_default(),
        "size" => num("size_bp", 2),
        "height" => num("height", 0),
        "area" => c
            .and_then(|c| c["peak_index"].as_u64())
            .and_then(|pi| s.report.as_deref().and_then(|rep| rep["peaks"].get(pi as usize)))
            .and_then(|p| p["area"].as_f64())
            .map(|a| format!("{}", (a + 0.5).floor() as i64))
            .unwrap_or_default(),
        "dataPoint" => num("scan", 0),
        "aeReason" => r.reason.clone(),
        "MEC" => r.mec.clone(),
        "PLI" => if r.flags.get("PLI") == Some(&"check") { "✓".into() } else { String::new() },
        "sampleID" | "profileId" => String::new(),
        _ if id.starts_with("UD") => String::new(),
        _ => r.flags.get(id).copied().unwrap_or("na").to_string(),
    }
}

const ALLELE_COLS: [&str; 5] = ["allele", "size", "height", "area", "dataPoint"];

fn geno_raw(id: &str, g: &GenoCtx) -> String {
    if id == "dye" {
        return dye_letter(&g.r.channel).into();
    }
    if ts::filter_spec("genotypes", id).is_some_and(|sp| !sp.contains) {
        return g.r.flags.get(id).copied().unwrap_or("na").to_string();
    }
    if ALLELE_COLS.contains(&id) {
        return (0..g.r.calls.len()).map(|i| geno_cell(id, g, i)).collect::<Vec<_>>().join(" ");
    }
    geno_cell(id, g, 0)
}

fn number_of(v: &str) -> Option<f64> {
    let t = v.trim();
    if t.is_empty() || t.chars().any(|c| c.is_alphabetic() && c != 'e' && c != 'E') {
        return None;
    }
    t.parse::<f64>().ok().filter(|f| !f.is_nan())
}

/// `String.localeCompare(..., {numeric: true})` closely enough for table sorting: digit runs compare as numbers, letters
/// without regard to case (lower case first on a tie).
fn natural(a: &str, b: &str) -> Ordering {
    let (x, y): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let (mut i, mut j) = (0, 0);
    while i < x.len() && j < y.len() {
        if x[i].is_ascii_digit() && y[j].is_ascii_digit() {
            let (si, sj) = (i, j);
            while i < x.len() && x[i].is_ascii_digit() {
                i += 1;
            }
            while j < y.len() && y[j].is_ascii_digit() {
                j += 1;
            }
            let na: String = x[si..i].iter().collect::<String>().trim_start_matches('0').to_string();
            let nb: String = y[sj..j].iter().collect::<String>().trim_start_matches('0').to_string();
            let o = na.len().cmp(&nb.len()).then_with(|| na.cmp(&nb));
            if o != Ordering::Equal {
                return o;
            }
            continue;
        }
        let (ca, cb) = (x[i].to_lowercase().next().unwrap_or(x[i]), y[j].to_lowercase().next().unwrap_or(y[j]));
        if ca != cb {
            return ca.cmp(&cb);
        }
        i += 1;
        j += 1;
    }
    (x.len() - i).cmp(&(y.len() - j)).then_with(|| b.cmp(a))
}

/// web `compare`: two numeric values as numbers, anything else as text
pub fn compare(a: &str, b: &str) -> Ordering {
    match (number_of(a), number_of(b)) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        _ => natural(a, b),
    }
}

fn sort_keys(setting: &Value, kind: &str) -> Vec<(String, bool)> {
    setting[kind]["sort"].as_array().into_iter().flatten().filter_map(|k| Some((k["col"].as_str()?.to_string(), k["desc"] == true))).collect()
}

/// order of the rows by the sort keys (stable; ties keep the given order)
fn order(n: usize, keys: &[(String, bool)], value: impl Fn(usize, &str) -> String) -> Vec<usize> {
    let vals: Vec<Vec<String>> = (0..n).map(|i| keys.iter().map(|(c, _)| value(i, c)).collect()).collect();
    let mut idx: Vec<usize> = (0..n).collect();
    idx.sort_by(|&a, &b| {
        for (k, (_, desc)) in keys.iter().enumerate() {
            let d = compare(&vals[a][k], &vals[b][k]);
            if d != Ordering::Equal {
                return if *desc { d.reverse() } else { d };
            }
        }
        a.cmp(&b)
    });
    idx
}

impl Backend {
    pub fn active_table_setting(&self) -> Value {
        self.table_settings.lock().unwrap().active().clone()
    }
    /// "Number of alleles to show" of the active table setting (the ADO flag depends on it)
    pub fn num_alleles(&self) -> usize {
        self.table_settings.lock().unwrap().active()["genotypes"]["numAlleles"].as_u64().unwrap_or(2) as usize
    }

    pub fn table_columns(&self, kind: &str) -> Vec<TableCol> {
        let s = self.active_table_setting();
        let n = s["genotypes"]["numAlleles"].as_u64().unwrap_or(2) as usize;
        let mut out = Vec::new();
        for id in s[kind]["visible"].as_array().into_iter().flatten().filter_map(|x| x.as_str()) {
            let Some(c) = ts::col(kind, id) else { continue };
            let title = ts::col_title(c.id, c.head);
            let shown = match (c.id, c.kind) {
                ("status", _) => "status",
                ("PLI", _) => "mark",
                (_, "pqv") => "pqv",
                (_, "num") | (_, "allele") if c.id != "allele" => "num",
                _ => "text",
            };
            if c.kind == "allele" {
                for i in 0..n {
                    out.push(TableCol { id: c.id.into(), head: format!("{} {}", c.head, i + 1), title: format!("{title} {}", i + 1), width: c.width, kind: shown.into(), index: i as i32 });
                }
            } else {
                out.push(TableCol { id: c.id.into(), head: c.head.into(), title, width: c.width, kind: shown.into(), index: -1 });
            }
        }
        out
    }

    /// One row per sample in project order: the cells of the visible columns, whether the column filters of the table
    /// setting let the row through, and its place in the order of the setting's sort keys.
    pub fn samples_table(&self) -> Vec<TableRow> {
        let setting = self.active_table_setting();
        let list: Vec<Sample> = self.samples.lock().unwrap().clone();
        let n_all = self.num_alleles();
        let flags: Vec<Flags> = list
            .iter()
            .map(|s| {
                let kit = self.kit_of(s);
                let method = self.methods.lock().unwrap().iter().find(|m| m["name"] == s.method.as_str()).cloned();
                crate::flags::sample_flags(s, kit.as_ref().map(|k| &k.kit), method.as_ref(), n_all)
            })
            .collect();
        let cols = self.table_columns("samples");
        let filters = ts::active_filters(&setting, "samples");
        let ord = order(list.len(), &sort_keys(&setting, "samples"), |i, c| sample_cell(c, &list[i], &flags[i]));
        let mut rank = vec![0usize; list.len()];
        for (pos, &i) in ord.iter().enumerate() {
            rank[i] = pos;
        }
        list.iter()
            .enumerate()
            .map(|(i, s)| {
                let fl = &flags[i];
                let tips = cols
                    .iter()
                    .map(|c| match c.id.as_str() {
                        "status" if !s.error.is_empty() => s.error.clone(),
                        "status" => s.genotype_error.clone(),
                        _ => String::new(),
                    })
                    .collect();
                TableRow {
                    source: i,
                    marker: String::new(),
                    cells: cols.iter().map(|c| sample_cell(&c.id, s, fl)).collect(),
                    tips,
                    shown: filters.iter().all(|(id, f)| ts::matches("samples", id, f, &sample_raw(id, s, fl))),
                    rank: rank[i],
                }
            })
            .collect()
    }

    /// One row per marker of every genotyped sample, in the order of the setting's sort keys.
    pub fn genotypes_table(&self) -> Vec<TableRow> {
        let setting = self.active_table_setting();
        let gts = &setting["genotypes"];
        let list: Vec<Sample> = self.samples.lock().unwrap().clone();
        let n_all = self.num_alleles();
        let kits: Vec<_> = list.iter().map(|s| self.kit_of(s)).collect();
        let mut rows: Vec<(usize, MarkerRow)> = Vec::new();
        for (i, s) in list.iter().enumerate() {
            if s.report.is_none() {
                continue;
            }
            for r in crate::flags::marker_rows(s, kits[i].as_ref().map(|k| &k.kit), n_all) {
                rows.push((i, r));
            }
        }
        let ctx = |k: usize| GenoCtx { s: &list[rows[k].0], r: &rows[k].1, gts, kit: kits[rows[k].0].as_ref().map(|k| &k.kit) };
        let cols = self.table_columns("genotypes");
        let filters = ts::active_filters(&setting, "genotypes");
        let ord = order(rows.len(), &sort_keys(&setting, "genotypes"), |k, c| geno_cell(c, &ctx(k), 0));
        ord.iter()
            .enumerate()
            .map(|(pos, &k)| {
                let g = ctx(k);
                let ladder = g.s.stype == "ladder";
                TableRow {
                    source: rows[k].0,
                    marker: g.r.marker.clone(),
                    cells: cols.iter().map(|c| if c.id == "GQ" && ladder { "na".to_string() } else { geno_cell(&c.id, &g, c.index.max(0) as usize) }).collect(),
                    tips: cols.iter().map(|c| if c.id == "GQ" { g.r.gq.map(|q| format!("{q:.3}")).unwrap_or_default() } else { String::new() }).collect(),
                    shown: filters.iter().all(|(id, f)| ts::matches("genotypes", id, f, &geno_raw(id, &g))),
                    rank: pos,
                }
            })
            .collect()
    }

    /// column ids with an active filter in the active table setting
    pub fn table_filter_columns(&self, kind: &str) -> Vec<String> {
        ts::active_filters(&self.active_table_setting(), kind).into_iter().map(|(id, _)| id).collect()
    }
    pub fn table_font_size(&self, kind: &str) -> i32 {
        self.active_table_setting()[kind]["fontSize"].as_i64().unwrap_or(0) as i32
    }

    // ---- store
    pub fn table_setting_names(&self) -> Vec<String> {
        self.table_settings.lock().unwrap().names()
    }
    pub fn table_setting_json(&self, name: &str) -> String {
        self.table_settings.lock().unwrap().get(name).map(|v| v.to_string()).unwrap_or_default()
    }
    pub fn table_setting_new_json(&self) -> String {
        let mut s = ts::validate(&ts::factory()).unwrap();
        s["name"] = Value::String(self.table_settings.lock().unwrap().untitled_name());
        s.to_string()
    }
    /// "" or the error text
    pub fn table_setting_put(&self, json: &str, previous: &str) -> String {
        let v: Value = match serde_json::from_str(json) {
            Ok(v) => v,
            Err(_) => return ts::MSG_XML_INVALID.into(),
        };
        let r = self.table_settings.lock().unwrap().put(&v, previous);
        match r {
            Ok(_) => self.save_library().err().unwrap_or_default(),
            Err(e) => e,
        }
    }
    pub fn table_setting_remove(&self, name: &str) {
        self.table_settings.lock().unwrap().remove(name);
        let _ = self.save_library();
    }
    pub fn table_setting_active(&self) -> String {
        self.table_settings.lock().unwrap().active()["name"].as_str().unwrap_or("").to_string()
    }
    pub fn table_setting_set_active(&self, name: &str) {
        {
            let mut db = self.table_settings.lock().unwrap();
            if db.active == name || db.get(name).is_none() {
                return;
            }
            db.active = name.to_string();
        }
        let _ = self.save_library();
    }
    /// parsed setting as JSON ("" and `last_import_error` on failure)
    pub fn table_setting_import(&self, path: &str) -> String {
        let r = std::fs::read_to_string(path).map_err(|e| e.to_string()).and_then(|t| {
            if t.trim_start().starts_with('{') {
                serde_json::from_str::<Value>(&t).map_err(|_| ts::MSG_XML_INVALID.to_string()).and_then(|v| ts::validate(&v))
            } else {
                ts::from_xml(&t.replace("\r\n", "\n"))
            }
        });
        match r {
            Ok(v) => {
                self.last_error.lock().unwrap().clear();
                v.to_string()
            }
            Err(e) => {
                *self.last_error.lock().unwrap() = e;
                String::new()
            }
        }
    }
    pub fn table_setting_export(&self, name: &str, path: &str) -> String {
        let Some(s) = self.table_settings.lock().unwrap().get(name).cloned() else { return "нет такой настройки".into() };
        std::fs::write(path, ts::to_xml(&s)).err().map(|e| e.to_string()).unwrap_or_default()
    }
    /// Columns of a table for the editor: [{id, head, title, filter: {contains, options: [{op, label}]}}] as JSON
    pub fn table_setting_catalog(&self, kind: &str) -> String {
        let cols: Vec<Value> = ts::catalog(kind)
            .iter()
            .map(|c| {
                let sp = ts::filter_spec(kind, c.id).unwrap();
                serde_json::json!({"id": c.id, "head": c.head, "title": ts::col_title(c.id, c.head), "contains": sp.contains,
                    "options": sp.options.iter().map(|o| serde_json::json!({"op": o.op, "label": ts::filter_label(&o.label)})).collect::<Vec<_>>()})
            })
            .collect();
        Value::Array(cols).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_like_the_web() {
        assert_eq!(compare("2", "10"), Ordering::Less);
        assert_eq!(compare("a2", "a10"), Ordering::Less);
        assert_eq!(compare("B", "a"), Ordering::Greater);
        assert_eq!(compare("", "1"), Ordering::Less);
        assert_eq!(compare("D3S1358", "D21S11"), Ordering::Less);
        assert_eq!(compare("12.5", "12.25"), Ordering::Greater);
        assert_eq!(fixed(120.5, 2), "120.5");
        assert_eq!(fixed(99.999, 2), "100");
        assert_eq!(fixed(101.126, 2), "101.13");
        let ord = order(4, &[("a".into(), true)], |i, _| ["b", "c", "a", "c"][i].to_string());
        assert_eq!(ord, vec![1, 3, 0, 2]);
    }
}
