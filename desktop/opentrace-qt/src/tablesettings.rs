//! Table Settings (ID-X Table Setting Editor): visible columns, column filters, number of alleles and sort keys of the
//! Samples and Genotypes tables. Port of `resources/web/idx/tables.js`: the same catalog, default, validation and
//! GeneMapper `TableSettingContainer` XML (test `web_parity::same_as_web` runs the web code in a browser).
use serde_json::{json, Map, Value};

pub const MSG_NAME: &str = "Введите имя.";
pub const MSG_NUM_ALLELES: &str = "Число показываемых аллелей: целое от 1 до 20.";
pub const MSG_XML_INVALID: &str = "Файл не является настройкой таблицы GeneMapper (XML) или OpenTrace (JSON).";
pub const MSG_NAME_EXISTS: &str = "Настройка с таким именем уже есть.";
pub const DEFAULT_NAME: &str = "Data Interpretation";

pub struct Col {
    pub id: &'static str,
    pub xml: Option<&'static str>,
    pub head: &'static str,
    pub width: i32,
    pub kind: &'static str,
}

const fn c(id: &'static str, xml: &'static str, head: &'static str, width: i32, kind: &'static str) -> Col {
    Col { id, xml: if xml.is_empty() { None } else { Some(xml) }, head, width, kind }
}

/// [id, XML field of the TableSettingContainer, header, width, kind]; headers are the column names of the original.
pub static SAMPLE_COLS: &[Col] = &[
    c("status", "status", "Status", 48, "icon"),
    c("sampleFile", "sampleFile", "Sample File", 170, "text"),
    c("sampleName", "sampleName", "Sample Name", 130, "text"),
    c("sampleID", "sampleID", "Sample ID", 90, "text"),
    c("comments", "comments", "Comment", 110, "text"),
    c("sampleType", "sampleType", "Sample Type", 150, "edit"),
    c("analysisMethod", "analysisMethod", "Analysis Method", 130, "edit"),
    c("panel", "panel", "Panel", 150, "edit"),
    c("sizeStandard", "sizeStandard", "Size Standard", 130, "edit"),
    c("matrix", "matrix", "Matrix", 70, "text"),
    c("runName", "runName", "Run Name", 150, "text"),
    c("instrumentType", "instrumentType", "Instrument Type", 90, "text"),
    c("instrumentID", "instrumentID", "Instrument ID", 90, "text"),
    c("runDate", "runDate", "Run Date", 130, "text"),
    c("plate", "plate", "Plate Name", 110, "text"),
    c("well", "well", "Well", 44, "text"),
    c("lane", "lane", "Capillary", 58, "num"),
    c("sampleFileNorm", "sampleFileNorm", "SFN", 44, "text"),
    c("ssNormFactor", "normalizationFactor", "SS Normalization Factor", 90, "num"),
    c("SE", "sampleAlleleEdit", "SE", 36, "pqv"),
    c("SQO", "sizingQualityInvalid", "SQO", 38, "pqv"),
    c("ARNM", "arnm", "ARNM", 44, "pqv"),
    c("SFNF", "fileNotFound", "SFNF", 42, "pqv"),
    c("MNF", "matrixNotFound", "MNF", 38, "pqv"),
    c("SNF", "sizeStdNotFound", "SNF", 38, "pqv"),
    c("SOS", "offscale", "SOS", 38, "pqv"),
    c("SQ", "sizeStdQuality", "SQ", 38, "pqv"),
    c("SSPK", "spikeSamplePqvQuality", "SSPK", 42, "pqv"),
    c("MIX", "mixedSamplePqvQuality", "MIX", 38, "pqv"),
    c("OMR", "omrSamplePqvQuality", "OMR", 42, "pqv"),
    c("customControl", "customControl", "Custom Control", 100, "text"),
    c("CGQ", "smplGenoTypeQuality", "CGQ", 42, "pqv"),
    c("specimenCategory", "specimenType", "Specimen Category", 140, "edit"),
    c("profileId", "profileId", "Profile ID", 90, "text"),
    c("UD1", "userDefinedColumn1", "UD1", 60, "text"),
    c("UD2", "userDefinedColumn2", "UD2", 60, "text"),
    c("UD3", "userDefinedColumn3", "UD3", 60, "text"),
    c("UD4", "userDefinedColumn4", "UD4", 60, "text"),
    c("UD5", "userDefinedColumn5", "UD5", 60, "text"),
    c("UD6", "userDefinedColumn6", "UD6", 60, "text"),
    c("UD7", "userDefinedColumn7", "UD7", 60, "text"),
    c("UD8", "userDefinedColumn8", "UD8", 60, "text"),
    c("UD9", "userDefinedColumn9", "UD9", 60, "text"),
];

