//! Plot settings (port of the web `plotsettings.js`): the ID-X Plot Settings Editor model (General, Sample Header,
//! Genotype Header, Sizing Table, Labels, Display Settings), the factory settings shipped with ID-X, validation,
//! GeneMapper `PlotSettingContainer` XML import / export and the allele label lines of the Labels tab.
//! The JSON shape is the web one, so settings move between the two interfaces unchanged.
use serde_json::{json, Map, Value};

pub const DYES: [&str; 6] = ["blue", "green", "yellow", "red", "purple", "orange"];
/// [id, XmlPlotSetting field, header]
pub const SAMPLE_HEADER: [(&str, &str, &str); 9] = [
    ("sampleFileName", "sdrSampleFileNameSelected", "Sample File"),
    ("sampleName", "sdrSampleNameSelected", "Sample Name"),
    ("panel", "sdrPanelSelected", "Panel"),
    ("SOS", "sdrPqvOSselected", "SOS"),
    ("SQ", "sdrPqvSQselected", "SQ"),
    ("SSPK", "sdrSpikeSamplePqvQualitySelected", "SSPK"),
    ("MIX", "sdrMixedSamplePqvQualitySelected", "MIX"),
    ("OMR", "sdrOmrSamplePqvQualitySelected", "OMR"),
    ("CGQ", "sdrSampleGenotypeQualitySelected", "CGQ"),
];
pub const GENO_HEADER: [(&str, &str, &str); 16] = [
    ("sampleFileName", "gdrSampleFileNameSelected", "Sample File"),
    ("sampleName", "gdrSampleNameSelected", "Sample Name"),
    ("panel", "gdrPanelSelected", "Panel"),
    ("marker", "gdrMarkerSelected", "Marker"),
    ("AN", "gdrPqvANEselected", "AN"),
    ("BIN", "gdrPqvBINselected", "BIN"),
    ("PHR", "gdrPqvPHRselected", "PHR"),
    ("LPH", "gdrPqvLPHselected", "LPH"),
    ("MPH", "gdrPqvHPHselected", "MPH"),
    ("BD", "gdrPqvBDselected", "BD"),
    ("SPK", "gdrSpikeMarkerPqvSelected", "SPK"),
    ("OS", "gdrPqvOSselected", "OS"),
    ("OVL", "gdrPqvOverLapSelected", "OVL"),
    ("ACC", "gdrPqvACCselected", "ACC"),
    ("CC", "gdrPqvCCselected", "CC"),
    ("GQ", "gdrPqvGQselected", "GQ"),
];
pub const SIZING_COLS: [(&str, &str, &str); 9] = [
    ("sampleFileName", "stSampleFileNameSelected", "Sample File"),
    ("marker", "stMarkerSelected", "Marker"),
    ("dyeSamplePeak", "dyeSamplePeakSelected", "Dye/Sample Peak"),
    ("minutes", "minutesSelected", "Minutes"),
    ("size", "sizeSelected", "Size"),
    ("height", "peakHeightSelected", "Height"),
    ("area", "peakAreaSelected", "Area"),
    ("dataPoint", "dataPointSelected", "Data Point"),
    ("alleleCall", "alleleCallSelected", "Allele Call"),
];
/// label content per peak type (up to four lines); [id, name, prefix]
pub const LABEL_TYPES: [(&str, &str, &str); 7] = [
    ("none", "None", ""),
    ("alleleCall", "Allele Call", "al"),
    ("size", "Size", "sz"),
    ("height", "Height", "ht"),
    ("area", "Area", "ar"),
    ("dataPoint", "Data Point", "dp"),
    ("aeReason", "AE Reason for Change", "ae"),
];
pub const LABEL_KINDS: [(&str, &str, &str); 4] = [
    ("assigned", "assignedAlleleLabel", "Assigned Allele"),
    ("custom", "customAlleleLabel", "Custom Allele"),
    ("ladder", "allelicLadderLabel", "Allelic Ladder"),
    ("artifact", "artifactLabel", "Artifact"),
];
/// Java logical fonts as stored by ID-X plus common system fonts
pub const FONTS: [&str; 10] = ["Serif", "SansSerif", "Dialog", "DialogInput", "Monospaced", "Arial", "Tahoma", "Verdana", "Times New Roman", "Courier New"];
const LABEL_XML: [(&str, &str); 7] = [
    ("none", "NONE"),
    ("alleleCall", "Allele Call"),
    ("size", "Size"),
    ("height", "Height"),
    ("area", "Area"),
    ("dataPoint", "Data Point"),
    ("aeReason", "AE Reason For Change"),
];
const DEFAULT_COLORS: &str = "-13405973,-15937240,-16777216,-63482,-26317,-8355712,-6724096,-915324,-12602374,-14895826,-5046093,-39276,-1841112,-4024346,-14881137,-6684673";
const FACTORY: &str = include_str!("plot_settings_factory.json");
pub const FACTORY_VERSION: i64 = 2;
/// enumerations: positions in the ID-X editor lists (1-based), as PlotDisplaySetting keeps them
const E_PERSISTENCE: [&str; 2] = ["last", "these"];
const E_LABELS: [&str; 3] = ["none", "h", "v"];
const E_XAXIS: [&str; 2] = ["dp", "bp"];
const E_YAXIS: [&str; 3] = ["individual", "max", "entered"];
const E_TABLE: [&str; 4] = ["none", "sizing", "genotypes", "lev"];
const E_LAYOUT: [&str; 3] = ["combine", "separate", "overlay"];
const E_COLOR: [&str; 3] = ["bw", "dyeAll", "dyeBorder"];

