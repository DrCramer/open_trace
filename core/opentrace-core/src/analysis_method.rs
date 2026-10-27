//! Port of `genemapper_open/analysis_method.py`: the HID analysis method (validation and normalization),
//! translation into OpenTrace engine inputs, GeneMapper XML import/export, and the request dispatcher.
//!
//! Methods are `serde_json::Value` objects (key order preserved) so that the same fields, int/float
//! distinction and error messages as the Python reference are kept.
use roxmltree::Node;
use serde_json::{Map, Value};
use std::path::Path;

pub type Res<T> = Result<T, String>;

pub const DYES: [&str; 6] = ["blue", "green", "yellow", "red", "purple", "orange"];
const REPEAT_KEYS: [&str; 4] = ["3", "4", "5", "6"];
const REPEAT_FIELDS: [&str; 10] = [
    "cutoff", "minus_a_ratio", "minus_a_from", "minus_a_to", "minus_stutter_ratio", "minus_stutter_from",
    "minus_stutter_to", "plus_stutter_ratio", "plus_stutter_from", "plus_stutter_to",
];
/// Dye order of the Python `_XML_DYE` mapping (used for element order in GeneMapper XML).
const XML_DYE_ORDER: [&str; 6] = ["blue", "green", "yellow", "red", "orange", "purple"];
const TOP_KEYS: [&str; 9] = [
    "schema_version", "name", "description", "instrument", "allele", "peak_detector", "peak_quality", "sq_gq", "fixed",
];
const WEIGHTS: [&str; 11] = ["bd", "bin", "ovl", "spk", "acc", "cc", "an", "lph", "mph", "os", "phr"];
const DEFAULT_DYE_CHANNELS: [(&str, &str); 6] = [
    ("blue", "DATA1"), ("green", "DATA2"), ("yellow", "DATA3"), ("red", "DATA4"),
    ("orange", "DATA105"), ("purple", "DATA106"),
];
const SIZE_CALLING: [(&str, &str); 5] = [
    ("local_southern", "Local Southern"),
    ("lsq2", "Least Square Second Order"),
    ("lsq3", "Least Square Third Order"),
    ("cubic_spline", "Cubic Spline Interpolation"),
    ("global_southern", "Global Southern"),
];
const CALIBRATION_MODEL: [(&str, &str); 5] = [
    ("local_southern", "local_rational"),
    ("lsq2", "lsq2"),
    ("lsq3", "lsq3"),
    ("cubic_spline", "natural_cubic"),
    ("global_southern", "global_southern"),
];

fn smoothing_passes(name: &str) -> Option<i64> {
    match name {
        "none" => Some(0),
        "light" => Some(2),
        "heavy" => Some(8),
        _ => None,
    }
}

fn lookup<'a>(table: &'a [(&str, &str)], key: &str) -> Option<&'a str> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

fn xml_dye(dye: &str) -> &'static str {
    match dye {
        "blue" => "Blue",
        "green" => "Green",
        "yellow" => "Yellow",
        "red" => "Red",
        "orange" => "Orange",
        _ => "Purple",
    }
}

fn xml_repeat(repeat: &str) -> &'static str {
    match repeat {
        "3" => "triNucleotide",
        "4" => "tetriNucleotide",
        "5" => "pentaNucleotide",
        _ => "hexNucleotide",
    }
}

// ----------------------------------------------------------------- Python-compatible formatting

/// Python `repr(float)`: shortest round-trip digits, fixed notation for -4 <= exponent < 16.
pub fn py_float_repr(x: f64) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0.0".into() } else { "0.0".into() };
    }
    let sci = format!("{:e}", x.abs());
    let (mant, exp) = sci.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let sign = if x < 0.0 { "-" } else { "" };
    if (-4..16).contains(&exp) {
        let body = if exp >= 0 {
            let int_len = (exp + 1) as usize;
            if digits.len() <= int_len {
                format!("{}{}", digits, "0".repeat(int_len - digits.len()) + ".0")
            } else {
                format!("{}.{}", &digits[..int_len], &digits[int_len..])
            }
        } else {
            format!("0.{}{}", "0".repeat((-exp - 1) as usize), digits)
        };
        format!("{}{}", sign, body)
    } else {
        let frac = if digits.len() > 1 { format!(".{}", &digits[1..]) } else { String::new() };
        format!("{}{}{}e{}{:02}", sign, &digits[..1], frac, if exp < 0 { "-" } else { "+" }, exp.abs())
    }
}

/// Python `f'{x:g}'` (six significant digits, trailing zeros removed).
pub fn py_format_g(x: f64) -> String {
    if x == 0.0 {
        return if x.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    if !x.is_finite() {
        return py_float_repr(x);
    }
    let sci = format!("{:.5e}", x);
    let (mant, exp) = sci.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    if (-4..6).contains(&exp) {
        let decimals = (5 - exp).max(0) as usize;
        let s = format!("{:.*}", decimals, x);
        strip_zeros(&s)
    } else {
        let mant = strip_zeros(mant);
        format!("{}e{}{:02}", mant, if exp < 0 { "-" } else { "+" }, exp.abs())
    }
}

fn strip_zeros(s: &str) -> String {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s.to_string()
    }
}

/// Python `repr(str)` for the simple strings used in messages.
fn py_str_repr(s: &str) -> String {
    if s.contains('\'') && !s.contains('"') {
        format!("\"{}\"", s)
    } else {
        format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
    }
}

fn py_list_repr(items: &[String]) -> String {
    let parts: Vec<String> = items.iter().map(|s| py_str_repr(s)).collect();
    format!("[{}]", parts.join(", "))
}

fn sorted(items: &[&str]) -> Vec<String> {
    let mut v: Vec<String> = items.iter().map(|s| s.to_string()).collect();
    v.sort();
    v
}

/// Python truthiness for JSON values.
fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|x| x != 0.0).unwrap_or(true),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

// ----------------------------------------------------------------- validators

/// Python bound literal: int literal or float literal (repr).
#[derive(Clone, Copy)]
enum Bound {
    I(i64),
    F(f64),
}

impl Bound {
    fn value(self) -> f64 {
        match self {
            Bound::I(i) => i as f64,
            Bound::F(f) => f,
        }
    }
    fn repr(self) -> String {
        match self {
            Bound::I(i) => i.to_string(),
            Bound::F(f) => py_float_repr(f),
        }
    }
}

fn check_num(v: &Value, where_: &str, lo: Bound, hi: Bound) -> Res<f64> {
    let x = match v {
        Value::Number(n) => n.as_f64().ok_or_else(|| format!("{}: finite number required", where_))?,
        _ => return Err(format!("{}: finite number required", where_)),
    };
    if !x.is_finite() {
        return Err(format!("{}: finite number required", where_));
    }
    if !(lo.value() <= x && x <= hi.value()) {
        return Err(format!("{}: value must be in [{}, {}]", where_, lo.repr(), hi.repr()));
    }
    Ok(x)
}

fn check_int(v: &Value, where_: &str, lo: i64, hi: i64) -> Res<i64> {
    let i = match v {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i
            } else {
                let x = n.as_f64().unwrap_or(f64::NAN);
                if x.is_nan() {
                    return Err("cannot convert float NaN to integer".into());
                }
                if x.is_infinite() {
                    return Err("cannot convert float infinity to integer".into());
                }
                if x != x.trunc() {
                    return Err(format!("{}: integer required", where_));
                }
                x as i64
            }
        }
        _ => return Err(format!("{}: integer required", where_)),
    };
    if !(lo <= i && i <= hi) {
        return Err(format!("{}: value must be in [{}, {}]", where_, lo, hi));
    }
    Ok(i)
}

fn check_bool(v: &Value, where_: &str) -> Res<bool> {
    match v {
        Value::Bool(b) => Ok(*b),
        _ => Err(format!("{}: true or false required", where_)),
    }
}

fn check_keys(v: &Value, keys: &[&str], where_: &str) -> Res<()> {
    let key_list = sorted(keys);
    match v {
        Value::Object(map) => {
            let missing: Vec<&str> = keys.iter().copied().filter(|k| !map.contains_key(*k)).collect();
            let extra: Vec<&str> = map.keys().map(|s| s.as_str()).filter(|k| !keys.contains(k)).collect();
            if missing.is_empty() && extra.is_empty() {
                return Ok(());
            }
            Err(format!(
                "{}: fields {} required (missing {}, unknown {})",
                where_,
                py_list_repr(&key_list),
                py_list_repr(&sorted(&missing)),
                py_list_repr(&sorted(&extra))
            ))
        }
        _ => Err(format!(
            "{}: fields {} required (missing {}, unknown {})",
            where_,
            py_list_repr(&key_list),
            py_list_repr(&key_list),
            py_list_repr(&[])
        )),
    }
}