pub static GENO_COLS: &[Col] = &[
    c("sampleFile", "sampleFile", "Sample File", 160, "text"),
    c("sampleName", "sampleName", "Sample Name", 120, "text"),
    c("sampleID", "sampleID", "Sample ID", 80, "text"),
    c("runName", "runName", "Run Name", 140, "text"),
    c("panel", "panel", "Panel", 130, "text"),
    c("marker", "marker", "Marker", 80, "text"),
    c("dye", "dyeColor", "Dye", 36, "text"),
    c("allele", "allele", "Allele", 56, "allele"),
    c("size", "peakSize", "Size", 60, "allele"),
    c("height", "peakHeight", "Height", 58, "allele"),
    c("area", "peakArea", "Peak Area", 66, "allele"),
    c("dataPoint", "peakScan", "Data Point", 62, "allele"),
    c("aeReason", "alleleComment", "AE Reason For Change", 120, "text"),
    c("MEC", "", "MEC", 120, "text"),
    c("ADO", "", "ADO", 38, "pqv"),
    c("ME", "", "ME", 34, "pqv"),
    c("OS", "offscale", "OS", 34, "pqv"),
    c("BIN", "binProblem", "BIN", 34, "pqv"),
    c("PHR", "peakHeightRatio", "PHR", 38, "pqv"),
    c("LPH", "absoluteHeight", "LPH", 38, "pqv"),
    c("MPH", "highPeakHeight", "MPH", 38, "pqv"),
    c("SPK", "spikeMarkerPqv", "SPK", 36, "pqv"),
    c("AN", "numAlleleError", "AN", 34, "pqv"),
    c("ACC", "amelCrossCheck", "ACC", 36, "pqv"),
    c("BD", "broadPeak", "BD", 34, "pqv"),
    c("CC", "controlConcordance", "CC", 34, "pqv"),
    c("OVL", "overlap", "OVL", 36, "pqv"),
    c("GQ", "alleleConfidence", "GQ", 40, "pqv"),
    c("PLI", "partialLocusIndicator", "PLI", 34, "text"),
    c("profileId", "", "Profile ID", 90, "text"),
    c("UD1", "userDefinedColumn1", "UD1", 60, "text"),
    c("UD2", "userDefinedColumn2", "UD2", 60, "text"),
    c("UD3", "userDefinedColumn3", "UD3", 60, "text"),
    c("UD4", "userDefinedColumn4", "UD4", 60, "text"),
    c("UD5", "userDefinedColumn5", "UD5", 60, "text"),
    c("UD6", "userDefinedColumn6", "UD6", 60, "text"),
    c("UD7", "userDefinedColumn7", "UD7", 60, "text"),
    c("UD8", "userDefinedColumn8", "UD8", 60, "text"),
    c("UD9", "userDefinedColumn9", "UD9", 60, "text"),
];

pub const KINDS: [&str; 2] = ["samples", "genotypes"];

pub fn catalog(kind: &str) -> &'static [Col] {
    if kind == "genotypes" {
        GENO_COLS
    } else {
        SAMPLE_COLS
    }
}

pub fn col(kind: &str, id: &str) -> Option<&'static Col> {
    catalog(kind).iter().find(|c| c.id == id)
}

/// Russian column title (tooltip) of the web interface; the header itself stays as in the original.
pub fn col_title(id: &str, head: &str) -> String {
    static I18N: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    let m = I18N.get_or_init(|| serde_json::from_str(include_str!("table_settings_i18n.json")).unwrap_or(Value::Null));
    m[format!("colT_{id}")].as_str().unwrap_or(head).to_string()
}

/// Russian text of a filter option label ("Show Pass" ...), the label itself when the web has no translation.
pub fn filter_label(label: &str) -> String {
    static I18N: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    let m = I18N.get_or_init(|| serde_json::from_str(include_str!("table_settings_i18n.json")).unwrap_or(Value::Null));
    let key: String = format!("tf_{}", label.chars().filter(|c| c.is_ascii_alphabetic()).collect::<String>());
    m[key].as_str().unwrap_or(label).to_string()
}