// Russian messages (web i18n keys in comments)
pub const MSG_XML_INVALID: &str = "Файл не является корректным XML настроек."; // xmlInvalid
pub const MSG_NAME_REQUIRED: &str = "Это новая настройка графика. Введите уникальное имя."; // psNameRequired
pub const MSG_NAME_EXISTS: &str = "Настройка с таким именем уже есть. Введите уникальное имя."; // psNameExists
const MSG_NAME_LONG: &str = "Имя слишком длинное (не более 255 символов)."; // psNameLong
const MSG_NAME_CHARS: &str = "Имя содержит недопустимые символы"; // psNameChars
const MSG_RANGE: &str = "Начало и конец диапазона — числа от 0 до 10 000"; // psRangeNumbers
const MSG_END_GT_START: &str = "Конец диапазона должен быть больше начала"; // psEndGtStart
pub const MSG_DYE_SCALE: &str = "Введите значение масштаба красителя от 0,1 до 100."; // psDyeScaleRange
const MSG_FONT_SIZE: &str = "Размер шрифта — от 1 до 20."; // psFontSize
const MSG_IMPORT_LABEL_FONT: &str = "Файл не импортирован: размер шрифта меток должен быть от 1 до 20."; // psImportLabelFont
const MSG_IMPORT_SIZING_FONT: &str = "Файл не импортирован: размер шрифта таблицы размеров должен быть от 1 до 20."; // psImportSizingFont
const MSG_NO_SETTING: &str = "В файле нет настройки графика (XmlPlotSetting)"; // psNoSetting

/// JavaScript number -> JSON value (integers stay integers, as `JSON.stringify` writes them).
pub fn num(f: f64) -> Value {
    if f.fract() == 0.0 && f.abs() < 9e15 {
        json!(f as i64)
    } else {
        serde_json::Number::from_f64(f).map(Value::Number).unwrap_or(Value::Null)
    }
}
/// JavaScript `String(number)`.
fn js(v: &Value) -> String {
    crate::kits::js_num(v)
}
/// JavaScript `Math.round`.
fn js_round(f: f64) -> f64 {
    (f + 0.5).floor()
}
/// JavaScript `Number(value)` of a JSON value (NaN for what is not a number).
fn number(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        Value::String(s) => js_number(s),
        Value::Bool(b) => *b as i32 as f64,
        Value::Null => 0.0,
        _ => f64::NAN,
    }
}
/// JavaScript `Number(text)` / unary `+`.
fn js_number(s: &str) -> f64 {
    let t = s.trim();
    if t.is_empty() {
        return 0.0;
    }
    match t {
        "Infinity" | "+Infinity" => f64::INFINITY,
        "-Infinity" => f64::NEG_INFINITY,
        _ if t.chars().any(|c| c.is_ascii_alphabetic() && c != 'e' && c != 'E') => f64::NAN,
        _ => t.parse::<f64>().unwrap_or(f64::NAN),
    }
}
/// JavaScript `parseInt(text, 10)` (leading integer; None for NaN).
fn parse_int(s: &str) -> Option<i64> {
    let t = s.trim_start();
    let (neg, rest) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    let v: i128 = digits.chars().take(30).collect::<String>().parse().ok()?;
    let v = if neg { -v } else { v };
    Some(v.clamp(i64::MIN as i128, i64::MAX as i128) as i64)
}
/// JavaScript `parseFloat(text)` (leading decimal number).
fn parse_float(s: &str) -> f64 {
    let t = s.trim_start();
    let mut end = 0;
    let b = t.as_bytes();
    let mut seen_digit = false;
    let mut seen_dot = false;
    let mut seen_e = false;
    while end < b.len() {
        let c = b[end];
        if c.is_ascii_digit() {
            seen_digit = true;
        } else if (c == b'-' || c == b'+') && (end == 0 || matches!(b[end - 1], b'e' | b'E')) {
        } else if c == b'.' && !seen_dot && !seen_e {
            seen_dot = true;
        } else if (c == b'e' || c == b'E') && seen_digit && !seen_e {
            seen_e = true;
        } else {
            break;
        }
        end += 1;
    }
    let mut cand = &t[..end];
    while !cand.is_empty() {
        if let Ok(v) = cand.parse::<f64>() {
            return v;
        }
        cand = &cand[..cand.len() - 1];
    }
    f64::NAN
}

/// ID-X stores java.awt.Color ARGB as signed ints; "#rrggbb" in the JSON.
pub fn color_from_xml(v: &str) -> Option<String> {
    let v = v.trim();
    if v.len() == 7 && v.starts_with('#') && v[1..].chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(v.to_lowercase());
    }
    let n = parse_int(v)?;
    let u = (n as i128).rem_euclid(1 << 32) as u64;
    Some(format!("#{:06x}", u & 0xffffff))
}
pub fn color_to_xml(c: &str) -> String {
    let hex: String = c.get(1..).unwrap_or("").chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    let rgb = u64::from_str_radix(&hex.chars().take(15).collect::<String>(), 16).unwrap_or(0);
    let v = ((rgb as u32) | 0xff00_0000) as i32;
    v.to_string()
}