fn obj_mut(v: &mut Value) -> &mut Map<String, Value> {
    v.as_object_mut().expect("validated object")
}

fn repeat_row(values: &[(&str, f64)]) -> Value {
    let mut row = Map::new();
    for key in REPEAT_FIELDS {
        row.insert(key.to_string(), Value::from(0.0));
    }
    for (k, v) in values {
        row.insert(k.to_string(), Value::from(*v));
    }
    Value::Object(row)
}

fn dye_map(values: &[(&str, Value)]) -> Value {
    let mut m = Map::new();
    for (k, v) in values {
        m.insert(k.to_string(), v.clone());
    }
    Value::Object(m)
}

/// FACTORY (application mode 2, HID) with the same key order as the Python literal.
pub fn factory_value() -> Value {
    let mut repeats = Map::new();
    repeats.insert("3".into(), repeat_row(&[]));
    repeats.insert("4".into(), repeat_row(&[("minus_stutter_from", 3.25), ("minus_stutter_to", 4.75)]));
    repeats.insert("5".into(), repeat_row(&[]));
    repeats.insert("6".into(), repeat_row(&[]));
    let range_filter: Vec<(&str, Value)> = DYES
        .iter()
        .map(|d| (*d, Value::Object(serde_json::from_str::<Map<String, Value>>(r#"{"from":0.0,"to":0.0}"#).unwrap())))
        .collect();
    let thresholds: Vec<(&str, Value)> = DYES.iter().map(|d| (*d, Value::from(50))).collect();
    let weights = dye_map(&[
        ("bd", Value::from(0.8)), ("bin", Value::from(0.8)), ("ovl", Value::from(0.8)), ("spk", Value::from(0.3)),
        ("acc", Value::from(0.0)), ("cc", Value::from(1.0)), ("an", Value::from(1.0)), ("lph", Value::from(0.3)),
        ("mph", Value::from(0.3)), ("os", Value::from(0.8)), ("phr", Value::from(0.3)),
    ]);
    serde_json::json!({
        "schema_version": 1,
        "name": "OpenTrace HID",
        "description": "",
        "instrument": "",
        "allele": {
            "bin_set": null,
            "use_marker_specific_stutter": true,
            "repeats": Value::Object(repeats),
            "amelogenin_cutoff": 0.0,
            "range_filter": dye_map(&range_filter),
        },
        "peak_detector": {
            "analysis_range": {"full": true, "start": 0, "stop": 10000},
            "sizing_range": {"all": true, "start_bp": 0.0, "stop_bp": 1000.0},
            "smoothing": "light",
            "baseline_window": 51,
            "size_calling": "local_southern",
            "thresholds": dye_map(&thresholds),
            "min_half_width": 2,
            "polynomial_degree": 3,
            "peak_window": 15,
            "slope_start": 0.0,
            "slope_end": 0.0,
            "normalization": false,
        },
        "peak_quality": {
            "lph_homozygous": 200.0, "lph_heterozygous": 100.0, "mph": 5000.0, "phr": 0.7, "bd_bp": 1.5,
            "max_alleles": 2, "max_alleles_y": 1,
            "ladder_spike": {"enabled": true, "cutoff": 0.2},
            "sample_spike": true,
        },
        "sq_gq": {
            "weights": weights,
            "sq_weight": 0.5, "ladder_spike": 1, "ladder_offscale": 1,
            "sq": {"pass": 0.75, "low": 0.25},
            "gq": {"pass": 0.75, "low": 0.25},
        },
        "fixed": {
            "str_peak_window": 9, "ladder_cutoff": 0.03, "bleedthrough_height_ratio": 0.05,
            "bleedthrough_scan_distance": 2, "profile_pullup_threshold": 0.5, "spike_max_width_bp": 0.4,
        },
    })
}

pub fn factory_method(name: Option<&str>) -> Res<Value> {
    let mut m = factory_value();
    if let Some(n) = name {
        obj_mut(&mut m).insert("name".into(), Value::from(n));
    }
    load_analysis_method(&m)
}

/// Python `load_analysis_method`: validate a method mapping and return a normalized copy.
pub fn load_analysis_method(source: &Value) -> Res<Value> {
    let mut m = source.clone();
    check_keys(&m, &TOP_KEYS, "method")?;
    {
        let sv = &m["schema_version"];
        let is_int_one = matches!(sv, Value::Number(n) if n.is_i64() && n.as_i64() == Some(1));
        if !is_int_one {
            return Err("method.schema_version must be 1".into());
        }
    }
    let name_val = m["name"].clone();
    let name = match &name_val {
        Value::String(s) => s.clone(),
        _ => return Err("method.name: 1..240 characters without control or markup characters".into()),
    };
    let stripped = name.trim();
    if !name_ok(stripped) || name != stripped {
        return Err("method.name: 1..240 characters without control or markup characters".into());
    }
    for key in ["description", "instrument"] {
        match &m[key] {
            Value::String(s) if s.chars().count() <= 2000 => {}
            _ => return Err(format!("method.{}: text up to 2000 characters required", key)),
        }
    }
    normalize_allele(&mut m)?;
    normalize_peak_detector(&mut m)?;
    normalize_peak_quality(&mut m)?;
    normalize_sq_gq(&mut m)?;
    normalize_fixed(&mut m)?;
    Ok(m)
}

/// NAME_RE = ^[^\x00-\x1f<>&"\\/]{1,240}$ applied to the stripped name.
fn name_ok(s: &str) -> bool {
    let n = s.chars().count();
    (1..=240).contains(&n)
        && s.chars().all(|c| {
            !(c as u32 <= 0x1f) && !matches!(c, '<' | '>' | '&' | '"' | '\\' | '/')
        })
}

fn set_num(obj: &mut Value, key: &str, x: f64) {
    obj_mut(obj).insert(key.to_string(), Value::from(x));
}

fn set_int(obj: &mut Value, key: &str, x: i64) {
    obj_mut(obj).insert(key.to_string(), Value::from(x));
}

fn normalize_allele(m: &mut Value) -> Res<()> {
    let allele_keys = ["bin_set", "use_marker_specific_stutter", "repeats", "amelogenin_cutoff", "range_filter"];
    check_keys(&m["allele"], &allele_keys, "allele")?;
    {
        let bin_set = &m["allele"]["bin_set"];
        let bad = match bin_set {
            Value::Null => false,
            Value::String(s) => !(1..=255).contains(&s.chars().count()),
            _ => true,
        };
        if bad {
            return Err("allele.bin_set: name or null required".into());
        }
    }
    let usms = m["allele"]["use_marker_specific_stutter"].clone();
    check_bool(&usms, "allele.use_marker_specific_stutter")?;
    check_keys(&m["allele"]["repeats"], &REPEAT_KEYS, "allele.repeats")?;
    // Python iterates the input mapping in its own key order.
    let repeat_keys: Vec<String> = m["allele"]["repeats"].as_object().unwrap().keys().cloned().collect();
    for repeat in &repeat_keys {
        let where_ = format!("allele.repeats.{}", repeat_name(repeat));
        let row_val = m["allele"]["repeats"][repeat.as_str()].clone();
        check_keys(&row_val, &REPEAT_FIELDS, &where_)?;
        for key in REPEAT_FIELDS {
            let hi = if key.ends_with("ratio") || key.ends_with("cutoff") { Bound::I(1) } else { Bound::I(100) };
            let x = check_num(&m["allele"]["repeats"][repeat.as_str()][key], &format!("{}.{}", where_, key), Bound::I(0), hi)?;
            set_num(&mut m["allele"]["repeats"][repeat.as_str()], key, x);
        }
        for kind in ["minus_a", "minus_stutter", "plus_stutter"] {
            let from = f64_of(&m["allele"]["repeats"][repeat.as_str()][format!("{}_from", kind)]);
            let to = f64_of(&m["allele"]["repeats"][repeat.as_str()][format!("{}_to", kind)]);
            if from > to {
                return Err(format!("{}: {} From must not exceed To", where_, kind));
            }
        }
    }
    let acm = m["allele"]["amelogenin_cutoff"].clone();
    let x = check_num(&acm, "allele.amelogenin_cutoff", Bound::I(0), Bound::I(1))?;
    set_num(&mut m["allele"], "amelogenin_cutoff", x);
    check_keys(&m["allele"]["range_filter"], &DYES, "allele.range_filter")?;
    let dye_keys: Vec<String> = m["allele"]["range_filter"].as_object().unwrap().keys().cloned().collect();
    for dye in &dye_keys {
        let dye = dye.as_str();
        let where_ = format!("allele.range_filter.{}", dye);
        let rng_val = m["allele"]["range_filter"][dye].clone();
        check_keys(&rng_val, &["from", "to"], &where_)?;
        let f = check_num(&m["allele"]["range_filter"][dye]["from"], &format!("{}.from", where_), Bound::I(0), Bound::I(100000))?;
        let t = check_num(&m["allele"]["range_filter"][dye]["to"], &format!("{}.to", where_), Bound::I(0), Bound::I(100000))?;
        set_num(&mut m["allele"]["range_filter"][dye], "from", f);
        set_num(&mut m["allele"]["range_filter"][dye], "to", t);
        if f > t {
            return Err(format!("{}: From must not exceed To", where_));
        }
    }
    Ok(())
}

fn repeat_name(repeat: &str) -> &'static str {
    match repeat {
        "3" => "tri",
        "4" => "tetra",
        "5" => "penta",
        _ => "hexa",
    }
}

fn normalize_peak_detector(m: &mut Value) -> Res<()> {
    let keys = [
        "analysis_range", "sizing_range", "smoothing", "baseline_window", "size_calling", "thresholds",
        "min_half_width", "polynomial_degree", "peak_window", "slope_start", "slope_end", "normalization",
    ];
    check_keys(&m["peak_detector"], &keys, "peak_detector")?;
    check_keys(&m["peak_detector"]["analysis_range"], &["full", "start", "stop"], "peak_detector.analysis_range")?;
    let full = m["peak_detector"]["analysis_range"]["full"].clone();
    check_bool(&full, "analysis_range.full")?;
    for key in ["start", "stop"] {
        let v = m["peak_detector"]["analysis_range"][key].clone();
        let x = check_int(&v, &format!("analysis_range.{}", key), 0, 10_000_000)?;
        set_int(&mut m["peak_detector"]["analysis_range"], key, x);
    }
    let ar = &m["peak_detector"]["analysis_range"];
    if ar["stop"].as_i64() <= ar["start"].as_i64() {
        return Err("analysis_range: 'Stop Point' must be greater than 'Start Point'".into());
    }
    check_keys(&m["peak_detector"]["sizing_range"], &["all", "start_bp", "stop_bp"], "peak_detector.sizing_range")?;
    let all = m["peak_detector"]["sizing_range"]["all"].clone();
    check_bool(&all, "sizing_range.all")?;
    for key in ["start_bp", "stop_bp"] {
        let v = m["peak_detector"]["sizing_range"][key].clone();
        let x = check_num(&v, &format!("sizing_range.{}", key), Bound::I(0), Bound::I(100000))?;
        set_num(&mut m["peak_detector"]["sizing_range"], key, x);
    }
    let sr = &m["peak_detector"]["sizing_range"];
    if sr["stop_bp"].as_f64() <= sr["start_bp"].as_f64() {
        return Err("sizing_range: 'Stop Size' must be greater than 'Start Size'".into());
    }
    let smoothing = m["peak_detector"]["smoothing"].clone();
    let sname = smoothing.as_str().unwrap_or("");
    if smoothing_passes(sname).is_none() {
        return Err("peak_detector.smoothing: none, light or heavy required".into());
    }
    let bw = m["peak_detector"]["baseline_window"].clone();
    let bw = check_int(&bw, "peak_detector.baseline_window", 3, 4001)?;
    set_int(&mut m["peak_detector"], "baseline_window", bw);
    if bw % 2 == 0 {
        return Err("peak_detector.baseline_window must be odd".into());
    }
    let calling = m["peak_detector"]["size_calling"].clone();
    match calling.as_str() {
        Some(c) if lookup(&SIZE_CALLING, c).is_some() => {}
        _ => return Err("peak_detector.size_calling: unknown method".into()),
    }
    check_keys(&m["peak_detector"]["thresholds"], &DYES, "peak_detector.thresholds")?;
    for dye in DYES {
        let v = m["peak_detector"]["thresholds"][dye].clone();
        let x = check_int(&v, &format!("threshold {}", dye), 1, 1_000_000)?;
        set_int(&mut m["peak_detector"]["thresholds"], dye, x);
    }
    let v = m["peak_detector"]["min_half_width"].clone();
    let x = check_int(&v, "peak_detector.min_half_width", 2, 99)?;
    set_int(&mut m["peak_detector"], "min_half_width", x);
    let v = m["peak_detector"]["polynomial_degree"].clone();
    let x = check_int(&v, "peak_detector.polynomial_degree", 2, 99)?;
    set_int(&mut m["peak_detector"], "polynomial_degree", x);
    let v = m["peak_detector"]["peak_window"].clone();
    let x = check_int(&v, "peak_detector.peak_window", 5, 101)?;
    set_int(&mut m["peak_detector"], "peak_window", x);
    if x % 2 == 0 {
        return Err("peak_detector.peak_window must be an odd number".into());
    }
    if m["peak_detector"]["polynomial_degree"].as_i64().unwrap() > x {
        return Err("Polynomial Degree cannot be greater than Peak Window Size.".into());
    }
    let v = m["peak_detector"]["slope_start"].clone();
    let x = check_num(&v, "peak_detector.slope_start", Bound::I(0), Bound::F(1e12))?;
    set_num(&mut m["peak_detector"], "slope_start", x);
    let v = m["peak_detector"]["slope_end"].clone();
    let x = check_num(&v, "peak_detector.slope_end", Bound::F(-1e12), Bound::I(0))?;
    set_num(&mut m["peak_detector"], "slope_end", x);
    let v = m["peak_detector"]["normalization"].clone();
    check_bool(&v, "peak_detector.normalization")?;
    Ok(())
}

fn normalize_peak_quality(m: &mut Value) -> Res<()> {
    let keys = [
        "lph_homozygous", "lph_heterozygous", "mph", "phr", "bd_bp", "max_alleles", "max_alleles_y",
        "ladder_spike", "sample_spike",
    ];
    check_keys(&m["peak_quality"], &keys, "peak_quality")?;
    for key in ["lph_homozygous", "lph_heterozygous", "mph"] {
        let v = m["peak_quality"][key].clone();
        let x = check_num(&v, &format!("peak_quality.{}", key), Bound::I(0), Bound::F(1e9))?;
        set_num(&mut m["peak_quality"], key, x);
    }
    let v = m["peak_quality"]["phr"].clone();
    let x = check_num(&v, "peak_quality.phr", Bound::I(0), Bound::I(1))?;
    set_num(&mut m["peak_quality"], "phr", x);
    let v = m["peak_quality"]["bd_bp"].clone();
    let x = check_num(&v, "peak_quality.bd_bp", Bound::I(0), Bound::I(1000))?;
    set_num(&mut m["peak_quality"], "bd_bp", x);
    let v = m["peak_quality"]["max_alleles"].clone();
    let x = check_int(&v, "peak_quality.max_alleles", 1, 128)?;
    set_int(&mut m["peak_quality"], "max_alleles", x);
    let v = m["peak_quality"]["max_alleles_y"].clone();
    let x = check_int(&v, "peak_quality.max_alleles_y", 1, 128)?;
    set_int(&mut m["peak_quality"], "max_alleles_y", x);
    check_keys(&m["peak_quality"]["ladder_spike"], &["enabled", "cutoff"], "peak_quality.ladder_spike")?;
    let en = m["peak_quality"]["ladder_spike"]["enabled"].clone();
    check_bool(&en, "ladder_spike.enabled")?;
    let v = m["peak_quality"]["ladder_spike"]["cutoff"].clone();
    let x = check_num(&v, "ladder_spike.cutoff", Bound::I(0), Bound::I(1))?;
    set_num(&mut m["peak_quality"]["ladder_spike"], "cutoff", x);
    let v = m["peak_quality"]["sample_spike"].clone();
    check_bool(&v, "peak_quality.sample_spike")?;
    Ok(())
}

fn normalize_sq_gq(m: &mut Value) -> Res<()> {
    let keys = ["weights", "sq_weight", "ladder_spike", "ladder_offscale", "sq", "gq"];
    check_keys(&m["sq_gq"], &keys, "sq_gq")?;
    check_keys(&m["sq_gq"]["weights"], &WEIGHTS, "sq_gq.weights")?;
    for key in WEIGHTS {
        let v = m["sq_gq"]["weights"][key].clone();
        let x = check_num(&v, &format!("weight {}", key.to_uppercase()), Bound::I(0), Bound::I(1))?;
        set_num(&mut m["sq_gq"]["weights"], key, x);
    }
    let v = m["sq_gq"]["sq_weight"].clone();
    let x = check_num(&v, "sq_gq.sq_weight", Bound::I(0), Bound::I(1))?;
    set_num(&mut m["sq_gq"], "sq_weight", x);
    for key in ["ladder_spike", "ladder_offscale"] {
        let v = m["sq_gq"][key].clone();
        let x = check_int(&v, &format!("sq_gq.{}", key), 0, 1)?;
        set_int(&mut m["sq_gq"], key, x);
    }
    for key in ["sq", "gq"] {
        check_keys(&m["sq_gq"][key], &["pass", "low"], &format!("sq_gq.{}", key))?;
        let v = m["sq_gq"][key]["pass"].clone();
        let p = check_num(&v, &format!("{}.pass", key), Bound::I(0), Bound::I(1))?;
        set_num(&mut m["sq_gq"][key], "pass", p);
        let v = m["sq_gq"][key]["low"].clone();
        let l = check_num(&v, &format!("{}.low", key), Bound::I(0), Bound::I(1))?;
        set_num(&mut m["sq_gq"][key], "low", l);
        if l >= p {
            return Err(format!("{}: Low Quality Range must end below the Pass Range", key.to_uppercase()));
        }
    }
    Ok(())
}

fn normalize_fixed(m: &mut Value) -> Res<()> {
    let keys = [
        "str_peak_window", "ladder_cutoff", "bleedthrough_height_ratio", "bleedthrough_scan_distance",
        "profile_pullup_threshold", "spike_max_width_bp",
    ];
    check_keys(&m["fixed"], &keys, "fixed")?;
    let v = m["fixed"]["str_peak_window"].clone();
    let x = check_int(&v, "fixed.str_peak_window", 5, 101)?;
    set_int(&mut m["fixed"], "str_peak_window", x);
    if x % 2 == 0 {
        return Err("fixed.str_peak_window must be odd".into());
    }
    let v = m["fixed"]["ladder_cutoff"].clone();
    let x = check_num(&v, "fixed.ladder_cutoff", Bound::I(0), Bound::I(1))?;
    set_num(&mut m["fixed"], "ladder_cutoff", x);
    let v = m["fixed"]["bleedthrough_height_ratio"].clone();
    let x = check_num(&v, "fixed.bleedthrough_height_ratio", Bound::I(0), Bound::I(1))?;
    set_num(&mut m["fixed"], "bleedthrough_height_ratio", x);
    let v = m["fixed"]["bleedthrough_scan_distance"].clone();
    let x = check_int(&v, "fixed.bleedthrough_scan_distance", 0, 1000)?;
    set_int(&mut m["fixed"], "bleedthrough_scan_distance", x);
    let v = m["fixed"]["profile_pullup_threshold"].clone();
    let x = check_num(&v, "fixed.profile_pullup_threshold", Bound::I(0), Bound::I(1))?;
    set_num(&mut m["fixed"], "profile_pullup_threshold", x);
    let v = m["fixed"]["spike_max_width_bp"].clone();
    let x = check_num(&v, "fixed.spike_max_width_bp", Bound::I(0), Bound::I(100))?;
    set_num(&mut m["fixed"], "spike_max_width_bp", x);
    Ok(())
}

// ----------------------------------------------------------------- engine inputs

/// Settings OpenTrace cannot apply yet; analysis refuses them explicitly.
pub fn unsupported(method: &Value) -> Vec<String> {
    let calling = method["peak_detector"]["size_calling"].as_str().unwrap_or("");
    if lookup(&CALIBRATION_MODEL, calling).is_none() {
        vec![format!("size_calling_{}", calling)]
    } else {
        Vec::new()
    }
}

fn f64_of(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0)
}

/// Python `float(x)` for a JSON value (numbers and numeric strings).
fn py_float(v: &Value) -> Res<f64> {
    match v {
        Value::Number(n) => n.as_f64().ok_or_else(|| "float() conversion failed".to_string()),
        Value::String(s) => s.trim().parse::<f64>().map_err(|_| format!("could not convert string to float: {}", py_str_repr(s))),
        Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        _ => Err("float() argument must be a string or a real number".into()),
    }
}

/// Keyword settings for server.analyze() for this method.
pub fn analyze_settings(method: &Value, standard_bp: &[Value], dye_channels: Option<&Map<String, Value>>, standard_dye: &str) -> Res<Value> {
    let method = load_analysis_method(method)?;
    let bad = unsupported(&method);
    if !bad.is_empty() {
        return Err(format!("Analysis method settings not supported by OpenTrace: {}", bad.join(", ")));
    }
    let p = &method["peak_detector"];
    let f = &method["fixed"];
    let mut channels: Map<String, Value> = Map::new();
    for (d, c) in DEFAULT_DYE_CHANNELS {
        channels.insert(d.into(), Value::from(c));
    }
    if let Some(extra) = dye_channels {
        for (k, v) in extra {
            channels.insert(k.clone(), v.clone());
        }
    }
    let channel = |dye: &str| -> Res<String> {
        channels
            .get(dye)
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .ok_or_else(|| format!("KeyError: '{}'", dye))
    };
    let mut bp: Vec<f64> = Vec::new();
    for x in standard_bp {
        bp.push(py_float(x)?);
    }
    bp.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let sr = &p["sizing_range"];
    if !sr["all"].as_bool().unwrap_or(true) {
        let (lo, hi) = (f64_of(&sr["start_bp"]), f64_of(&sr["stop_bp"]));
        bp.retain(|&x| lo <= x && x <= hi);
    }
    if bp.len() < 4 {
        return Err("Sizing range leaves fewer than 4 size-standard fragments".into());
    }
    let ar = &p["analysis_range"];
    let analysis_range = if ar["full"].as_bool().unwrap_or(true) {
        Value::Null
    } else {
        Value::Array(vec![ar["start"].clone(), ar["stop"].clone()])
    };
    let mut channel_thresholds = Map::new();
    for d in DYES {
        if let Some(ch) = channels.get(d).and_then(|v| v.as_str()) {
            channel_thresholds.insert(ch.to_string(), Value::from(f64_of(&p["thresholds"][d])));
        }
    }
    let passes = smoothing_passes(p["smoothing"].as_str().unwrap_or("")).unwrap_or(0);
    let calling = p["size_calling"].as_str().unwrap_or("");
    let standard_channel = channel(standard_dye)?;
    let mut settings = Map::new();
    settings.insert("sizing_mode".into(), Value::from("auto"));
    settings.insert("analysis_engine".into(), Value::from("morphology_ratio"));
    settings.insert("standard_channel".into(), Value::from(standard_channel));
    settings.insert(
        "standard_bp".into(),
        Value::from(bp.iter().map(|x| py_format_g(*x)).collect::<Vec<_>>().join(",")),
    );
    settings.insert("quality_threshold".into(), method["sq_gq"]["sq"]["pass"].clone());
    settings.insert("height_mode".into(), Value::from("explicit"));
    settings.insert("calibration_model".into(), Value::from(lookup(&CALIBRATION_MODEL, calling).unwrap_or("")));
    settings.insert("num_extra_peaks".into(), Value::from(25));
    settings.insert("distance".into(), Value::from(8));
    settings.insert("max_residual_bp".into(), Value::from(5.0));
    settings.insert("threshold".into(), Value::from(f64_of(&p["thresholds"][standard_dye])));
    settings.insert("channel_thresholds".into(), Value::Object(channel_thresholds));
    settings.insert("analysis_range".into(), analysis_range);
    let detector = serde_json::json!({
        "baseline_window": p["baseline_window"],
        "smooth_passes": passes,
        "window": f["str_peak_window"],
        "standard_window": p["peak_window"],
        "min_width": f64_of(&p["min_half_width"]),
        "tau_begin": p["slope_start"],
        "tau_end": p["slope_end"],
        "degree": p["polynomial_degree"],
    });
    settings.insert("detector".into(), detector);
    if p["normalization"].as_bool().unwrap_or(false) {
        settings.insert("normalization".into(), Value::Bool(true));
    }
    if method["peak_quality"]["sample_spike"].as_bool().unwrap_or(false) {
        settings.insert("spike_definition".into(), spike_definition(&method));
    }
    settings.insert("pullup_definition".into(), pullup_definition(&method, &channels, standard_dye)?);
    Ok(Value::Object(settings))
}

pub fn spike_definition(method: &Value) -> Value {
    let ladder = &method["peak_quality"]["ladder_spike"];
    let ratio = if ladder["enabled"].as_bool().unwrap_or(false) { ladder["cutoff"].clone() } else { Value::from(1e12) };
    serde_json::json!({
        "schema_version": 2,
        "measurement_source": "prepared_signal",
        "scope": "non_reference_selected_channels",
        "morphology_enabled": false,
        "units": "scan",
        "min_height": 0,
        "exhalf_width_threshold": 3.5,
        "near_base_width_threshold": 5,
        "area_ratio_threshold": 0.7,
        "treat_overlap_as_spike": true,
        "near_base_fraction": 0.06,
        "max_base_height": 140,
        "adjustments": {
            "sample_cutoff": true,
            "sample_marker_envelope": true,
            "ladder_height_ratio": ratio,
        },
    })
}

pub fn pullup_definition(method: &Value, channels: &Map<String, Value>, standard_dye: &str) -> Res<Value> {
    let f = &method["fixed"];
    let tags: Vec<String> = DYES
        .iter()
        .filter_map(|d| channels.get(*d).and_then(|v| v.as_str()).map(|s| s.to_string()))
        .collect();
    let standard = channels
        .get(standard_dye)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("KeyError: '{}'", standard_dye))?
        .to_string();
    let mut pairs: Vec<Value> = Vec::new();
    for a in &tags {
        if *a == standard {
            continue;
        }
        for b in &tags {
            if b != a {
                pairs.push(serde_json::json!({"candidate_channel": a, "reference_channel": b}));
            }
        }
    }
    pairs.truncate(32);
    Ok(serde_json::json!({
        "schema_version": 1,
        "profile_threshold": f["profile_pullup_threshold"],
        "detector": {
            "algorithm": "static_ar_pairwise_body",
            "calling_policy": "explicit_ordered_channel_pairs",
            "phase": "after_spike_restoration_before_calling",
            "reference_order": "stable_scan_midpoint_binary_search",
            "max_scan_distance": f["bleedthrough_scan_distance"],
            "height_ratio_threshold": f["bleedthrough_height_ratio"],
            "channel_pairs": pairs,
        },
    }))
}

fn zero_repeat() -> Value {
    repeat_row(&[])
}

/// Allele tab values for one marker (by repeat and dye) as kit parameters.
pub fn kit_marker_parameters(method: &Value, repeat: i64, dye: &str) -> Value {
    let a = &method["allele"];
    let row = a["repeats"].get(repeat.to_string()).cloned().unwrap_or_else(zero_repeat);
    let rng = a["range_filter"].get(dye).cloned().unwrap_or_else(|| serde_json::json!({"from": 0.0, "to": 0.0}));
    let cutoff = if repeat == 9 { a["amelogenin_cutoff"].clone() } else { row["cutoff"].clone() };
    let (rf, rt) = (f64_of(&rng["from"]), f64_of(&rng["to"]));
    let clear_ranges = if rt > rf {
        Value::Array(vec![serde_json::json!({"min_bp": rng["from"], "max_bp": rng["to"]})])
    } else {
        Value::Array(Vec::new())
    };
    serde_json::json!({
        "ladder_cutoff_ratio": method["fixed"]["ladder_cutoff"],
        "cutoff_ratio": cutoff,
        "minus_a_ratio": row["minus_a_ratio"],
        "minus_a_min_bp": row["minus_a_from"],
        "minus_a_max_bp": row["minus_a_to"],
        "global_minus_stutter": {
            "ratio": row["minus_stutter_ratio"],
            "min_bp": row["minus_stutter_from"],
            "max_bp": row["minus_stutter_to"],
        },
        "global_plus_stutter": {
            "ratio": row["plus_stutter_ratio"],
            "min_bp": row["plus_stutter_from"],
            "max_bp": row["plus_stutter_to"],
        },
        "clear_ranges": clear_ranges,
    })
}

pub fn marker_quality_definition(method: &Value, kit: &Value) -> Value {
    let q = &method["peak_quality"];
    let w = &method["sq_gq"]["weights"];
    let weights = serde_json::json!({
        "allele_number": w["an"], "absolute_height": w["lph"], "high_height": w["mph"], "spike": w["spk"],
        "ratio": w["phr"], "broad": w["bd"], "offscale": w["os"], "overlap": w["ovl"], "binning": w["bin"],
    });
    let mut markers = Map::new();
    let mut amel: Value = Value::Null;
    for m in kit["markers"].as_array().map(|v| v.as_slice()).unwrap_or(&[]) {
        let name = m["name"].as_str().unwrap_or("").to_string();
        let kind = if matches!(name.to_uppercase().as_str(), "AMEL" | "AMELOGENIN") {
            amel = m["name"].clone();
            "amel"
        } else if truthy(m.get("y_marker")) {
            "y"
        } else {
            "autosomal"
        };
        let max_alleles = if kind == "y" { q["max_alleles_y"].clone() } else { q["max_alleles"].clone() };
        markers.insert(name, serde_json::json!({
            "kind": kind,
            "confidence_source": "bin_confidence_weight",
            "thresholds": {
                "max_alleles": max_alleles,
                "homozygous_height": q["lph_homozygous"],
                "heterozygous_height": q["lph_heterozygous"],
                "high_height": q["mph"],
                "ratio": q["phr"],
                "width_bp": q["bd_bp"],
            },
            "weights": weights.clone(),
            "flags": {"spike": false, "offscale": false, "overlap": false, "binning": false},
        }));
    }
    serde_json::json!({
        "schema_version": 1,
        "sample_sq": {"source": "base_quality"},
        "gq_thresholds": {"good": method["sq_gq"]["gq"]["pass"], "bad": method["sq_gq"]["gq"]["low"]},
        "amel_marker": amel,
        "control_mode": "none",
        "markers": Value::Object(markers),
    })
}

pub fn ladder_quality_definition(method: &Value, kit: &Value) -> Value {
    let q = &method["peak_quality"];
    let s = &method["sq_gq"];
    let spike_on = truthy(Some(&s["ladder_spike"])) && q["ladder_spike"]["enabled"].as_bool().unwrap_or(false);
    let offscale_on = truthy(Some(&s["ladder_offscale"]));
    let recipe = serde_json::json!({
        "min_height_ratio": 0.15,
        "spike_metric": 0,
        "spike_weight": if spike_on { 1.0 } else { 0.0 },
        "offscale_metric": 0,
        "offscale_weight": if offscale_on { 1.0 } else { 0.0 },
        "spacing": {
            "enabled": true,
            "max_distance_bp": 49,
            "curve": [{"distance_bp": 0, "fraction": 0.25, "interpolate": false}],
            "exceptions": [],
            "excluded_alleles": [],
        },
    });
    let mut markers = Map::new();
    for m in kit["markers"].as_array().map(|v| v.as_slice()).unwrap_or(&[]) {
        if truthy(m.get("ladder_labels")) {
            markers.insert(m["name"].as_str().unwrap_or("").to_string(), recipe.clone());
        }
    }
    serde_json::json!({
        "schema_version": 1,
        "calling_parameters_source": "ladder_matching_calls",
        "ladder_sq": {"source": "base_quality"},
        "sq_thresholds": {"good": s["sq"]["pass"], "bad": s["sq"]["low"]},
        "gq_thresholds": {"good": s["gq"]["pass"], "bad": s["gq"]["low"]},
        "markers": Value::Object(markers),
    })
}

// ----------------------------------------------------------------- GeneMapper XML

const NS: &str = "www.AppliedBiosystems.com";
const MAX_XML_BYTES: usize = 1024 * 1024;

fn local<'a>(name: &'a str) -> &'a str {
    name.rsplit('}').next().unwrap_or(name)
}