/// "3500 Data Analysis_6Dye" visibility of the shipped defaults.
pub fn factory() -> Value {
    json!({"name": DEFAULT_NAME,
        "samples": {"visible": ["status", "sampleFile", "sampleName", "sampleType", "analysisMethod", "panel", "sizeStandard", "sampleFileNorm", "ARNM", "SOS", "SQ", "SSPK", "MIX", "OMR", "customControl", "CGQ"],
            "sort": [{"col": "sampleFile", "desc": false}]},
        "genotypes": {"visible": ["sampleName", "marker", "allele", "OS", "BIN", "PHR", "LPH", "MPH", "SPK", "AN", "ACC", "BD", "CC", "GQ"], "numAlleles": 2,
            "sort": [{"col": "sampleName", "desc": false}, {"col": "dye", "desc": false}, {"col": "marker", "desc": false}]}})
}

// ------------------------------------------------------------------ column filters
// op 0 = Show All Records. Text columns: op 1 = Show Records Containing. Quality columns: pass 1, check 2, low 3, override 4.
#[derive(Clone, Debug, PartialEq)]
pub struct FilterOption {
    pub op: i64,
    pub key: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FilterSpec {
    pub contains: bool,
    pub options: Vec<FilterOption>,
}

fn qual(keys: &[&str]) -> FilterSpec {
    let q = |k: &str| match k {
        "override" => (4, "Show Override"),
        "pass" => (1, "Show Pass"),
        "check" => (2, "Show Check"),
        _ => (3, "Show Low Quality"),
    };
    FilterSpec { contains: false, options: keys.iter().map(|k| FilterOption { op: q(k).0, key: k.to_string(), label: q(k).1.to_string() }).collect() }
}

fn en(pairs: &[(&str, &str)]) -> FilterSpec {
    FilterSpec { contains: false, options: pairs.iter().enumerate().map(|(i, (k, l))| FilterOption { op: i as i64 + 1, key: k.to_string(), label: l.to_string() }).collect() }
}

pub const SPECIMEN: [&str; 21] = [
    "No Export", "Alleged Father", "Alleged Mother", "Biological Child", "Biological Father", "Biological Mother", "Biological Sibling", "Convicted Offender",
    "Deduced Victim Known", "Elimination, Known", "Forensic Mixture", "Forensic, Unknown", "Maternal Relative", "Missing Person", "Other", "Paternal Relative", "Population",
    "Proficiency", "Suspect, Known", "Unidentified Person", "Victim, Known",
];

pub fn filter_spec(kind: &str, id: &str) -> Option<FilterSpec> {
    let col = col(kind, id)?;
    let found = [("pass", "Found"), ("low", "Not Found")];
    let yes_no = [("check", "Yes"), ("pass", "No")];
    let special = if kind == "samples" {
        match id {
            "status" => Some(en(&[("analyzed", "Analyzed"), ("unanalyzed", "Unanalyzed"), ("changed", "Analysis Setting Changed")])),
            "sampleType" => Some(en(&[("sample", "Sample"), ("positive", "Positive Control"), ("ladder", "Allelic Ladder"), ("negative", "Negative Control")])),
            "specimenCategory" => Some(en(&SPECIMEN.iter().map(|x| (*x, *x)).collect::<Vec<_>>())),
            "sampleFileNorm" => Some(en(&[("yes", "Yes"), ("no", "No"), ("na", "N/A")])),
            "SQ" | "CGQ" => Some(qual(&["override", "pass", "check", "low"])),
            "SOS" => Some(qual(&["pass", "check", "low"])),
            "SSPK" | "MIX" | "OMR" | "ARNM" => Some(qual(&["pass", "check"])),
            "SFNF" | "MNF" | "SNF" => Some(en(&found)),
            "SE" | "SQO" => Some(en(&yes_no)),
            _ => None,
        }
    } else {
        match id {
            "dye" => Some(en(&[("B", "B"), ("G", "G"), ("Y", "Y"), ("R", "R"), ("P", "P"), ("O", "O")])),
            "GQ" => Some(qual(&["override", "pass", "check", "low"])),
            "ME" | "ADO" => Some(en(&yes_no)),
            "PLI" => Some(en(&[("check", "Yes"), ("na", "No")])),
            _ => None,
        }
    };
    Some(special.unwrap_or_else(|| {
        if col.kind == "pqv" {
            qual(&["pass", "check", "low"])
        } else {
            FilterSpec { contains: true, options: vec![FilterOption { op: 1, key: String::new(), label: "Show Records Containing:".into() }] }
        }
    }))
}

/// raw: the row's key (status, type, flag, dye letter) for enum filters, the displayed text for text filters.
pub fn matches(kind: &str, id: &str, f: &Value, raw: &str) -> bool {
    let op = f["op"].as_i64().unwrap_or(0);
    if op == 0 {
        return true;
    }
    let Some(sp) = filter_spec(kind, id) else { return true };
    if sp.contains {
        return raw.to_lowercase().contains(&f["text"].as_str().unwrap_or("").to_lowercase());
    }
    match sp.options.iter().find(|o| o.op == op) {
        Some(o) => o.key == raw,
        None => true,
    }
}

/// [(column id, filter)] of the active filters of a table, in the stored order
pub fn active_filters(s: &Value, kind: &str) -> Vec<(String, Value)> {
    s[kind]["filters"].as_object().into_iter().flatten().filter(|(_, f)| f["op"].as_i64().unwrap_or(0) != 0).map(|(k, f)| (k.clone(), f.clone())).collect()
}

// ------------------------------------------------------------------ JavaScript conversions used by validate()
fn js_number(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        Value::Bool(b) => *b as i64 as f64,
        Value::Null => 0.0,
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                0.0
            } else {
                t.parse::<f64>().ok().filter(|_| !t.chars().any(|c| c.is_ascii_alphabetic() && c != 'e' && c != 'E')).unwrap_or(f64::NAN)
            }
        }
        Value::Array(a) if a.is_empty() => 0.0,
        Value::Array(a) if a.len() == 1 => js_number(&a[0]),
        _ => f64::NAN,
    }
}