fn all(rows: &[(&str, &str, &str)], v: bool) -> Value {
    Value::Object(rows.iter().map(|r| (r.0.to_string(), Value::Bool(v))).collect())
}

/// Defaults (the values shared by the ID-X factory files).
pub fn base(name: &str, description: &str) -> Value {
    let colors: Vec<Value> = DEFAULT_COLORS.split(',').filter_map(color_from_xml).map(Value::String).collect();
    let dyes: Map<String, Value> = DYES.iter().map(|d| (d.to_string(), Value::Bool(true))).collect();
    let scales: Map<String, Value> = DYES.iter().map(|d| (d.to_string(), json!(1))).collect();
    json!({
        "name": name, "description": description,
        "sampleHeader": all(&SAMPLE_HEADER, true), "genotypeHeader": all(&GENO_HEADER, true),
        "sizingTable": {"cols": all(&SIZING_COLS, true), "font": "Dialog", "size": 11},
        "labels": {"assigned": ["alleleCall", "none", "none", "none"], "custom": ["alleleCall", "none", "none", "none"],
            "ladder": ["alleleCall", "none", "none", "none"], "artifact": ["alleleCall", "none", "none", "none"],
            "font": "Serif", "size": 10, "color": "dyeBorder", "prefixes": false, "editType": false, "pqvTrigger": false, "virtualBlack": false},
        "display": {"persistence": "these", "panes": 2, "header": true, "markerRange": true, "markerIndicator": true, "bins": true,
            "peakPositions": false, "alleleChanges": true, "toolbar": true, "offscale": true, "controlsTop": false, "laddersTop": false,
            "sizeStdLabels": true, "labels": "v", "xAxis": "bp", "yAxis": "individual", "yValue": 8191, "dyes": dyes, "allDyes": true,
            "start": 0, "end": 500, "table": "genotypes", "layout": "separate", "printQVD": false, "customColors": colors,
            "markerMargin": 5, "dyeScales": scales}
    })
}

fn deep_merge(dst: &mut Value, src: &Value) {
    let Some(src) = src.as_object() else { return };
    for (k, v) in src {
        match (dst.get_mut(k), v) {
            (Some(d @ Value::Object(_)), Value::Object(_)) => deep_merge(d, v),
            _ => {
                if let Some(o) = dst.as_object_mut() {
                    o.insert(k.clone(), v.clone());
                }
            }
        }
    }
}

/// The plot settings shipped with ID-X ("Data Interpretation" ... "Mixture Analysis Defaults").
pub fn factory() -> Vec<Value> {
    let list: Vec<Value> = serde_json::from_str(FACTORY).expect("factory plot settings");
    list.iter()
        .map(|d| {
            let mut b = base(d["name"].as_str().unwrap_or(""), d["description"].as_str().unwrap_or(""));
            deep_merge(&mut b, d);
            b
        })
        .collect()
}

fn js_type(v: &Value) -> &'static str {
    match v {
        Value::Null | Value::Array(_) | Value::Object(_) => "object",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
    }
}

/// web `merge` of `validate`: known keys only, objects recursively, arrays element-wise, scalars of the same type.
fn merge_known(dst: &mut Value, src: &Value) {
    let Some(src) = src.as_object() else { return };
    let Some(d) = dst.as_object_mut() else { return };
    for (k, dv) in d.iter_mut() {
        let Some(v) = src.get(k) else { continue };
        match dv {
            Value::Object(_) => merge_known(dv, v),
            Value::Array(da) => {
                if let Value::Array(va) = v {
                    let mut out: Vec<Value> = va.iter().take(da.len()).cloned().collect();
                    out.extend(da.iter().skip(va.len()).cloned());
                    *da = out;
                }
            }
            _ => {
                if js_type(dv) == js_type(v) {
                    *dv = v.clone();
                }
            }
        }
    }
}

fn pick<'a>(v: &Value, allowed: &[&'a str], dflt: &'a str) -> &'a str {
    allowed.iter().copied().find(|a| v.as_str() == Some(a)).unwrap_or(dflt)
}