/// Direct element children by local name; the first occurrence wins (Python `_children`).
fn children<'a, 'i>(node: Node<'a, 'i>) -> Vec<(String, Node<'a, 'i>)> {
    node.children()
        .filter(|c| c.is_element())
        .map(|c| (local(c.tag_name().name()).to_string(), c))
        .collect()
}

fn child_of<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    children(node).into_iter().find(|(n, _)| n == name).map(|(_, c)| c)
}

/// Python `_text`: stripped text of a child, default when absent or empty-element.
fn text_of(node: Option<Node>, name: &str, default: Option<&str>) -> Option<String> {
    let child = node.and_then(|n| child_of(n, name));
    match child.and_then(|c| c.text()) {
        None => default.map(|d| d.to_string()),
        Some(t) => Some(t.trim().to_string()),
    }
}

fn num_of(node: Option<Node>, name: &str, default: f64) -> Res<f64> {
    match text_of(node, name, None) {
        None => Ok(default),
        Some(v) => v.trim().parse::<f64>().map_err(|_| format!("{}: number expected, got {}", name, py_str_repr(&v))),
    }
}

fn bool_of(node: Option<Node>, name: &str, default: bool) -> bool {
    match text_of(node, name, None) {
        None => default,
        Some(v) => v.to_lowercase() == "true",
    }
}