/// `Number(x)` where a missing key is `undefined` (NaN), unlike JSON null (0)
fn js_number_opt(v: Option<&Value>) -> f64 {
    v.map(js_number).unwrap_or(f64::NAN)
}

fn js_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

fn js_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(_) => crate::kits::js_num(v),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".into(),
        Value::Array(a) => a.iter().map(|x| if x.is_null() { String::new() } else { js_string(x) }).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

/// JavaScript `Math.round` (half up)
fn js_round(f: f64) -> f64 {
    (f + 0.5).floor()
}

fn utf16_prefix(s: &str, n: usize) -> String {
    let u: Vec<u16> = s.encode_utf16().take(n).collect();
    String::from_utf16_lossy(&u)
}

/// Checks and normalises a setting like the web `validate` (unknown columns dropped, at most three sort keys, filters
/// reduced to the ones the column supports).
pub fn validate(s: &Value) -> Result<Value, String> {
    let mut s = s.clone();
    let name_ok = s["name"].as_str().is_some_and(|n| !n.trim().is_empty() && n.encode_utf16().count() <= 240);
    if !s.is_object() || !name_ok {
        return Err(MSG_NAME.into());
    }
    for kind in KINDS {
        if !s[kind].is_object() || !s[kind]["visible"].is_array() {
            return Err(format!("table setting: {kind}"));
        }
        let has = |id: &str| col(kind, id).is_some();
        let t = s[kind].as_object_mut().unwrap();
        let visible: Vec<Value> = t["visible"].as_array().unwrap().iter().filter(|x| x.as_str().is_some_and(has)).cloned().collect();
        t.insert("visible".into(), Value::Array(visible));
        let sort: Vec<Value> = t
            .get("sort")
            .and_then(|x| x.as_array())
            .into_iter()
            .flatten()
            .filter(|x| x["col"].as_str().is_some_and(has))
            .take(3)
            .map(|x| json!({"col": x["col"], "desc": js_truthy(&x["desc"])}))
            .collect();
        t.insert("sort".into(), Value::Array(sort));
        let mut fl = Map::new();
        if let Some(o) = t.get("filters").and_then(|x| x.as_object()) {
            for (id, f) in o {
                let Some(sp) = filter_spec(kind, id) else { continue };
                if !js_truthy(f) {
                    continue;
                }
                let n = js_number_opt(f.get("op"));
                let op = if n.is_finite() { n.trunc() as i64 } else { 0 };
                if op == 0 {
                    continue;
                }
                if sp.contains {
                    let raw = f.get("text").filter(|x| js_truthy(x)).map(js_string).unwrap_or_default();
                    let text = utf16_prefix(&raw, 200);
                    if !text.is_empty() {
                        fl.insert(id.clone(), json!({"op": 1, "text": text}));
                    }
                } else if sp.options.iter().any(|o| o.op == op) {
                    fl.insert(id.clone(), json!({"op": op}));
                }
            }
        }
        t.insert("filters".into(), Value::Object(fl));
    }
    let n = js_number_opt(s["genotypes"].get("numAlleles"));
    if !(n.is_finite() && n.fract() == 0.0 && (1.0..=20.0).contains(&n)) {
        return Err(MSG_NUM_ALLELES.into());
    }
    let g = s["genotypes"].as_object_mut().unwrap();
    g.insert("numAlleles".into(), json!(n as i64));
    let lim = g.get("limitToBins") == Some(&Value::Bool(true));
    let only = g.get("onlyBinned") == Some(&Value::Bool(true));
    g.insert("limitToBins".into(), json!(lim));
    g.insert("onlyBinned".into(), json!(only));
    for kind in KINDS {
        let t = s[kind].as_object_mut().unwrap();
        let f = js_round(js_number_opt(t.get("fontSize")));
        t.insert("fontSize".into(), json!(if (8.0..=24.0).contains(&f) { f as i64 } else { 0 }));
    }
    Ok(s)
}