/// Normalised copy of a plot setting or the reason it is not valid (web `validate`).
pub fn validate(s: &Value) -> Result<Value, String> {
    if !s.is_object() {
        return Err(MSG_XML_INVALID.into());
    }
    let name = match &s["name"] {
        Value::String(x) => x.trim().to_string(),
        Value::Null => String::new(),
        other => js(other).trim().to_string(),
    };
    if name.is_empty() {
        return Err(MSG_NAME_REQUIRED.into());
    }
    if name.encode_utf16().count() > 255 {
        return Err(MSG_NAME_LONG.into());
    }
    if name.contains(['<', '>', '"', '\\']) {
        return Err(MSG_NAME_CHARS.into());
    }
    let mut d = base(&name, "");
    merge_known(&mut d, s);
    d["name"] = json!(name);
    {
        let dd = &mut d["display"];
        let p = js_round(number(&dd["panes"]));
        let p = if p == 0.0 || p.is_nan() { 1.0 } else { p };
        dd["panes"] = num(p.clamp(1.0, 16.0));
        for k in ["start", "end"] {
            let v = number(&dd[k]);
            if !(v >= 0.0 && v <= 10000.0) {
                return Err(MSG_RANGE.into());
            }
        }
        if !dd["allDyes"].as_bool().unwrap_or(false) && number(&dd["end"]) <= number(&dd["start"]) {
            return Err(MSG_END_GT_START.into());
        }
        for dy in DYES {
            let v = number(&dd["dyeScales"][dy]);
            if !(v >= 0.1 && v <= 100.0) {
                return Err(MSG_DYE_SCALE.into());
            }
        }
        if !(number(&dd["yValue"]) > 0.0) {
            dd["yValue"] = json!(1000);
        }
        let mm = js_round(number(&dd["markerMargin"]));
        dd["markerMargin"] = num(if mm.is_nan() { 0.0 } else { mm.max(0.0) });
    }
    for k in ["labels", "sizingTable"] {
        let v = number(&d[k]["size"]);
        if !(v >= 1.0 && v <= 20.0) {
            return Err(MSG_FONT_SIZE.into());
        }
    }
    for (k, _, _) in LABEL_KINDS {
        let ok = d["labels"][k].as_array().is_some_and(|a| a.iter().all(|x| LABEL_TYPES.iter().any(|t| x.as_str() == Some(t.0))));
        if !ok {
            return Err(MSG_XML_INVALID.into());
        }
    }
    let dd = &mut d["display"];
    dd["labels"] = json!(pick(&dd["labels"], &E_LABELS, "h"));
    dd["xAxis"] = json!(pick(&dd["xAxis"], &E_XAXIS, "bp"));
    dd["yAxis"] = json!(pick(&dd["yAxis"], &E_YAXIS, "individual"));
    dd["table"] = json!(pick(&dd["table"], &E_TABLE, "genotypes"));
    dd["layout"] = json!(pick(&dd["layout"], &E_LAYOUT, "separate"));
    dd["persistence"] = json!(pick(&dd["persistence"], &E_PERSISTENCE, "these"));
    d["labels"]["color"] = json!(pick(&d["labels"]["color"], &E_COLOR, "bw"));
    Ok(d)
}

// ------------------------------------------------------------------ PlotSettingContainer XML
fn esc(x: &str) -> String {
    x.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
fn enc(list: &[&str], v: &Value) -> i64 {
    list.iter().position(|x| v.as_str() == Some(x)).map(|i| i as i64 + 1).unwrap_or(0)
}
fn dec<'a>(list: &[&'a str], n: Option<&str>, d: &'a str) -> &'a str {
    let i = n.and_then(parse_int).unwrap_or(0);
    if i >= 1 && (i as usize) <= list.len() {
        list[i as usize - 1]
    } else {
        d
    }
}
fn cap(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}
/// `${value}` of the web writer (strings as they are, numbers JavaScript-style).
fn tv(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "undefined".into(),
        other => js(other),
    }
}

/// GeneMapper ID-X `PlotSettingContainer` XML of a setting (the same bytes as the web `toXml`).
pub fn to_xml(s: &Value) -> String {
    let d = &s["display"];
    let l = &s["labels"];
    let b = |n: &str, v: &Value| format!("<{n}>{}</{n}>", truthy(v));
    let t = |n: &str, v: String| format!("<{n}>{}</{n}>", esc(&v));
    let mut x = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<PlotSettingContainer xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:type=\"PlotSettingContainer\">");
    x += "<appVersion>1.6</appVersion><fileVersion>1.0</fileVersion><xmlPlotSetting>";
    x += &t("name", tv(&s["name"]));
    x += &t("comments", tv(&s["description"]));
    x += &t("userName", "OpenTrace".into());
    for (id, f, _) in SAMPLE_HEADER {
        x += &b(f, &s["sampleHeader"][id]);
    }
    for (id, f, _) in GENO_HEADER {
        x += &b(f, &s["genotypeHeader"][id]);
    }
    for (id, f, _) in SIZING_COLS {
        x += &b(f, &s["sizingTable"]["cols"][id]);
    }
    x += &t("sizingTableFontName", tv(&s["sizingTable"]["font"]));
    x += &t("sizingTableFontSize", tv(&s["sizingTable"]["size"]));
    for (k, f, _) in LABEL_KINDS {
        for (i, v) in l[k].as_array().into_iter().flatten().enumerate() {
            let v = v.as_str().unwrap_or("");
            let out = if k == "artifact" && v == "alleleCall" { "Artifact Label" } else { LABEL_XML.iter().find(|x| x.0 == v).map(|x| x.1).unwrap_or("NONE") };
            x += &t(&format!("{f}{}", i + 1), out.into());
        }
    }
    x += &t("labelingFontName", tv(&l["font"]));
    x += &t("labelingFontSize", tv(&l["size"]));
    x += &t("labelingColorScheme", enc(&E_COLOR, &l["color"]).to_string());
    x += &b("showDataTypePrefix", &l["prefixes"]);
    x += &b("showManualChangePrefix", &l["editType"]);
    x += &b("showPqvTriggerPeak", &l["pqvTrigger"]);
    x += &b("invertMutantLabel", &l["virtualBlack"]);
    x += "<plotDisplaySetting>";
    x += &t("persistenceMode", enc(&E_PERSISTENCE, &d["persistence"]).to_string());
    x += &t("numPane", tv(&d["panes"]));
    for (n, k) in [
        ("showPaneHeader", "header"),
        ("showMarkerRange", "markerRange"),
        ("showMarkerIndicator", "markerIndicator"),
        ("showBin", "bins"),
        ("showPeakPositionIndicators", "peakPositions"),
        ("showAlleleEditHistory", "alleleChanges"),
        ("showToolBar", "toolbar"),
        ("showOffscale", "offscale"),
        ("controlsAtTopOfPane", "controlsTop"),
        ("laddersAtTopOfPane", "laddersTop"),
        ("showSizeLabels", "sizeStdLabels"),
    ] {
        x += &b(n, &d[k]);
    }
    x += &t("labelStatus", enc(&E_LABELS, &d["labels"]).to_string());
    x += &t("xAxisMode", enc(&E_XAXIS, &d["xAxis"]).to_string());
    x += &t("yAxisScaleMode", enc(&E_YAXIS, &d["yAxis"]).to_string());
    x += &t("yAxisScaleToUseValue", tv(&d["yValue"]));
    x += &b("dyeAllSelected", &d["allDyes"]);
    for dy in DYES {
        x += &b(&format!("dye{}Selected", cap(dy)), &d["dyes"][dy]);
    }
    x += &t("allDyeStartRange", tv(&d["start"]));
    x += &t("allDyeEndRange", tv(&d["end"]));
    x += &t("tableMode", enc(&E_TABLE, &d["table"]).to_string());
    x += &t("dyeMode", enc(&E_LAYOUT, &d["layout"]).to_string());
    x += &b("printQualityValueDetails", &d["printQVD"]);
    let cc: Vec<String> = d["customColors"].as_array().into_iter().flatten().map(|c| color_to_xml(c.as_str().unwrap_or(""))).collect();
    x += &t("customColors", cc.join(","));
    x += &t("markerMargin", tv(&d["markerMargin"]));
    for dy in ["blue", "green", "yellow", "red", "purple", "orange", "turquoise", "fuchsia"] {
        let v = &d["dyeScales"][dy];
        x += &t(&format!("dye{}ScaleFactor", cap(dy)), if truthy(v) { tv(v) } else { "1".into() });
    }
    x += "</plotDisplaySetting></xmlPlotSetting></PlotSettingContainer>\n";
    x
}