/// Python `int(float)` truncation.
fn trunc_int(x: f64) -> Res<i64> {
    if x.is_nan() {
        return Err("cannot convert float NaN to integer".into());
    }
    if x.is_infinite() {
        return Err("cannot convert float infinity to integer".into());
    }
    Ok(x.trunc() as i64)
}

pub fn parse_gm_xml(text: &str) -> Res<Value> {
    let raw = text.as_bytes();
    if raw.len() > MAX_XML_BYTES {
        return Err("Analysis method XML up to 1 MiB required".into());
    }
    if contains(raw, b"<!DOCTYPE") || contains(raw, b"<!ENTITY") {
        return Err("DTDs and entities are not accepted in analysis method XML".into());
    }
    let doc = roxmltree::Document::parse(text).map_err(|e| format!("Malformed XML: {}", e))?;
    let root = doc.root_element();
    if local(root.tag_name().name()) != "AnalysisMethodContainer" {
        return Err("AnalysisMethodContainer root element required".into());
    }
    let node = child_of(root, "xmlAnalysisMethod").ok_or("xmlAnalysisMethod element required")?;
    let mode = text_of(Some(node), "applicationMode", None);
    if let Some(m) = &mode {
        if m != "2" {
            return Err(format!("Only HID analysis methods (applicationMode 2) are supported, got {}", m));
        }
    }
    let mut m = factory_value();
    let name = text_of(Some(node), "analysisMethodName", None).filter(|s| !s.is_empty()).unwrap_or_else(|| "Imported method".into());
    let name: String = name.trim().chars().take(240).collect();
    obj_mut(&mut m).insert("name".into(), Value::from(name));
    let description = text_of(Some(node), "description", Some("")).unwrap_or_default();
    obj_mut(&mut m).insert("description".into(), Value::from(description));
    let instrument = text_of(Some(node), "instrument", Some("")).unwrap_or_default();
    obj_mut(&mut m).insert("instrument".into(), Value::from(instrument));

    let ac = child_of(node, "alleleCallerParameter");
    let pd = child_of(node, "peakDetectorParameter");
    let bl = child_of(node, "baselinerParameter");
    let sc = child_of(node, "sizeCallerParameter");
    if let Some(ac) = ac {
        let bin_set = text_of(Some(ac), "binSet", None).filter(|s| !s.is_empty());
        m["allele"]["bin_set"] = bin_set.map(Value::from).unwrap_or(Value::Null);
        m["allele"]["use_marker_specific_stutter"] = Value::Bool(bool_of(Some(ac), "useMarkerStutterPercentage", true));
        for repeat in REPEAT_KEYS {
            let prefix = xml_repeat(repeat);
            let g = |suffix: &str, default: f64| num_of(Some(ac), &format!("{}{}", prefix, suffix), default);
            let row = &mut m["allele"]["repeats"][repeat];
            let fields: [(&str, f64); 10] = [
                ("cutoff", g("_cutOffValue", 0.0)?),
                ("minus_a_ratio", g("_minusARatio", 0.0)?),
                ("minus_a_from", g("_minusADistanceRight", 0.0)?),
                ("minus_a_to", g("_minusADistance", 0.0)?),
                ("minus_stutter_ratio", g("_stutterRatio", 0.0)?),
                ("minus_stutter_from", g("_stutterDistanceRight", 0.0)?),
                ("minus_stutter_to", g("_stutterDistance", 0.0)?),
                ("plus_stutter_ratio", g("_plus_stutterRatio", 0.0)?),
                ("plus_stutter_from", g("_plus_stutterDistanceRight", 0.0)?),
                ("plus_stutter_to", g("_plus_stutterDistance", 0.0)?),
            ];
            for (k, v) in fields {
                row[k] = Value::from(v);
            }
        }
        let amel = num_of(Some(ac), "amelogeninCutoff", 0.0)?;
        m["allele"]["amelogenin_cutoff"] = Value::from(amel);
        for dye in XML_DYE_ORDER {
            let word = xml_dye(dye);
            let from = num_of(Some(ac), &format!("clearRange{}DyeFrom", word), 0.0)?;
            let to = num_of(Some(ac), &format!("clearRange{}DyeTo", word), 0.0)?;
            m["allele"]["range_filter"][dye] = serde_json::json!({"from": from, "to": to});
        }
        let q = &mut m["peak_quality"];
        let phr = num_of(Some(ac), "warningPeakHeightRatioThreshold", f64_of(&q["phr"]))?;
        q["phr"] = Value::from(phr);
        let hom = num_of(Some(ac), "warningPeakHeightThresholdHomozygous", f64_of(&q["lph_homozygous"]))?;
        q["lph_homozygous"] = Value::from(hom);
        let het = num_of(Some(ac), "warningPeakHeightThresholdHeterozygous", f64_of(&q["lph_heterozygous"]))?;
        q["lph_heterozygous"] = Value::from(het);
        let mph = num_of(Some(ac), "warningHighPeakHeightThreshold", f64_of(&q["mph"]))?;
        q["mph"] = Value::from(mph);
        let ma = trunc_int(num_of(Some(ac), "warningMaxAlleleNumber", f64_of(&q["max_alleles"]))?)?;
        q["max_alleles"] = Value::from(ma);
        let may = trunc_int(num_of(Some(ac), "warningMaxAlleleNumberForYMarkers", f64_of(&q["max_alleles_y"]))?)?;
        q["max_alleles_y"] = Value::from(may);
        let bd = num_of(Some(ac), "warningPeakWidthThreshold", f64_of(&q["bd_bp"]))?;
        q["bd_bp"] = Value::from(bd);
        let enabled = bool_of(Some(ac), "spikeDetectionEnabled", true);
        let cutoff = num_of(Some(ac), "spikeLadderCutoffThreshold", 0.2)?;
        q["ladder_spike"] = serde_json::json!({"enabled": enabled, "cutoff": cutoff});
        q["sample_spike"] = Value::Bool(bool_of(Some(ac), "spikeDetectionForSampleEnabled", true));
        let f = &mut m["fixed"];
        let bh = num_of(Some(ac), "warningBleedthroughHeightThreshold", f64_of(&f["bleedthrough_height_ratio"]))?;
        f["bleedthrough_height_ratio"] = Value::from(bh);
        let bs = num_of(Some(ac), "warningBleedthroughScanThreshold", f64_of(&f["bleedthrough_scan_distance"]))?;
        f["bleedthrough_scan_distance"] = Value::from(trunc_int(bs)?);
        let sm = num_of(Some(ac), "spikeMaxPeakWidthThreshold", f64_of(&f["spike_max_width_bp"]))?;
        f["spike_max_width_bp"] = Value::from(sm);
    }
    let full = bool_of(Some(node), "analysisFullRange", true);
    let start = trunc_int(num_of(Some(node), "analysisStartPt", 0.0)?)?;
    let stop = trunc_int(num_of(Some(node), "analysisStopPt", 10000.0)?)?;
    m["peak_detector"]["analysis_range"] = serde_json::json!({"full": full, "start": start, "stop": stop});
    let all = bool_of(Some(node), "sizeCallAllSizes", true);
    let start_bp = num_of(Some(node), "sizeCallStartSize", 0.0)?;
    let stop_bp = num_of(Some(node), "sizeCallStopSize", 1000.0)?;
    m["peak_detector"]["sizing_range"] = serde_json::json!({"all": all, "start_bp": start_bp, "stop_bp": stop_bp});
    let smooth = trunc_int(num_of(Some(node), "smoothValue", 2.0)?)?;
    let smoothing = match smooth {
        0 => "none",
        2 => "light",
        8 => "heavy",
        _ => {
            return Err(format!("smoothValue {} is not None (0), Light (2) or Heavy (8)", smooth));
        }
    };
    m["peak_detector"]["smoothing"] = Value::from(smoothing);
    if let Some(bl) = bl {
        let w = trunc_int(num_of(Some(bl), "windowSize", 51.0)?)?;
        m["peak_detector"]["baseline_window"] = Value::from(w);
    }
    if let Some(sc) = sc {
        let name = text_of(Some(sc), "sizeCallMethod", Some("Local Southern")).unwrap_or_default();
        match SIZE_CALLING.iter().find(|(_, label)| *label == name) {
            Some((key, _)) => m["peak_detector"]["size_calling"] = Value::from(*key),
            None => return Err(format!("Unknown size calling method {}", py_str_repr(&name))),
        }
    }
    if let Some(pd) = pd {
        let degree = trunc_int(num_of(Some(pd), "degree", 3.0)?)?;
        m["peak_detector"]["polynomial_degree"] = Value::from(degree);
        let window = trunc_int(num_of(Some(pd), "window", 15.0)?)?;
        m["peak_detector"]["peak_window"] = Value::from(window);
        let tetri = trunc_int(num_of(Some(pd), "windowTetri", 9.0)?)?;
        m["fixed"]["str_peak_window"] = Value::from(tetri);
        let minw = trunc_int(num_of(Some(pd), "minWidth", 2.0)?)?;
        m["peak_detector"]["min_half_width"] = Value::from(minw);
        let tb = num_of(Some(pd), "tauB", 0.0)?;
        m["peak_detector"]["slope_start"] = Value::from(tb);
        let te = num_of(Some(pd), "tauE", 0.0)?;
        m["peak_detector"]["slope_end"] = Value::from(te);
        for dye in XML_DYE_ORDER {
            let word = xml_dye(dye);
            let current = f64_of(&m["peak_detector"]["thresholds"][dye]);
            let v = trunc_int(num_of(Some(pd), &format!("minHeight{}", word), current)?)?;
            m["peak_detector"]["thresholds"][dye] = Value::from(v);
        }
        m["peak_detector"]["normalization"] = Value::Bool(bool_of(Some(pd), "applyNormalization", false));
    }
    let qv = child_of(node, "qualityValueProcessParameter");
    if let Some(qv) = qv {
        let mapping: [(&str, &str); 11] = [
            ("bd", "broadPeakQualityWeight"), ("bin", "binningQualityWeight"), ("ovl", "overlapQualityWeight"),
            ("spk", "spikeMarkerPqvQualityWeight"), ("acc", "amelCCQualityWeight"), ("cc", "controlAlleleQualityWeight"),
            ("an", "alleleNumberMarkerPqvQualityWeight"), ("lph", "peakAbsoluteHeightQualityWeight"),
            ("mph", "highPeakHeightQualityWeight"), ("os", "offscaleQualityWeight"),
            ("phr", "peakHeightRatioQualityWeight"),
        ];
        for (key, name) in mapping {
            let current = f64_of(&m["sq_gq"]["weights"][key]);
            let v = num_of(Some(qv), name, current)?;
            m["sq_gq"]["weights"][key] = Value::from(v);
        }
        let sqw = num_of(Some(qv), "BroadPeakSQPqvQualityWeight", f64_of(&m["sq_gq"]["sq_weight"]))?;
        m["sq_gq"]["sq_weight"] = Value::from(sqw);
        let ls = num_of(Some(qv), "alSpikeMarkerPqvQualityWeight", 1.0)?;
        m["sq_gq"]["ladder_spike"] = Value::from(i64::from(ls >= 0.5));
        let lo = num_of(Some(qv), "alOffscaleMarkerPqvQualityWeight", 1.0)?;
        m["sq_gq"]["ladder_offscale"] = Value::from(i64::from(lo >= 0.5));
        let pull = num_of(Some(qv), "profilePullupQualityThreshold", 0.5)?;
        m["fixed"]["profile_pullup_threshold"] = Value::from(pull);
    }
    for (name, entry) in children(node) {
        if name != "thresholdMap" {
            continue;
        }
        let key = text_of(Some(entry), "Key", Some("")).unwrap_or_default();
        let qt = child_of(entry, "QualityThreshold");
        let target = if key.starts_with("Sizing") {
            Some("sq")
        } else if key.starts_with("Genotype") {
            Some("gq")
        } else {
            None
        };
        if let (Some(target), Some(qt)) = (target, qt) {
            let pass = num_of(Some(qt), "goodIndicator", 0.75)?;
            let low = num_of(Some(qt), "badIndicator", 0.25)?;
            m["sq_gq"][target] = serde_json::json!({"pass": pass, "low": low});
        }
    }
    load_analysis_method(&m)
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Python `_fmt`: bools as true/false, ints as-is, floats as repr.
fn fmt_bool(b: bool) -> String {
    if b { "true".into() } else { "false".into() }
}

fn fmt_value(v: &Value) -> String {
    match v {
        Value::Bool(b) => fmt_bool(*b),
        Value::Number(n) => match n.as_i64() {
            Some(i) => i.to_string(),
            None => py_float_repr(n.as_f64().unwrap_or(0.0)),
        },
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Minimal element tree written like `xml.etree.ElementTree.tostring`.
struct Elem {
    tag: String,
    attrs: Vec<(String, String)>,
    text: Option<String>,
    children: Vec<Elem>,
}

impl Elem {
    fn new(tag: &str) -> Elem {
        Elem { tag: tag.into(), attrs: Vec::new(), text: None, children: Vec::new() }
    }
    fn sub(&mut self, tag: &str, text: Option<String>) -> usize {
        let mut e = Elem::new(tag);
        e.text = text.filter(|t| !t.is_empty());
        self.children.push(e);
        self.children.len() - 1
    }
    fn child_mut(&mut self, i: usize) -> &mut Elem {
        &mut self.children[i]
    }
    fn write(&self, out: &mut String) {
        out.push('<');
        out.push_str(&self.tag);
        for (k, v) in &self.attrs {
            out.push(' ');
            out.push_str(k);
            out.push_str("=\"");
            out.push_str(&escape_attr(v));
            out.push('"');
        }
        if self.text.is_none() && self.children.is_empty() {
            out.push_str(" />");
            return;
        }
        out.push('>');
        if let Some(t) = &self.text {
            out.push_str(&escape_text(t));
        }
        for c in &self.children {
            c.write(out);
        }
        out.push_str("</");
        out.push_str(&self.tag);
        out.push('>');
    }
}

fn escape_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\r', "&#13;")
        .replace('\n', "&#10;")
        .replace('\t', "&#09;")
}

/// Write the HID subset as an `AnalysisMethodContainer` XML document.
pub fn to_gm_xml(method: &Value) -> Res<String> {
    let m = load_analysis_method(method)?;
    let a = &m["allele"];
    let p = &m["peak_detector"];
    let q = &m["peak_quality"];
    let s = &m["sq_gq"];
    let f = &m["fixed"];
    let mut root = Elem::new("AnalysisMethodContainer");
    root.attrs = vec![
        ("xmlns".into(), NS.into()),
        ("xmlns:xsi".into(), "http://www.w3.org/2001/XMLSchema-instance".into()),
        ("xsi:type".into(), "AnalysisMethodContainer".into()),
    ];
    root.sub("appVersion", Some("1.0".into()));
    root.sub("fileVersion", Some("1.0".into()));
    let n_idx = root.sub("xmlAnalysisMethod", None);
    let n = root.child_mut(n_idx);
    n.sub("analysisMethodName", Some(fmt_value(&m["name"])));
    n.sub("originationProduct", Some("OpenTrace".into()));
    if truthy(Some(&m["description"])) {
        n.sub("description", Some(fmt_value(&m["description"])));
    }
    if truthy(Some(&m["instrument"])) {
        n.sub("instrument", Some(fmt_value(&m["instrument"])));
    }
    n.sub("applicationMode", Some("2".into()));
    n.sub("signalProcessMode", Some("2".into()));
    n.sub("analysisFullRange", Some(fmt_value(&p["analysis_range"]["full"])));
    n.sub("analysisStartPt", Some(fmt_value(&p["analysis_range"]["start"])));
    n.sub("analysisStopPt", Some(fmt_value(&p["analysis_range"]["stop"])));
    n.sub("sizeCallAllSizes", Some(fmt_value(&p["sizing_range"]["all"])));
    n.sub("sizeCallStartSize", Some(fmt_value(&p["sizing_range"]["start_bp"])));
    n.sub("sizeCallStopSize", Some(fmt_value(&p["sizing_range"]["stop_bp"])));
    let smooth_name = p["smoothing"].as_str().unwrap_or("");
    n.sub("smoothValue", Some(smoothing_passes(smooth_name).unwrap_or(0).to_string()));
    for (key, label) in [("sq", "Sizing Quality (SQ)"), ("gq", "Genotype Quality (GQ)")] {
        let t = n.sub("thresholdMap", None);
        let tm = n.child_mut(t);
        tm.sub("Key", Some(label.into()));
        let qt = tm.sub("QualityThreshold", None);
        let qe = tm.child_mut(qt);
        qe.sub("goodIndicator", Some(fmt_value(&s[key]["pass"])));
        qe.sub("badIndicator", Some(fmt_value(&s[key]["low"])));
        qe.sub("name", Some(label.into()));
        qe.sub("result", Some("Pass".into()));
    }
    let b = n.sub("baselinerParameter", None);
    let be = n.child_mut(b);
    be.sub("AnalysisParameter", None);
    be.sub("windowSize", Some(fmt_value(&p["baseline_window"])));
    be.sub("spikeSize", Some("5".into()));
    let d = n.sub("peakDetectorParameter", None);
    let de = n.child_mut(d);
    de.sub("AnalysisParameter", None);
    de.sub("degree", Some(fmt_value(&p["polynomial_degree"])));
    de.sub("window", Some(fmt_value(&p["peak_window"])));
    de.sub("degreeTetri", Some("3".into()));
    de.sub("windowTetri", Some(fmt_value(&f["str_peak_window"])));
    for dye in XML_DYE_ORDER {
        de.sub(&format!("minHeight{}", xml_dye(dye)), Some(fmt_value(&p["thresholds"][dye])));
    }
    de.sub("minWidth", Some(fmt_value(&p["min_half_width"])));
    de.sub("tauB", Some(fmt_value(&p["slope_start"])));
    de.sub("tauE", Some(fmt_value(&p["slope_end"])));
    de.sub("heightAutoDetermined", Some("false".into()));
    de.sub("applyNormalization", Some(fmt_value(&p["normalization"])));
    let sc = n.sub("sizeCallerParameter", None);
    let sce = n.child_mut(sc);
    sce.sub("AnalysisParameter", None);
    let calling = p["size_calling"].as_str().unwrap_or("");
    sce.sub("sizeCallMethod", Some(lookup(&SIZE_CALLING, calling).unwrap_or("").into()));
    let ac = n.sub("alleleCallerParameter", None);
    let ace = n.child_mut(ac);
    ace.sub("AnalysisParameter", None);
    for repeat in REPEAT_KEYS {
        let prefix = xml_repeat(repeat);
        let r = &a["repeats"][repeat];
        let pairs: [(&str, &str); 10] = [
            ("cutOffValue", "cutoff"), ("minusADistance", "minus_a_to"), ("minusADistanceRight", "minus_a_from"),
            ("minusARatio", "minus_a_ratio"), ("stutterDistance", "minus_stutter_to"),
            ("stutterDistanceRight", "minus_stutter_from"), ("stutterRatio", "minus_stutter_ratio"),
            ("plus_stutterDistance", "plus_stutter_to"), ("plus_stutterDistanceRight", "plus_stutter_from"),
            ("plus_stutterRatio", "plus_stutter_ratio"),
        ];
        for (suffix, key) in pairs {
            ace.sub(&format!("{}_{}", prefix, suffix), Some(fmt_value(&r[key])));
        }
    }
    ace.sub("useMarkerStutterPercentage", Some(fmt_value(&a["use_marker_specific_stutter"])));
    if truthy(Some(&a["bin_set"])) {
        ace.sub("binSet", Some(fmt_value(&a["bin_set"])));
    }
    ace.sub("amelogeninCutoff", Some(fmt_value(&a["amelogenin_cutoff"])));
    ace.sub("warningPeakHeightRatioThreshold", Some(fmt_value(&q["phr"])));
    ace.sub("warningPeakHeightThresholdHomozygous", Some(fmt_value(&q["lph_homozygous"])));
    ace.sub("warningPeakHeightThresholdHeterozygous", Some(fmt_value(&q["lph_heterozygous"])));
    ace.sub("warningHighPeakHeightThreshold", Some(fmt_value(&q["mph"])));
    ace.sub("warningMaxAlleleNumber", Some(fmt_value(&q["max_alleles"])));
    ace.sub("warningMaxAlleleNumberForYMarkers", Some(fmt_value(&q["max_alleles_y"])));
    ace.sub("warningPeakWidthThreshold", Some(fmt_value(&q["bd_bp"])));
    ace.sub("warningBleedthroughHeightThreshold", Some(fmt_value(&f["bleedthrough_height_ratio"])));
    ace.sub("warningBleedthroughScanThreshold", Some(fmt_value(&f["bleedthrough_scan_distance"])));
    ace.sub("spikeMaxPeakWidthThreshold", Some(fmt_value(&f["spike_max_width_bp"])));
    ace.sub("spikeLadderCutoffThreshold", Some(fmt_value(&q["ladder_spike"]["cutoff"])));
    ace.sub("spikeDetectionEnabled", Some(fmt_value(&q["ladder_spike"]["enabled"])));
    ace.sub("spikeDetectionForSampleEnabled", Some(fmt_value(&q["sample_spike"])));
    for dye in XML_DYE_ORDER {
        let word = xml_dye(dye);
        ace.sub(&format!("clearRange{}DyeFrom", word), Some(fmt_value(&a["range_filter"][dye]["from"])));
        ace.sub(&format!("clearRange{}DyeTo", word), Some(fmt_value(&a["range_filter"][dye]["to"])));
    }
    let qv = n.sub("qualityValueProcessParameter", None);
    let qve = n.child_mut(qv);
    qve.sub("AnalysisParameter", None);
    let weight_names: [(&str, &str); 11] = [
        ("bd", "broadPeakQualityWeight"), ("bin", "binningQualityWeight"), ("lph", "peakAbsoluteHeightQualityWeight"),
        ("mph", "highPeakHeightQualityWeight"), ("spk", "spikeMarkerPqvQualityWeight"),
        ("an", "alleleNumberMarkerPqvQualityWeight"), ("phr", "peakHeightRatioQualityWeight"),
        ("os", "offscaleQualityWeight"), ("cc", "controlAlleleQualityWeight"), ("ovl", "overlapQualityWeight"),
        ("acc", "amelCCQualityWeight"),
    ];
    for (key, name) in weight_names {
        qve.sub(name, Some(fmt_value(&s["weights"][key])));
    }
    qve.sub("BroadPeakSQPqvQualityWeight", Some(fmt_value(&s["sq_weight"])));
    qve.sub("profilePullupQualityThreshold", Some(fmt_value(&f["profile_pullup_threshold"])));
    qve.sub("alSpikeMarkerPqvQualityWeight", Some(py_float_repr(f64_of(&s["ladder_spike"]))));
    qve.sub("alOffscaleMarkerPqvQualityWeight", Some(py_float_repr(f64_of(&s["ladder_offscale"]))));
    let mut body = String::new();
    root.write(&mut body);
    Ok(format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{}", body))
}

// ----------------------------------------------------------------- request dispatcher

/// Python `method_request`: the /api/analysis-method dispatcher. `shipped_path` is the shipped methods JSON.
pub fn method_request(request: &Value, shipped_path: &Path) -> Res<Value> {
    let obj = request.as_object().ok_or("Analysis method request must be an object")?;
    let action = obj.get("action").and_then(|v| v.as_str()).unwrap_or("");
    match action {
        "factory" => {
            let name = obj.get("name").and_then(|v| v.as_str());
            Ok(serde_json::json!({"method": factory_method(name)?}))
        }
        "shipped" => {
            let text = std::fs::read_to_string(shipped_path).map_err(|e| e.to_string())?;
            let data: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            let mut out = Vec::new();
            for item in data["methods"].as_array().map(|v| v.as_slice()).unwrap_or(&[]) {
                out.push(load_analysis_method(item)?);
            }
            Ok(serde_json::json!({"methods": out}))
        }
        "validate" => {
            let method = load_analysis_method(obj.get("method").unwrap_or(&Value::Null))?;
            Ok(serde_json::json!({"method": method, "unsupported": unsupported(&method)}))
        }
        "import_xml" => {
            let xml = match obj.get("xml") {
                Some(Value::String(s)) => s.clone(),
                _ => return Err("Analysis method XML up to 1 MiB required".into()),
            };
            let method = parse_gm_xml(&xml)?;
            Ok(serde_json::json!({"method": method, "unsupported": unsupported(&method)}))
        }
        "export_xml" => {
            let xml = to_gm_xml(obj.get("method").unwrap_or(&Value::Null))?;
            Ok(serde_json::json!({"xml": xml}))
        }
        "analyze_settings" => {
            let dye = match obj.get("standard_dye") {
                Some(v) if truthy(Some(v)) => v.as_str().unwrap_or("").to_string(),
                _ => "orange".to_string(),
            };
            if !DYES.contains(&dye.as_str()) {
                return Err(format!("standard_dye must be one of {}", DYES.join(", ")));
            }
            let bp: Vec<Value> = match obj.get("standard_bp") {
                Some(v) if truthy(Some(v)) => v.as_array().cloned().unwrap_or_default(),
                _ => Vec::new(),
            };
            let channels = match obj.get("dye_channels") {
                Some(Value::Object(o)) => Some(o.clone()),
                Some(v) if truthy(Some(v)) => {
                    let kind = match v {
                        Value::String(_) => "str",
                        Value::Array(_) => "list",
                        Value::Bool(_) => "bool",
                        Value::Number(n) if n.is_f64() => "float",
                        _ => "int",
                    };
                    return Err(format!("TypeError: '{}' object is not a mapping", kind));
                }
                _ => None,
            };
            let settings = analyze_settings(
                obj.get("method").unwrap_or(&Value::Null),
                &bp,
                channels.as_ref(),
                &dye,
            )?;
            Ok(serde_json::json!({"settings": settings}))
        }
        _ => Err("action must be factory, validate, import_xml, export_xml or analyze_settings".into()),
    }
}