// ------------------------------------------------------------------ GeneMapper TableSettingContainer XML
fn text_content(n: roxmltree::Node) -> String {
    n.descendants().filter(|d| d.is_text()).filter_map(|d| d.text()).collect()
}

fn child<'a, 'i>(n: roxmltree::Node<'a, 'i>, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
    n.children().find(|c| c.is_element() && c.tag_name().name() == name)
}

/// JavaScript `parseInt(text, 10)`
fn parse_int(s: &str) -> Option<i64> {
    let t = s.trim_start();
    let (neg, rest) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    let v = digits.parse::<f64>().ok()?;
    Some(if neg { -v } else { v } as i64)
}

pub fn from_xml(text: &str) -> Result<Value, String> {
    if text.len() > 1_048_576 {
        return Err("XML > 1 MiB".into());
    }
    let up = text.to_ascii_uppercase();
    if up.contains("<!DOCTYPE") || up.contains("<!ENTITY") {
        return Err("DTD".into());
    }
    let doc = roxmltree::Document::parse(text).map_err(|_| MSG_XML_INVALID.to_string())?;
    let prof = doc.descendants().find(|n| n.is_element() && n.tag_name().name() == "xmlProjectTableProfile").ok_or(MSG_XML_INVALID)?;
    let name = child(prof, "profileName").map(text_content).filter(|t| !t.is_empty()).unwrap_or_else(|| "Imported".into());
    let mut s = json!({"name": name.trim(), "samples": {"visible": [], "sort": []}, "genotypes": {"visible": [], "numAlleles": 2, "sort": []}});
    for (kind, sec_name) in [("samples", "sampleProfile"), ("genotypes", "markerProfile")] {
        let Some(sec) = child(prof, sec_name) else { continue };
        for c in catalog(kind) {
            let Some(x) = c.xml else { continue };
            let Some(n) = child(sec, &format!("{x}Col")).or_else(|| child(sec, x)) else { continue };
            if child(n, "visible").is_some_and(|v| text_content(v).trim() == "true") {
                s[kind]["visible"].as_array_mut().unwrap().push(json!(c.id));
            }
            if let Some(fc) = child(n, "filterCriteria") {
                let op = child(fc, "operation").map(text_content).and_then(|t| parse_int(&t)).unwrap_or(0);
                if op != 0 {
                    let text = child(fc, "operand1").map(text_content).unwrap_or_default();
                    if !s[kind]["filters"].is_object() {
                        s[kind]["filters"] = json!({});
                    }
                    s[kind]["filters"][c.id] = json!({"op": op, "text": text.trim()});
                }
            }
        }
    }
    if let Some(na) = child(prof, "markerProfile").and_then(|mp| child(mp, "numAllelesToShow")) {
        let n = parse_int(&text_content(na)).filter(|n| *n != 0).unwrap_or(2);
        s["genotypes"]["numAlleles"] = json!(n.clamp(1, 20));
    }
    for (kind, keys, orders) in [("samples", "sampleColsSort", "sampleSortColOrder"), ("genotypes", "markerColsSort", "markerSortOrder")] {
        let list = |tag: &str| -> Vec<String> { prof.children().filter(|c| c.is_element() && c.tag_name().name() == tag).map(|c| text_content(c).trim().to_string()).collect() };
        let (ks, os) = (list(keys), list(orders));
        let sort: Vec<Value> = ks
            .iter()
            .enumerate()
            .filter_map(|(i, k)| catalog(kind).iter().rev().find(|c| c.head == k).map(|c| json!({"col": c.id, "desc": os.get(i).is_some_and(|o| o == "1")})))
            .collect();
        s[kind]["sort"] = Value::Array(sort);
    }
    validate(&s)
}