fn label_from_xml(v: Option<&str>) -> Option<&'static str> {
    let x = v.unwrap_or("").trim().to_lowercase();
    if x == "artifact label" {
        return Some("alleleCall");
    }
    LABEL_XML.iter().find(|(k, n)| n.to_lowercase() == x || k.to_lowercase() == x).map(|(k, _)| *k)
}

/// GeneMapper ID-X `PlotSettingContainer` XML -> validated setting (web `fromXml`).
pub fn from_xml(text: &str) -> Result<Value, String> {
    if text.len() > 1_048_576 {
        return Err("XML > 1 MiB".into());
    }
    let low = text.to_ascii_lowercase();
    if low.contains("<!doctype") || low.contains("<!entity") {
        return Err("DTD".into());
    }
    let doc = roxmltree::Document::parse(text).map_err(|_| MSG_XML_INVALID.to_string())?;
    let p = doc.descendants().find(|n| n.is_element() && n.tag_name().name() == "xmlPlotSetting").ok_or_else(|| MSG_NO_SETTING.to_string())?;
    fn child<'a, 'i>(n: roxmltree::Node<'a, 'i>, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
        n.children().find(|c| c.is_element() && c.tag_name().name() == name)
    }
    fn txt(n: roxmltree::Node, name: &str) -> Option<String> {
        child(n, name).map(|c| c.descendants().filter(|d| d.is_text()).map(|d| d.text().unwrap_or("")).collect::<String>().trim().to_string())
    }
    let bool_of = |n: roxmltree::Node, name: &str, d: bool| -> bool { txt(n, name).map(|v| v == "true").unwrap_or(d) };
    let or = |v: Option<String>| v.filter(|s| !s.is_empty());
    let mut s = base(&or(txt(p, "name")).unwrap_or_else(|| "Imported".into()), &or(txt(p, "comments")).unwrap_or_default());
    for (id, f, _) in SAMPLE_HEADER {
        s["sampleHeader"][id] = json!(bool_of(p, f, true));
    }
    for (id, f, _) in GENO_HEADER {
        s["genotypeHeader"][id] = json!(bool_of(p, f, true));
    }
    for (id, f, _) in SIZING_COLS {
        s["sizingTable"]["cols"][id] = json!(bool_of(p, f, true));
    }
    if let Some(f) = or(txt(p, "sizingTableFontName")) {
        s["sizingTable"]["font"] = json!(f);
    }
    let size = or(txt(p, "sizingTableFontSize")).map(|v| js_number(&v)).unwrap_or(11.0);
    if !(size >= 1.0 && size <= 20.0) {
        return Err(MSG_IMPORT_SIZING_FONT.into());
    }
    s["sizingTable"]["size"] = num(size);
    let nones = || vec!["none"; 4];
    for (k, f, _) in LABEL_KINDS {
        let nums: Vec<Option<String>> = (1..=4).map(|i| txt(p, &format!("{f}{i}"))).collect();
        if nums.iter().any(|v| v.is_some()) {
            let a: Vec<&str> = nums.iter().map(|v| label_from_xml(v.as_deref()).unwrap_or("none")).collect();
            let cut = a.iter().position(|x| *x == "none").unwrap_or(a.len());
            let mut out: Vec<&str> = a[..cut].to_vec();
            out.extend(nones());
            out.truncate(4);
            s["labels"][k] = json!(out);
        } else if let Some(v) = or(txt(p, f)) {
            let mut out: Vec<&str> = v.split(',').filter_map(|x| label_from_xml(Some(x))).collect();
            out.extend(nones());
            out.truncate(4);
            s["labels"][k] = json!(out);
        }
    }
    if let Some(f) = or(txt(p, "labelingFontName")) {
        s["labels"]["font"] = json!(f);
    }
    let lsize = or(txt(p, "labelingFontSize")).map(|v| js_number(&v)).unwrap_or(10.0);
    if !(lsize >= 1.0 && lsize <= 20.0) {
        return Err(MSG_IMPORT_LABEL_FONT.into());
    }
    s["labels"]["size"] = num(lsize);
    s["labels"]["color"] = json!(dec(&E_COLOR, txt(p, "labelingColorScheme").as_deref(), "bw"));
    s["labels"]["prefixes"] = json!(bool_of(p, "showDataTypePrefix", false));
    s["labels"]["editType"] = json!(bool_of(p, "showManualChangePrefix", false));
    s["labels"]["pqvTrigger"] = json!(bool_of(p, "showPqvTriggerPeak", false));
    s["labels"]["virtualBlack"] = json!(bool_of(p, "invertMutantLabel", false));
    if let Some(d) = child(p, "plotDisplaySetting") {
        let dd = &mut s["display"];
        dd["persistence"] = json!(dec(&E_PERSISTENCE, txt(d, "persistenceMode").as_deref(), "these"));
        // a number that does not parse is NaN in the web version; `validate` then applies its defaults or refuses
        // NaN of the web version: what `validate` makes of it (panes 1, Y value 1000, margin 0, range refused)
        let n = |name: &str, cur: &Value, nan: Option<f64>| -> Result<Value, String> {
            match or(txt(d, name)).map(|v| js_number(&v)) {
                None => Ok(cur.clone()),
                Some(f) if f.is_finite() => Ok(num(f)),
                Some(_) => nan.map(num).ok_or_else(|| MSG_RANGE.to_string()),
            }
        };
        dd["panes"] = n("numPane", &dd["panes"], Some(1.0))?;
        for (k, f) in [
            ("header", "showPaneHeader"),
            ("markerRange", "showMarkerRange"),
            ("markerIndicator", "showMarkerIndicator"),
            ("bins", "showBin"),
            ("peakPositions", "showPeakPositionIndicators"),
            ("alleleChanges", "showAlleleEditHistory"),
            ("toolbar", "showToolBar"),
            ("offscale", "showOffscale"),
            ("controlsTop", "controlsAtTopOfPane"),
            ("laddersTop", "laddersAtTopOfPane"),
            ("sizeStdLabels", "showSizeLabels"),
            ("allDyes", "dyeAllSelected"),
            ("printQVD", "printQualityValueDetails"),
        ] {
            let cur = dd[k].as_bool().unwrap_or(false);
            dd[k] = json!(bool_of(d, f, cur));
        }
        dd["labels"] = json!(dec(&E_LABELS, txt(d, "labelStatus").as_deref(), "h"));
        dd["xAxis"] = json!(dec(&E_XAXIS, txt(d, "xAxisMode").as_deref(), "bp"));
        dd["yAxis"] = json!(dec(&E_YAXIS, txt(d, "yAxisScaleMode").as_deref(), "individual"));
        dd["yValue"] = n("yAxisScaleToUseValue", &dd["yValue"], Some(1000.0))?;
        for dy in DYES {
            let cur = dd["dyes"][dy].as_bool().unwrap_or(true);
            dd["dyes"][dy] = json!(bool_of(d, &format!("dye{}Selected", cap(dy)), cur));
        }
        dd["start"] = n("allDyeStartRange", &dd["start"], None)?;
        dd["end"] = n("allDyeEndRange", &dd["end"], None)?;
        dd["table"] = json!(dec(&E_TABLE, txt(d, "tableMode").as_deref(), "genotypes"));
        dd["layout"] = json!(dec(&E_LAYOUT, txt(d, "dyeMode").as_deref(), "separate"));
        if let Some(cc) = or(txt(d, "customColors")) {
            let mut list: Vec<Value> = cc.split(',').filter_map(color_from_xml).map(Value::String).collect();
            list.extend(dd["customColors"].as_array().cloned().unwrap_or_default());
            list.truncate(16);
            dd["customColors"] = Value::Array(list);
        }
        dd["markerMargin"] = n("markerMargin", &dd["markerMargin"], Some(0.0))?;
        for dy in DYES {
            let v = txt(d, &format!("dye{}ScaleFactor", cap(dy))).map(|v| parse_float(&v)).unwrap_or(f64::NAN);
            if v > 0.0 {
                dd["dyeScales"][dy] = num(v);
            }
        }
    }
    validate(&s)
}

// ------------------------------------------------------------------ store (library.json "plot_settings")
#[derive(Clone)]
pub struct Db {
    pub settings: Vec<(Value, String)>,
    pub active: String,
    /// "Use the display settings last used for this plot": the interactive state per setting name
    pub last_view: Map<String, Value>,
}

impl Default for Db {
    fn default() -> Self {
        let now = crate::edits::now_iso();
        Db { settings: factory().into_iter().map(|s| (s, now.clone())).collect(), active: String::new(), last_view: Map::new() }
    }
}

impl Db {
    pub fn from_json(v: &Value) -> Db {
        let mut db = Db::default();
        let rows: Vec<(Value, String)> = v["settings"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| validate(&r["setting"]).ok().map(|s| (s, r["saved"].as_str().unwrap_or("").to_string())))
            .collect();
        if !rows.is_empty() {
            db.settings = rows;
        }
        db.active = v["active"].as_str().unwrap_or("").to_string();
        if let Some(o) = v["last_view"].as_object() {
            db.last_view = o.clone();
        }
        db
    }
    pub fn to_json(&self) -> Value {
        json!({
            "factory": FACTORY_VERSION,
            "settings": self.settings.iter().map(|(s, saved)| json!({"setting": s, "saved": saved})).collect::<Vec<_>>(),
            "active": self.active,
            "last_view": self.last_view,
        })
    }
    pub fn names(&self) -> Vec<String> {
        self.settings.iter().map(|(s, _)| s["name"].as_str().unwrap_or("").to_string()).collect()
    }
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.settings.iter().find(|(s, _)| s["name"] == name).map(|(s, _)| s)
    }
    /// Save (replace the setting named `previous`, else add); a new name must not be taken by another setting.
    pub fn put(&mut self, s: &Value, previous: &str) -> Result<Value, String> {
        let v = validate(s)?;
        let name = v["name"].as_str().unwrap_or("").to_string();
        if name != previous && self.get(&name).is_some() {
            return Err(MSG_NAME_EXISTS.into());
        }
        let row = (v.clone(), crate::edits::now_iso());
        match self.settings.iter().position(|(x, _)| x["name"] == previous) {
            Some(i) => self.settings[i] = row,
            None => self.settings.push(row),
        }
        if !previous.is_empty() && previous != name {
            if self.active == previous {
                self.active = name.clone();
            }
            if let Some(lv) = self.last_view.remove(previous) {
                self.last_view.insert(name, lv);
            }
        }
        Ok(v)
    }
    pub fn remove(&mut self, name: &str) {
        self.settings.retain(|(s, _)| s["name"] != name);
        self.last_view.remove(name);
        if self.settings.is_empty() {
            *self = Db::default();
        }
    }
    /// active setting name: the stored one if it still exists, else "Data Interpretation", else the first
    pub fn active_name(&self) -> String {
        let names = self.names();
        if names.contains(&self.active) {
            return self.active.clone();
        }
        if names.iter().any(|n| n == "Data Interpretation") {
            return "Data Interpretation".into();
        }
        names.first().cloned().unwrap_or_default()
    }
    /// display settings of a setting with the last interactive state merged when it says "use last"
    pub fn effective_display(&self, name: &str) -> Value {
        let Some(s) = self.get(name) else { return base("", "")["display"].clone() };
        let mut d = s["display"].clone();
        if d["persistence"] == "last" {
            if let Some(lv) = self.last_view.get(name) {
                deep_merge(&mut d, lv);
            }
        }
        d
    }
    pub fn untitled_name(&self) -> String {
        let mut n = 1;
        loop {
            let name = format!("Новая настройка графика {n}");
            if self.get(&name).is_none() {
                return name;
            }
            n += 1;
        }
    }
}