fn esc(x: &str) -> String {
    x.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub fn to_xml(s: &Value) -> String {
    let sec = |kind: &str, name: &str| -> String {
        let vis: Vec<&str> = s[kind]["visible"].as_array().into_iter().flatten().filter_map(|x| x.as_str()).collect();
        let mut x = format!("<{name}>");
        for c in catalog(kind) {
            let Some(xml) = c.xml else { continue };
            let f = &s[kind]["filters"][c.id];
            x += &format!(
                "<{xml}Col><visible>{}</visible><filterCriteria><operation>{}</operation><operand1>{}</operand1><operand2></operand2></filterCriteria></{xml}Col>",
                vis.contains(&c.id),
                f["op"].as_i64().unwrap_or(0),
                esc(f["text"].as_str().unwrap_or(""))
            );
        }
        if kind == "genotypes" {
            x += &format!("<numAllelesToShow>{}</numAllelesToShow>", crate::kits::js_num(&s["genotypes"]["numAlleles"]));
        }
        x + &format!("</{name}>")
    };
    let sorts = |kind: &str, keys: &str, orders: &str| -> String {
        let rows: Vec<Option<&Value>> = (0..3).map(|i| s[kind]["sort"].get(i)).collect();
        let head = |r: &Value| col(kind, r["col"].as_str().unwrap_or("")).map(|c| c.head).unwrap_or("undefined");
        rows.iter().map(|r| format!("<{keys}>{}</{keys}>", esc(r.map(head).unwrap_or("None")))).collect::<String>()
            + &rows.iter().map(|r| format!("<{orders}>{}</{orders}>", if r.is_some_and(|r| r["desc"] == true) { 1 } else { 0 })).collect::<String>()
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<TableSettingContainer xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:type=\"TableSettingContainer\"><appVersion>1.4</appVersion><fileVersion>1.0</fileVersion><xmlProjectTableProfile><profileName>{}</profileName><owner>OpenTrace</owner>{}{}{}{}</xmlProjectTableProfile></TableSettingContainer>\n",
        esc(s["name"].as_str().unwrap_or("")),
        sec("genotypes", "markerProfile"),
        sorts("samples", "sampleColsSort", "sampleSortColOrder"),
        sorts("genotypes", "markerColsSort", "markerSortOrder"),
        sec("samples", "sampleProfile")
    )
}

// ------------------------------------------------------------------ store (library.json "table_settings")
#[derive(Clone)]
pub struct Db {
    pub settings: Vec<(Value, String)>,
    pub active: String,
    /// the ID-X Database Defaults (resources/data/shipped_table_settings) were added once
    pub shipped: bool,
}

impl Default for Db {
    fn default() -> Self {
        Db { settings: vec![(validate(&factory()).unwrap(), crate::edits::now_iso())], active: DEFAULT_NAME.into(), shipped: false }
    }
}

impl Db {
    pub fn from_json(v: &Value) -> Db {
        let mut db = Db::default();
        let rows: Vec<(Value, String)> =
            v["settings"].as_array().into_iter().flatten().filter_map(|r| validate(&r["setting"]).ok().map(|s| (s, r["saved"].as_str().unwrap_or("").to_string()))).collect();
        if !rows.is_empty() {
            db.settings = rows;
        }
        db.active = v["active"].as_str().unwrap_or("").to_string();
        db.shipped = v["shipped"] == true;
        db
    }
    pub fn to_json(&self) -> Value {
        json!({"schema": 1, "active": self.active, "shipped": self.shipped,
            "settings": self.settings.iter().map(|(s, saved)| json!({"setting": s, "saved": saved})).collect::<Vec<_>>()})
    }
    pub fn names(&self) -> Vec<String> {
        self.settings.iter().map(|(s, _)| s["name"].as_str().unwrap_or("").to_string()).collect()
    }
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.settings.iter().find(|(s, _)| s["name"] == name).map(|(s, _)| s)
    }
    /// the active setting: the stored name if it still exists, else the first
    pub fn active(&self) -> &Value {
        self.get(&self.active).unwrap_or(&self.settings[0].0)
    }
    /// Save (replace the setting named `previous`, else add); a new name must not be taken by another setting.
    pub fn put(&mut self, s: &Value, previous: &str) -> Result<Value, String> {
        let mut v = s.clone();
        if let Some(n) = v["name"].as_str().map(|n| n.trim().to_string()) {
            v["name"] = json!(n);
        }
        let v = validate(&v)?;
        let name = v["name"].as_str().unwrap_or("").to_string();
        if name != previous && self.get(&name).is_some() {
            return Err(MSG_NAME_EXISTS.into());
        }
        let row = (v.clone(), crate::edits::now_iso());
        match self.settings.iter().position(|(x, _)| x["name"] == previous) {
            Some(i) => self.settings[i] = row,
            None => self.settings.push(row),
        }
        if !previous.is_empty() && self.active == previous {
            self.active = name;
        }
        Ok(v)
    }
    pub fn remove(&mut self, name: &str) {
        self.settings.retain(|(s, _)| s["name"] != name);
        if self.settings.is_empty() {
            let shipped = self.shipped;
            *self = Db::default();
            self.shipped = shipped;
        }
        if self.active == name {
            self.active = self.settings[0].0["name"].as_str().unwrap_or("").to_string();
        }
    }
    pub fn untitled_name(&self) -> String {
        (1..).map(|n| format!("Настройка таблицы {n}")).find(|n| self.get(n).is_none()).unwrap()
    }
    /// Adds the ID-X Database Defaults table settings once (each XML goes through `from_xml`, so the result is exactly
    /// what OpenTrace can represent); names already present are kept. Returns true when the store changed.
    pub fn ensure_shipped(&mut self, dir: &std::path::Path) -> bool {
        if self.shipped {
            return false;
        }
        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "xml")).collect();
        if files.is_empty() {
            return false;
        }
        files.sort_by_key(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()));
        for p in files {
            let Ok(text) = std::fs::read_to_string(&p) else { continue };
            if let Ok(s) = from_xml(&text.replace("\r\n", "\n")) {
                if self.get(s["name"].as_str().unwrap_or("")).is_none() {
                    let _ = self.put(&s, "");
                }
            }
        }
        self.shipped = true;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_valid_and_xml_round_trip() {
        let d = validate(&factory()).unwrap();
        assert_eq!(d["genotypes"]["numAlleles"], 2);
        assert_eq!(d["samples"]["fontSize"], 0);
        assert_eq!(from_xml(&to_xml(&d)).unwrap(), d);
    }

    #[test]
    fn validate_normalises_and_rejects() {
        let mut s = factory();
        s["samples"]["visible"] = json!(["status", "nope", "SQ"]);
        s["samples"]["sort"] = json!([{"col": "sampleFile"}, {"col": "x"}, {"col": "SQ", "desc": 1}, {"col": "well"}, {"col": "panel"}]);
        s["samples"]["filters"] = json!({"SQ": {"op": 3}, "SOS": {"op": 4}, "sampleName": {"op": 2, "text": "ab"}, "well": {"op": 1, "text": ""}, "zzz": {"op": 1}});
        s["genotypes"]["numAlleles"] = json!("4");
        s["genotypes"]["fontSize"] = json!(11.6);
        let v = validate(&s).unwrap();
        assert_eq!(v["samples"]["visible"], json!(["status", "SQ"]));
        assert_eq!(v["samples"]["sort"], json!([{"col": "sampleFile", "desc": false}, {"col": "SQ", "desc": true}, {"col": "well", "desc": false}]));
        assert_eq!(v["samples"]["filters"], json!({"SQ": {"op": 3}, "sampleName": {"op": 1, "text": "ab"}}));
        assert_eq!(v["genotypes"]["numAlleles"], 4);
        assert_eq!(v["genotypes"]["fontSize"], 12);
        for bad in [json!(0), json!(21), json!(2.5), json!("x")] {
            let mut b = factory();
            b["genotypes"]["numAlleles"] = bad;
            assert!(validate(&b).is_err());
        }
        let mut b = factory();
        b["name"] = json!("  ");
        assert!(validate(&b).is_err());
        assert!(from_xml("<!DOCTYPE x><a/>").is_err());
        assert!(from_xml("<a/>").is_err());
    }

    #[test]
    fn filters_match() {
        assert!(matches("samples", "SQ", &json!({"op": 3}), "low"));
        assert!(!matches("samples", "SQ", &json!({"op": 3}), "pass"));
        assert!(matches("samples", "sampleName", &json!({"op": 1, "text": "ЛеС"}), "Лестница 1"));
        assert!(!matches("genotypes", "dye", &json!({"op": 2}), "B"));
        assert!(matches("genotypes", "dye", &json!({"op": 2}), "G"));
        assert!(matches("genotypes", "GQ", &json!({"op": 0}), "low"));
    }

    #[test]
    fn store() {
        let mut db = Db::default();
        let mut s = factory();
        s["name"] = json!(" Мои столбцы ");
        db.put(&s, "").unwrap();
        assert_eq!(db.names(), vec![DEFAULT_NAME.to_string(), "Мои столбцы".to_string()]);
        assert!(db.put(&s, "").is_err(), "taken name");
        db.active = "Мои столбцы".into();
        s["name"] = json!("Другое");
        db.put(&s, "Мои столбцы").unwrap();
        assert_eq!(db.active, "Другое");
        let back = Db::from_json(&db.to_json());
        assert_eq!(back.names(), db.names());
        db.remove("Другое");
        assert_eq!(db.active().get("name").unwrap(), DEFAULT_NAME);
    }
}