// ------------------------------------------------------------------ label lines (Labels tab)
/// PQV trigger peaks (LPH, MPH, BD, OS) of a call from the marker GQ details.
pub fn triggers(c: &Value, report: &Value) -> Vec<&'static str> {
    let row = report["quality"]["marker_gq"]["markers"].as_array().and_then(|a| a.iter().find(|m| m["marker"] == c["marker"]));
    let null = Value::Null;
    let sc = row.map(|r| &r["score"]).unwrap_or(&null);
    let m = &sc["metrics"];
    let th = &sc["thresholds"];
    let pk = report["peaks"].as_array().and_then(|a| a.iter().find(|p| !c["source_peak_id"].is_null() && p["id"] == c["source_peak_id"])).unwrap_or(&null);
    let on = |v: &Value| truthy(v) && v.as_f64() != Some(-2.0);
    let h = c["height"].as_f64();
    let mut out = Vec::new();
    if on(&m["absolute_height"]) {
        if let (Some(t), Some(h)) = (th["heterozygous_height"].as_f64(), h) {
            if h < t {
                out.push("LPH");
            }
        }
    }
    if on(&m["high_height"]) {
        if let (Some(t), Some(h)) = (th["high_height"].as_f64(), h) {
            if h > t {
                out.push("MPH");
            }
        }
    }
    if on(&m["broad"]) {
        if let Some(t) = th["width_bp"].as_f64() {
            if pk["morphology_bp"]["fwhm_bp"].as_f64().unwrap_or(0.0) > t {
                out.push("BD");
            }
        }
    }
    if truthy(&pk["offscaled"]) {
        out.push("OS");
    }
    out
}

/// Text lines of an allele label per the Labels tab (web `labelLines`): up to four values, stopping at "None".
pub fn label_lines(c: &Value, report: &Value, ladder: bool, labels: &Value, reasons: &std::collections::HashMap<String, String>) -> Vec<String> {
    let edited = truthy(&c["user_edited"]);
    let kind = if ladder {
        "ladder"
    } else if edited {
        "custom"
    } else {
        "assigned"
    };
    let mut out: Vec<String> = Vec::new();
    for k in labels[kind].as_array().into_iter().flatten() {
        let k = k.as_str().unwrap_or("none");
        if k == "none" {
            break;
        }
        let v = match k {
            "alleleCall" => js(&c["label"]),
            "size" => c["size_bp"].as_f64().map(|b| format!("{b:.2}")).unwrap_or_default(),
            "height" => format!("{}", js_round(c["height"].as_f64().unwrap_or(0.0)) as i64),
            "area" => report["peaks"]
                .as_array()
                .and_then(|a| a.iter().find(|p| p["id"] == c["source_peak_id"]))
                .map(|p| if p["area"].is_null() { String::new() } else { js(&p["area"]) })
                .unwrap_or_default(),
            "dataPoint" => format!("{}", js_round(c["scan"].as_f64().unwrap_or(0.0)) as i64),
            "aeReason" => {
                if kind == "custom" {
                    reasons.get(&js(&c["id"])).cloned().unwrap_or_default()
                } else {
                    String::new()
                }
            }
            _ => String::new(),
        };
        let prefix = LABEL_TYPES.iter().find(|t| t.0 == k).map(|t| t.2).unwrap_or("");
        out.push(if truthy(&labels["prefixes"]) { format!("{prefix} {v}") } else { v });
    }
    if out.is_empty() {
        out.push(js(&c["label"]));
    }
    if truthy(&labels["editType"]) && edited {
        let added = c["flags"].as_array().is_some_and(|f| f.iter().any(|x| x == "user_added"));
        out[0] += if added { " (added)" } else { " (changed)" };
    }
    if truthy(&labels["pqvTrigger"]) {
        let tr = triggers(c, report);
        if !tr.is_empty() {
            out[0] += &format!(" {}", tr.join(","));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_valid_and_xml_round_trip() {
        for s in factory() {
            let v = validate(&s).unwrap();
            assert_eq!(v, s, "factory {} changes on validate", s["name"]);
            let back = from_xml(&to_xml(&s)).unwrap();
            assert_eq!(back, s, "XML round trip of {}", s["name"]);
        }
    }

    #[test]
    fn colors() {
        assert_eq!(color_from_xml("-16777216").unwrap(), "#000000");
        assert_eq!(color_from_xml("#A0b0C0").unwrap(), "#a0b0c0");
        for c in DEFAULT_COLORS.split(',') {
            assert_eq!(color_to_xml(&color_from_xml(c).unwrap()), c);
        }
    }

    #[test]
    fn validate_rejects() {
        let mut s = base("x", "");
        s["display"]["start"] = json!(-1);
        assert!(validate(&s).is_err());
        let mut s = base("x", "");
        s["display"]["allDyes"] = json!(false);
        s["display"]["end"] = json!(0);
        assert_eq!(validate(&s).unwrap_err(), MSG_END_GT_START);
        let mut s = base("a<b", "");
        s["name"] = json!("a<b");
        assert!(validate(&s).is_err());
        let mut s = base("x", "");
        s["display"]["panes"] = json!(40.4);
        assert_eq!(validate(&s).unwrap()["display"]["panes"], json!(16));
    }
}

#[cfg(test)]
mod web_parity {
    use super::*;
    /// OT_PS_WEB=<json written by the browser run of plotsettings.js>, OT_PS_XMLS=<XML samples>: factory settings,
    /// XML export, validation and import identical to the web version.
    #[test]
    fn same_as_web() {
        let Ok(path) = std::env::var("OT_PS_WEB") else { return };
        let w: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let f = factory();
        assert_eq!(Value::Array(f.clone()), w["factory"], "factory");
        for (i, s) in f.iter().enumerate() {
            assert_eq!(to_xml(s), w["xml"][i].as_str().unwrap(), "xml {i}");
            assert_eq!(from_xml(&to_xml(s)).unwrap(), w["back"][i], "back {i}");
        }
        let mut v = base("Тест & 1", "описание <x>");
        v["display"]["panes"] = json!(5.4);
        v["display"]["layout"] = json!("overlay");
        v["display"]["dyeScales"]["blue"] = json!(2.5);
        v["display"]["customColors"][0] = json!("#123456");
        v["labels"]["assigned"] = json!(["size", "height", "none", "alleleCall"]);
        v["labels"]["prefixes"] = json!(true);
        v["display"]["allDyes"] = json!(false);
        v["display"]["start"] = json!(75.5);
        v["display"]["end"] = json!(460);
        v["junk"] = json!(1);
        v["display"]["lastView"] = json!({"a": 1});
        let vv = validate(&v).unwrap();
        assert_eq!(vv, w["variant"]);
        assert_eq!(to_xml(&vv), w["variantXml"].as_str().unwrap());
        let xmls: Vec<String> = serde_json::from_str(&std::fs::read_to_string(std::env::var("OT_PS_XMLS").unwrap()).unwrap()).unwrap();
        for (i, x) in xmls.iter().enumerate() {
            match from_xml(x) {
                Ok(s) => assert_eq!(s, w["imports"][i]["ok"], "import {i}"),
                Err(e) => assert!(w["imports"][i]["err"].is_string(), "import {i}: native error {e}, web {}", w["imports"][i]),
            }
        }
    }
}