#[cfg(test)]
mod web_parity {
    use super::*;
    /// OT_TS_WEB=<json written by the browser run of tables.js>, OT_TS_XMLS=<XML samples (shipped + hostile)>:
    /// default, validation, XML export and import identical to the web version.
    #[test]
    fn same_as_web() {
        let Ok(path) = std::env::var("OT_TS_WEB") else { return };
        let w: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let d = validate(&factory()).unwrap();
        assert_eq!(d, w["default"], "default");
        assert_eq!(to_xml(&d), w["defaultXml"].as_str().unwrap());
        for kind in KINDS {
            let cat: Vec<Value> = catalog(kind).iter().map(|c| json!({"id": c.id, "xml": c.xml, "head": c.head, "width": c.width, "kind": c.kind})).collect();
            assert_eq!(Value::Array(cat), w["catalog"][kind], "catalog {kind}");
            for c in catalog(kind) {
                let sp = filter_spec(kind, c.id).unwrap();
                let web = &w["specs"][kind][c.id];
                assert_eq!(sp.contains, web["type"] == "contains", "{kind} {}", c.id);
                let opts: Vec<Value> = sp.options.iter().map(|o| if sp.contains { json!({"op": o.op, "label": o.label}) } else { json!({"op": o.op, "key": o.key, "label": o.label}) }).collect();
                assert_eq!(Value::Array(opts), web["options"], "{kind} {}", c.id);
            }
        }
        let cases: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(std::env::var("OT_TS_CASES").unwrap()).unwrap()).unwrap();
        for (i, c) in cases.iter().enumerate() {
            match validate(c) {
                Ok(s) => {
                    assert_eq!(s, w["cases"][i]["ok"], "case {i}");
                    assert_eq!(to_xml(&s), w["cases"][i]["xml"].as_str().unwrap(), "case xml {i}");
                }
                Err(e) => assert!(w["cases"][i]["err"].is_string(), "case {i}: native error {e}, web {}", w["cases"][i]),
            }
        }
        let xmls: Vec<String> = serde_json::from_str(&std::fs::read_to_string(std::env::var("OT_TS_XMLS").unwrap()).unwrap()).unwrap();
        for (i, x) in xmls.iter().enumerate() {
            match from_xml(x) {
                Ok(s) => {
                    assert_eq!(s, w["imports"][i]["ok"], "import {i}");
                    assert_eq!(to_xml(&s), w["imports"][i]["xml"].as_str().unwrap(), "import xml {i}");
                }
                Err(e) => assert!(w["imports"][i]["err"].is_string(), "import {i}: native error {e}, web {}", w["imports"][i]),
            }
        }
        for (i, m) in w["matches"].as_array().unwrap().iter().enumerate() {
            let got = matches(m["kind"].as_str().unwrap(), m["id"].as_str().unwrap(), &m["f"], m["raw"].as_str().unwrap());
            assert_eq!(json!(got), m["out"], "match {i}: {m}");
        }
    }
}
