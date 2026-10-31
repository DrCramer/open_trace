//! Port of `genemapper_open/server.py::analyze` and its helpers (the `/api/analyze` workflow).
//!
//! Channels are kept as JSON objects (`serde_json::Map`, insertion ordered) like the reference's dicts so that key order,
//! int/float types and aliasing-free copies match the Python output exactly.
use crate::abif::{AbifFile, Num};
use crate::abif_view::{channel_obj, directory, run_info, summary_head};
use crate::analysis::Calibration as Anchors;
use crate::calibration::{build_calibration, Calibration, MODEL_IDS};
use crate::profiles::{py_float_parse, strip};
use crate::sizing_residual::{find_peaks, match_size_standard, parse_anchors, parse_expected_bp};
use crate::sizing_value::{analyze_size_standard, fv, prepare_trace, sg_peaks_value, SizeParams};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

pub type Obj = Map<String, Value>;
pub type Res<T> = Result<T, String>;

pub const VERSION: &str = "0.57.0";

pub fn analysis_version() -> String {
    format!("OpenTrace/{}", VERSION)
}

fn real(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

fn is_int(v: &Value) -> bool {
    matches!(v, Value::Number(n) if n.is_i64() || n.is_u64())
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map_or(true, |f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn loads(s: &str) -> Res<Value> {
    crate::pyjson::loads(s, false).map_err(|e| e.message())
}

fn nums_of(values: &Value) -> Vec<Num> {
    values
        .as_array()
        .map(|a| a.iter().map(|v| if let Some(i) = v.as_i64() { Num::Int(i) } else { Num::Float(v.as_f64().unwrap_or(f64::NAN)) }).collect())
        .unwrap_or_default()
}

// ------------------------------------------------------------------------------------------ definitions

#[derive(Clone, Copy)]
pub enum DefKind {
    Spike,
    Pullup,
    SizingQuality,
}

/// `load_spike_definition` / `load_pullup_definition` / `load_sizing_quality_definition`, including the JSON-text path.
pub fn load_definition(source: &Value, kind: DefKind) -> Res<Value> {
    let (too_long, dup, constant, bounded) = match kind {
        DefKind::Spike => ("Spike definition exceeds16KiB", "Duplicate spike JSON field: ", "Nonfinite JSON constant: ", "Bounded spike definition JSON required"),
        DefKind::Pullup => ("Pull-up definition exceeds16KiB", "Duplicate pull-up field: ", "Nonfinite pull-up JSON constant: ", "Bounded pull-up definition JSON required"),
        DefKind::SizingQuality => ("Sizing quality definition exceeds16KiB", "Duplicate sizing quality JSON field: ", "Nonfinite JSON constant: ", "Bounded sizing quality definition required"),
    };
    if let Value::String(text) = source {
        if text.len() > 16384 {
            return Err(too_long.into());
        }
        let parsed = crate::pyjson::loads_opts(text, true, true).map_err(|e| match e {
            crate::pyjson::LoadErr::Dup(k) => format!("{}{}", dup, k),
            crate::pyjson::LoadErr::Const(c) => format!("{}{}", constant, c),
            crate::pyjson::LoadErr::Recursion(_) => bounded.to_string(),
            crate::pyjson::LoadErr::Decode(m) => m,
        })?;
        return match kind {
            DefKind::Spike => crate::spike::load_spike_definition_unsized(&parsed),
            DefKind::Pullup => crate::pullup::load_pullup_definition_unsized(&parsed),
            DefKind::SizingQuality => crate::post_sizing_quality::load_sizing_quality_definition_unsized(&parsed),
        };
    }
    match kind {
        DefKind::Spike => crate::spike::load_spike_definition(source),
        DefKind::Pullup => crate::pullup::load_pullup_definition(source),
        DefKind::SizingQuality => crate::post_sizing_quality::load_sizing_quality_definition(source),
    }
}

// ------------------------------------------------------------------------------------------ _pipeline_expected

pub fn pipeline_expected(value: &Value) -> Res<Vec<f64>> {
    let list: Vec<Value> = match value {
        Value::String(s) => {
            if s.chars().count() > 4096 {
                return Err("Expected fragment text is too long".into());
            }
            let t = strip(s);
            let is_sep = |c: char| c == ',' || c == ';' || crate::profiles::py_isspace_pub(c);
            let chars: Vec<char> = t.chars().collect();
            let mut parts: Vec<String> = Vec::new();
            let (mut cur, mut splits, mut i) = (String::new(), 0, 0);
            while i < chars.len() {
                if splits < 32 && is_sep(chars[i]) {
                    while i < chars.len() && is_sep(chars[i]) {
                        i += 1;
                    }
                    parts.push(std::mem::take(&mut cur));
                    splits += 1;
                } else {
                    cur.push(chars[i]);
                    i += 1;
                }
            }
            parts.push(cur);
            let mut out = Vec::new();
            for p in &parts {
                match py_float_parse(p) {
                    Some(f) => out.push(fv(f)),
                    None => return Err("Expected fragment sizes must be numbers".into()),
                }
            }
            out
        }
        Value::Array(a) => a.clone(),
        _ => return Err("Enter 3..32 explicit increasing fragment sizes".into()),
    };
    if !(3..=32).contains(&list.len()) {
        return Err("Enter 3..32 explicit increasing fragment sizes".into());
    }
    let mut sizes = Vec::new();
    for x in &list {
        match x {
            Value::Number(n) if n.as_f64().map_or(false, |f| f.is_finite() && 0.0 < f && f <= 1_000_000.0) => sizes.push(n.as_f64().unwrap()),
            _ => return Err("Expected fragment sizes must be finite and in (0, 1000000]".into()),
        }
    }
    if sizes.windows(2).any(|w| w[1] - w[0] < 1e-6) {
        return Err("Expected fragment sizes must increase with gaps >=0.000001bp".into());
    }
    Ok(sizes)
}

// ------------------------------------------------------------------------------------------ detector settings

pub struct Detector {
    pub obj: Obj,
    pub degree: i64,
    pub baseline_window: i64,
    pub smooth_passes: i64,
    pub window: i64,
    pub standard_window: i64,
    pub min_width: f64,
    pub tau_begin: f64,
    pub tau_end: f64,
}

fn default_detector_obj() -> Obj {
    let mut o = Obj::new();
    o.insert("baseline_window".into(), json!(51));
    o.insert("smooth_passes".into(), json!(2));
    o.insert("window".into(), json!(9));
    o.insert("standard_window".into(), json!(15));
    o.insert("min_width".into(), json!(2.0));
    o.insert("tau_begin".into(), json!(0.0));
    o.insert("tau_end".into(), json!(0.0));
    o
}

/// `_detector_settings(detector)`; the `bool` tells whether the result equals `DETECTOR_DEFAULTS`.
pub fn detector_settings(detector: &Value) -> Res<(Detector, bool)> {
    let owned;
    let detector = match detector {
        Value::String(s) => {
            owned = loads(s)?;
            &owned
        }
        d => d,
    };
    if detector.is_null() {
        return Ok((
            Detector { obj: default_detector_obj(), degree: 3, baseline_window: 51, smooth_passes: 2, window: 9, standard_window: 15, min_width: 2.0, tau_begin: 0.0, tau_end: 0.0 },
            true,
        ));
    }
    let keys = ["baseline_window", "smooth_passes", "window", "standard_window", "min_width", "tau_begin", "tau_end"];
    let map = match detector {
        Value::Object(m) => m,
        _ => return Err("Detector settings need exactly: baseline_window, min_width, smooth_passes, standard_window, tau_begin, tau_end, window (and optional degree)".into()),
    };
    let rest: std::collections::BTreeSet<&str> = map.keys().map(|k| k.as_str()).filter(|k| *k != "degree").collect();
    let want: std::collections::BTreeSet<&str> = keys.iter().cloned().collect();
    if rest != want {
        return Err("Detector settings need exactly: baseline_window, min_width, smooth_passes, standard_window, tau_begin, tau_end, window (and optional degree)".into());
    }
    let mut out = Obj::new();
    out.insert("degree".into(), json!(3));
    let mut degree = 3i64;
    if let Some(d) = map.get("degree") {
        match d {
            Value::Number(n) if n.is_i64() && (1..=99).contains(&n.as_i64().unwrap()) => {
                degree = n.as_i64().unwrap();
                out.insert("degree".into(), json!(degree));
            }
            _ => return Err("detector.degree must be an integer 1..99".into()),
        }
    }
    let mut ints = [0i64; 4];
    for (i, key) in ["baseline_window", "smooth_passes", "window", "standard_window"].iter().enumerate() {
        match &map[*key] {
            Value::Number(n) if n.is_i64() => {
                ints[i] = n.as_i64().unwrap();
                out.insert((*key).into(), json!(ints[i]));
            }
            _ => return Err(format!("detector.{}: integer required", key)),
        }
    }
    if !(3..=4001).contains(&ints[0]) || ints[0] % 2 == 0 {
        return Err("detector.baseline_window must be odd, 3..4001".into());
    }
    if !(0..=32).contains(&ints[1]) {
        return Err("detector.smooth_passes must be 0..32".into());
    }
    for (key, v) in [("window", ints[2]), ("standard_window", ints[3])] {
        if !(5..=101).contains(&v) || v % 2 == 0 {
            return Err(format!("detector.{} must be odd, 5..101", key));
        }
    }
    let mut fl = [0f64; 3];
    for (i, key) in ["min_width", "tau_begin", "tau_end"].iter().enumerate() {
        match &map[*key] {
            Value::Number(n) if n.as_f64().map_or(false, |f| f.is_finite()) => {
                fl[i] = n.as_f64().unwrap();
                out.insert((*key).into(), fv(fl[i]));
            }
            _ => return Err(format!("detector.{}: finite number required", key)),
        }
    }
    let is_default = false; // `degree` is always present in the result, so it never equals the defaults
    Ok((
        Detector { obj: out, degree, baseline_window: ints[0], smooth_passes: ints[1], window: ints[2], standard_window: ints[3], min_width: fl[0], tau_begin: fl[1], tau_end: fl[2] },
        is_default,
    ))
}

fn channel_thresholds(value: &Value) -> Res<Obj> {
    let owned;
    let value = match value {
        Value::String(s) => {
            owned = loads(s)?;
            &owned
        }
        v => v,
    };
    if value.is_null() {
        return Ok(Obj::new());
    }
    let map = match value {
        Value::Object(m) if m.len() <= 16 => m,
        _ => return Err("channel_thresholds must map DATA tags to RFU".into()),
    };
    let mut out = Obj::new();
    for (tag, rfu) in map {
        let ok = tag.len() > 4 && tag.len() <= 10 && tag.starts_with("DATA") && tag[4..].bytes().all(|b| b.is_ascii_digit());
        if !ok {
            return Err("channel_thresholds keys must be DATA tags".into());
        }
        match rfu {
            Value::Number(n) if n.as_f64().map_or(false, |f| f.is_finite() && 0.0 < f && f <= 1e12) => {
                out.insert(tag.clone(), fv(n.as_f64().unwrap()));
            }
            _ => return Err(format!("channel_thresholds.{} must be in (0, 1e12]", tag)),
        }
    }
    Ok(out)
}

/// `_parse_analysis_range(value)` for the HTTP layer: `None` or `[start, stop]`.
pub fn parse_analysis_range(value: &Value) -> Res<Option<(i64, i64)>> {
    let owned;
    let value = match value {
        Value::Null => return Ok(None),
        Value::String(s) if s.is_empty() => return Ok(None),
        Value::String(s) => {
            owned = loads(s)?;
            &owned
        }
        v => v,
    };
    match value {
        Value::Array(a) if a.len() == 2 && a.iter().all(is_int) => {
            let (s, e) = (a[0].as_i64().unwrap_or(-1), a[1].as_i64().unwrap_or(-1));
            if 0 <= s && s < e && e <= 100_000_000 {
                return Ok(Some((s, e)));
            }
            Err("Analysis range needs integer [start, stop] data points with stop greater than start".into())
        }
        _ => Err("Analysis range needs integer [start, stop] data points with stop greater than start".into()),
    }
}

fn apply_analysis_range(ch: &mut Obj, range: (i64, i64)) {
    let (mut start, stop) = (range.0 as usize, range.1 as usize);
    let values = ch["values"].as_array().cloned().unwrap_or_default();
    let n = values.len();
    if start >= n {
        start = 0;
        ch.insert("analysis_range_clamped".into(), json!(true));
    }
    let stop = stop.min(n - 1);
    ch.insert("raw_values".into(), Value::Array(values.clone()));
    let mut out: Vec<Value> = Vec::with_capacity(n);
    out.extend(std::iter::repeat(values[start].clone()).take(start));
    out.extend_from_slice(&values[start..=stop]);
    out.extend(std::iter::repeat(values[stop].clone()).take(n - stop - 1));
    ch.insert("values".into(), Value::Array(out));
    ch.insert("analysis_range".into(), json!([start, stop]));
}

// ------------------------------------------------------------------------------------------ sizing override

fn apply_sizing_override(sizing: &mut Obj, override_: &Value, mode: &str, engine: &str) -> Res<Anchors> {
    let owned;
    let override_ = match override_ {
        Value::String(s) => {
            owned = loads(s)?;
            &owned
        }
        v => v,
    };
    if mode != "auto" || engine != "morphology_ratio" {
        return Err("SQ override requires automatic morphology sizing".into());
    }
    let o = match override_ {
        Value::Object(o) if o.keys().all(|k| k == "anchors" || k == "reason") && o.contains_key("anchors") => o,
        _ => return Err("SQ override needs anchors and an optional reason".into()),
    };
    let anchors = match &o["anchors"] {
        Value::Array(a) if (3..=64).contains(&a.len()) => a,
        _ => return Err("SQ override needs 3..64 [scan, bp] anchors".into()),
    };
    let mut pairs: Vec<(f64, f64)> = Vec::new();
    for item in anchors {
        let bad = || "Each SQ override anchor must be [scan, bp]".to_string();
        match item {
            Value::Array(t) if t.len() == 2 => match (real(&t[0]), real(&t[1])) {
                (Some(a), Some(b)) if a.is_finite() && b.is_finite() => pairs.push((a, b)),
                _ => return Err(bad()),
            },
            _ => return Err(bad()),
        }
    }
    pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.partial_cmp(&b.1).unwrap()));
    if pairs.windows(2).any(|w| w[1].0 <= w[0].0 || w[1].1 <= w[0].1) {
        return Err("SQ override anchors must increase in both scan and bp".into());
    }
    let reason = match o.get("reason") {
        None => json!(""),
        Some(Value::String(s)) if s.chars().count() <= 500 => json!(s),
        _ => return Err("SQ override reason must be text up to 500 characters".into()),
    };
    sizing.insert(
        "sq_override".into(),
        json!({"anchors": pairs.iter().map(|&(a, b)| json!([fv(a), fv(b)])).collect::<Vec<_>>(), "reason": reason, "sq": 1.0,
            "automatic_status": sizing.get("status").cloned().unwrap_or(Value::Null),
            "automatic_reasons": sizing.get("reasons").cloned().unwrap_or(json!([])),
            "automatic_base_quality": sizing.get("base_quality").cloned().unwrap_or(Value::Null)}),
    );
    sizing.insert("status".into(), json!("accepted"));
    sizing.insert("reasons".into(), json!([]));
    Anchors::new(&pairs)
}

// ------------------------------------------------------------------------------------------ normalization

fn sample_file_normalization(sample: &AbifFile) -> (String, Option<f64>) {
    let mut family: Vec<String> = Vec::new();
    for (name, n) in [("HCFG", 3), ("HCFG", 2), ("MODL", 1), ("MCHN", 1)] {
        if let Some(crate::abif::Value::Str(s)) = sample.value(name, n) {
            family.push(s);
        }
    }
    let family = family.join(" ");
    if !(family.contains("3500") || family.contains("3200")) {
        return ("N/A".into(), None);
    }
    let factor = match sample.value("NrmS", 1) {
        Some(crate::abif::Value::Int(i)) => Some(i as f64),
        Some(crate::abif::Value::Float(f)) => Some(f),
        _ => None,
    };
    match factor {
        Some(f) if f.is_finite() && f > 0.0 => ("Yes".into(), Some(f)),
        _ => ("No".into(), None),
    }
}

// ------------------------------------------------------------------------------------------ 310 matrix

fn apply_matrix(sample: &AbifFile, channels: &mut [Obj], matrix: &Value) -> Res<Option<Value>> {
    use crate::matrix::{embedded_matrix, invert, is_310, multicomponent, validate_matrix};
    let parsed;
    let matrix: &Value = match matrix {
        Value::String(s) => {
            if strip(s).is_empty() {
                &Value::Null
            } else {
                parsed = loads(s)?;
                &parsed
            }
        }
        m => m,
    };
    if !is_310(sample) {
        return Ok(None);
    }
    let (inverse, source) = if !matrix.is_null() {
        let m = validate_matrix(matrix).map_err(|e| e.message())?;
        (Some(invert(&m).map_err(|e| e.message())?), "library")
    } else {
        let e = embedded_matrix(sample);
        let src = if e.is_some() { "sample_file" } else { "" };
        (e, src)
    };
    let inverse = match inverse {
        Some(i) => i,
        None => return Err("Matrix not found (MNF): 310 sample files need a matrix (Matrix column of the Samples table)".into()),
    };
    let n = inverse.len();
    let mut idx: Vec<usize> = (0..channels.len())
        .filter(|&i| {
            let c = &channels[i];
            c["group"] == json!("raw") && c["dye_index"].as_i64().map_or(false, |d| d != 0 && d as usize <= n)
        })
        .collect();
    idx.sort_by_key(|&i| channels[i]["dye_index"].as_i64().unwrap());
    if idx.len() != n {
        return Err("Matrix has wrong dimension for this sample file".into());
    }
    let int16 = idx.iter().all(|&i| {
        let c = &channels[i];
        sample.tags.get(&("DATA".to_string(), c["number"].as_i64().unwrap() as i32)).map_or(false, |t| t.element_type == 4)
    });
    let traces: Vec<Vec<i64>> = idx
        .iter()
        .map(|&i| channels[i]["values"].as_array().unwrap().iter().map(|v| v.as_i64().unwrap_or(0)).collect())
        .collect();
    let mixed = multicomponent(&traces, &inverse, int16).map_err(|e| e.message())?;
    for (&i, values) in idx.iter().zip(mixed) {
        channels[i].insert("values".into(), Value::Array(values.into_iter().map(|v| json!(v)).collect()));
        channels[i].insert("multicomponented".into(), json!(true));
    }
    Ok(Some(json!({"instrument": "310", "source": source, "dyes": n, "applied": true})))
}

// ------------------------------------------------------------------------------------------ morphology channel

fn public_sg_peaks(peaks: &[Value], raw_values: &[Value]) -> Res<Vec<Value>> {
    let mut out = Vec::new();
    for p in peaks {
        let mut d = p.as_object().cloned().ok_or("peak mapping required")?;
        let scan = real(&d["scan"]).ok_or("peak scan required")?.trunc() as i64;
        let raw = if scan >= 0 { raw_values.get(scan as usize) } else { raw_values.get((raw_values.len() as i64 + scan).max(0) as usize) };
        let raw = raw.ok_or("list index out of range")?.clone();
        let width = d.get("fwhm_scan").cloned().ok_or("'fwhm_scan'")?;
        d.insert("raw_height".into(), raw);
        d.insert("width_scans".into(), width);
        out.push(Value::Object(d));
    }
    Ok(out)
}

const FLOAT_TRACE: &str = "Trace values must be signed 32-bit integers";

fn int_values(values: &[Value]) -> Option<Vec<i64>> {
    values.iter().map(|v| v.as_i64()).collect()
}

fn measure_morphology_channel(ch: &mut Obj, threshold: f64, spike: Option<&Value>, det: &Detector, scale: Option<f64>) -> Res<()> {
    let values = ch["values"].as_array().cloned().unwrap_or_default();
    if values.len() < 9 {
        ch.insert("peaks".into(), json!([]));
        ch.insert("processing".into(), json!({"status": "skipped", "reason": "Trace is shorter than SG window9"}));
        return Ok(());
    }
    let ints = int_values(&values).ok_or(FLOAT_TRACE)?;
    let prepared = prepare_trace(&ints, det.baseline_window, 5, 1, det.smooth_passes)?;
    let mut signal = prepared.signal.clone();
    if let Some(s) = scale {
        signal = signal.iter().map(|&v| (v as f64 * s).trunc() as i64).collect();
    }
    let (mut peaks, dg) = sg_peaks_value(&signal, threshold, det.window, det.degree, det.min_width, det.tau_begin, det.tau_end, &[])?;
    if let Some(def) = spike {
        use crate::spike::{measure_features, signal_checked, SpikeDefinition};
        let measurement: Vec<i64> = if def["schema_version"].as_i64().unwrap_or(0) >= 3 { prepared.corrected.clone() } else { signal.clone() };
        let mv = Value::Array(measurement.iter().map(|&v| json!(v)).collect());
        let samples = signal_checked(&mv)?;
        if peaks.len() > 4096 {
            return Err("At most4096 SG peaks required".into());
        }
        let sd = SpikeDefinition::from_validated(def);
        for p in peaks.iter_mut() {
            let features = measure_features(&samples, p, &sd)?;
            p.as_object_mut().unwrap().insert("spike_features".into(), features);
        }
    }
    let projected = crate::standard_wrapper::wrap_standard_peaks(&Value::Array(peaks), &json!(values.len()), &json!(0), &json!([]))?;
    let pub_peaks = public_sg_peaks(projected["peaks"].as_array().unwrap(), &values)?;
    ch.insert("peaks".into(), Value::Array(pub_peaks));
    ch.insert("analyzed_values".into(), Value::Array(signal.iter().map(|&v| json!(v)).collect()));
    ch.insert(
        "processing".into(),
        json!({"status": "measured", "preparation": prepared.parameters, "detector": dg,
            "coordinate_conversion": projected["diagnostics"].clone(), "height_mode": "explicit", "original_runtime_validated": false}),
    );
    Ok(())
}

// ------------------------------------------------------------------------------------------ analyze

fn kw<'a>(m: &'a Obj, key: &str) -> Option<&'a Value> {
    m.get(key)
}

fn str_of(v: Option<&Value>, default: &str) -> Res<String> {
    match v {
        None => Ok(default.to_string()),
        Some(Value::String(s)) => Ok(s.clone()),
        Some(_) => Err("string argument required".into()),
    }
}

/// Everything `analyze()` computes. `kw` carries the Python keyword arguments (absent = Python default).
pub fn analyze(raw: &[u8], filename: &str, kwargs: &Obj) -> Res<Value> {
    let null = Value::Null;
    let threshold_v = kw(kwargs, "threshold").cloned().unwrap_or(json!(100.0));
    let distance_v = kw(kwargs, "distance").cloned().unwrap_or(json!(8));
    let anchors_text = str_of(kw(kwargs, "anchors"), "")?;
    let sizing_mode = kw(kwargs, "sizing_mode").cloned().unwrap_or(json!(""));
    let standard_channel_v = kw(kwargs, "standard_channel").cloned().unwrap_or(json!(""));
    let standard_bp = kw(kwargs, "standard_bp").cloned().unwrap_or(json!(""));
    let max_residual_v = kw(kwargs, "max_residual_bp").cloned().unwrap_or(json!(5.0));
    let standard_start_v = kw(kwargs, "standard_start").cloned().unwrap_or(json!(0));
    let standard_end_v = kw(kwargs, "standard_end").cloned().unwrap_or(Value::Null);
    let calibration_model = kw(kwargs, "calibration_model").cloned().unwrap_or(json!("linear"));
    let analysis_engine = kw(kwargs, "analysis_engine").cloned().unwrap_or(json!("residual"));
    let height_mode = kw(kwargs, "height_mode").cloned().unwrap_or(json!("explicit"));
    let quality_threshold_v = kw(kwargs, "quality_threshold").cloned().unwrap_or(json!(0.75));
    let num_extra_v = kw(kwargs, "num_extra_peaks").cloned().unwrap_or(json!(25));
    let normalization_v = kw(kwargs, "normalization").cloned().unwrap_or(json!(false));
    let analysis_range: Option<(i64, i64)> = match kw(kwargs, "analysis_range") {
        None | Some(Value::Null) => None,
        Some(Value::Array(a)) if a.len() == 2 && a.iter().all(is_int) => Some((a[0].as_i64().unwrap_or(0), a[1].as_i64().unwrap_or(0))),
        Some(_) => return Err("analysis_range must be a pair of integers".into()),
    };
    let mut spike_definition = kw(kwargs, "spike_definition").cloned().unwrap_or(Value::Null);
    let mut sizing_quality_definition = kw(kwargs, "sizing_quality_definition").cloned().unwrap_or(Value::Null);
    let mut pullup_definition = kw(kwargs, "pullup_definition").cloned().unwrap_or(Value::Null);

    let (det, det_is_default) = detector_settings(kw(kwargs, "detector").unwrap_or(&null))?;
    let channel_thresholds = channel_thresholds(kw(kwargs, "channel_thresholds").unwrap_or(&null))?;
    let model_ok = matches!(&calibration_model, Value::String(s) if MODEL_IDS.contains(&s.as_str()));
    if !model_ok {
        return Err("Unknown calibration model".into());
    }
    let engine = match &analysis_engine {
        Value::String(s) if s == "residual" || s == "morphology_ratio" => s.clone(),
        _ => return Err("analysis_engine must be residual or morphology_ratio".into()),
    };
    let morphology = engine == "morphology_ratio";
    if !spike_definition.is_null() {
        spike_definition = load_definition(&spike_definition, DefKind::Spike)?;
        if !morphology {
            return Err("Basic spike classification requires morphology_ratio engine".into());
        }
    }
    if !sizing_quality_definition.is_null() {
        sizing_quality_definition = load_definition(&sizing_quality_definition, DefKind::SizingQuality)?;
        if !morphology {
            return Err("Post-sizing quality requires morphology_ratio engine".into());
        }
    }
    if !pullup_definition.is_null() {
        pullup_definition = load_definition(&pullup_definition, DefKind::Pullup)?;
        if !morphology {
            return Err("Explicit pull-up route requires morphology_ratio engine".into());
        }
    }
    let threshold = real(&threshold_v);
    let (mut quality_threshold, mut num_extra_peaks) = (0.0f64, 0i64);
    if morphology {
        if !matches!(&height_mode, Value::String(s) if s == "explicit" || s == "basic_standard_auto") {
            return Err("height_mode must be explicit or basic_standard_auto".into());
        }
        match &quality_threshold_v {
            Value::Number(n) if n.as_f64().map_or(false, |f| f.is_finite() && 0.0 < f && f <= 1.0) => quality_threshold = n.as_f64().unwrap(),
            _ => return Err("quality_threshold must be finite and in (0, 1]".into()),
        }
        match &num_extra_v {
            Value::Number(n) if n.is_i64() && (0..=32).contains(&n.as_i64().unwrap()) => num_extra_peaks = n.as_i64().unwrap(),
            _ => return Err("num_extra_peaks must be an integer in [0, 32]".into()),
        }
        match threshold {
            Some(t) if t.is_finite() && 0.0 < t && t <= 1e12 => {}
            _ => return Err("Morphology peak threshold must be finite and in (0, 1e12]".into()),
        }
    }
    let sample = AbifFile::parse(raw, filename).map_err(|e| e.0)?;
    let normalization = match &normalization_v {
        Value::String(s) => {
            let t = strip(s).to_lowercase();
            t == "1" || t == "true" || t == "yes"
        }
        v => truthy(v),
    };
    let (sfn_status, sfn_factor) = sample_file_normalization(&sample);
    let norm_scale: Option<f64> = if normalization && sfn_status == "Yes" { sfn_factor } else { None };
    let sc_truthy = truthy(&standard_channel_v);
    let sb_truthy = truthy(&standard_bp);
    let mode: String = if truthy(&sizing_mode) {
        match &sizing_mode {
            Value::String(s) => s.clone(),
            _ => return Err("Sizing mode must be none, manual or auto".into()),
        }
    } else if sc_truthy || sb_truthy {
        "auto".into()
    } else if !strip(&anchors_text).is_empty() {
        "manual".into()
    } else {
        "none".into()
    };
    if !["none", "manual", "auto"].contains(&mode.as_str()) {
        return Err("Sizing mode must be none, manual or auto".into());
    }
    if !spike_definition.is_null() && mode != "auto" {
        return Err("Initial spike subset requires an explicit automatic reference dye".into());
    }
    if !sizing_quality_definition.is_null() && mode != "auto" {
        return Err("Post-sizing quality requires explicit automatic reference dye and fragments".into());
    }
    if !pullup_definition.is_null() && mode != "auto" {
        return Err("Pull-up policy requires an explicit automatic reference dye".into());
    }
    if mode == "auto" && !strip(&anchors_text).is_empty() {
        return Err("Automatic sizing cannot be combined with manual anchors".into());
    }
    let mut calibration_anchors: Option<Anchors> = if mode == "manual" { parse_anchors(&anchors_text)? } else { None };
    if mode == "manual" && calibration_anchors.is_none() {
        return Err("Manual sizing requires at least two scan:bp anchors".into());
    }
    let chs = sample.channels().map_err(|e| e.0)?;
    if chs.is_empty() {
        return Err("No supported numeric DATA traces found; unknown DATA types remain opaque".into());
    }
    for ch in &chs {
        // Raised after the matrix step in the reference; the order only differs when both fail.
        if ch.values.iter().any(|v| matches!(v, Num::Float(f) if !f.is_finite())) {
            return Err(format!("{}: non-finite numeric values", ch.tag));
        }
    }
    let mut channels: Vec<Obj> = chs.iter().map(channel_obj).collect();
    let matrix_info = apply_matrix(&sample, &mut channels, kw(kwargs, "matrix").unwrap_or(&null))?;
    for ch in channels.iter_mut() {
        if let Some(range) = analysis_range {
            if ch["group"] != json!("auxiliary") {
                apply_analysis_range(ch, range);
            }
        }
        if engine == "residual" {
            let th = threshold.ok_or("must be real number")?;
            let dist = real(&distance_v).ok_or("must be real number")?;
            let peaks = find_peaks(&nums_of(&ch["values"]), th, dist, 101)?;
            ch.insert("peaks".into(), Value::Array(peaks));
        }
    }
    let mut reference: Option<usize> = None;
    let mut standard_selected: Vec<Value> = Vec::new();
    let mut expected: Option<Vec<f64>> = None;
    let mut expected_value: Value = Value::Null; // for the residual engine, a caller-supplied list stays as is
    let (mut standard_start, mut end) = (0i64, 0i64);
    let standard_channel = match &standard_channel_v {
        Value::String(s) => s.clone(),
        v => crate::pydump::py_str(v),
    };
    let max_residual = real(&max_residual_v);
    if mode == "auto" {
        match max_residual {
            Some(m) if m.is_finite() && 0.0 < m && m <= 100.0 => {}
            _ => return Err("Maximum sizing residual must be finite and in (0, 100] bp".into()),
        }
        let idx = channels.iter().position(|c| c["tag"] == json!(standard_channel));
        match idx {
            Some(i) if channels[i]["group"] != json!("auxiliary") => reference = Some(i),
            _ => return Err("Select an existing raw/analyzed dye channel for the size standard".into()),
        }
        let rlen = channels[reference.unwrap()]["values"].as_array().unwrap().len() as i64;
        let end_v = if standard_end_v.is_null() { json!(rlen - 1) } else { standard_end_v.clone() };
        if !is_int(&standard_start_v) || !is_int(&end_v) {
            return Err("Standard scan range must be within the selected reference trace".into());
        }
        standard_start = standard_start_v.as_i64().unwrap_or(-1);
        end = end_v.as_i64().unwrap_or(-1);
        if !(0 <= standard_start && standard_start < end && end < rlen) {
            return Err("Standard scan range must be within the selected reference trace".into());
        }
        if morphology {
            if standard_start != 0 || end != rlen - 1 {
                return Err("Morphology/ratio engine requires the full reference trace; partial ranges are not supported".into());
            }
            expected = Some(pipeline_expected(&standard_bp)?);
        } else if let Value::String(s) = &standard_bp {
            let e = parse_expected_bp(s)?;
            expected_value = Value::Array(e.iter().map(|&x| fv(x)).collect());
            expected = Some(e);
        } else {
            expected_value = standard_bp.clone();
        }
        if morphology {
            expected_value = Value::Array(expected.as_ref().unwrap().iter().map(|&x| fv(x)).collect());
        }
    }
    let mut selected_group: Option<String> = None;
    if morphology {
        let sg = match reference {
            Some(i) => channels[i]["group"].as_str().unwrap().to_string(),
            None => {
                if channels.iter().any(|c| c["group"] == json!("raw")) {
                    "raw".to_string()
                } else {
                    "analyzed".to_string()
                }
            }
        };
        selected_group = Some(sg.clone());
        let th_default = threshold.unwrap();
        for i in 0..channels.len() {
            if channels[i]["group"].as_str().unwrap() != sg {
                channels[i].insert("peaks".into(), json!([]));
                channels[i].insert("processing".into(), json!({"status": "skipped", "reason": "Auxiliary or a different DATA group; no cross-group sizing"}));
            } else if Some(i) != reference {
                let tag = channels[i]["tag"].as_str().unwrap().to_string();
                let th = channel_thresholds.get(&tag).and_then(|v| v.as_f64()).unwrap_or(th_default);
                let spike = if spike_definition.is_null() { None } else { Some(&spike_definition) };
                measure_morphology_channel(&mut channels[i], th, spike, &det, norm_scale)?;
            }
        }
    }
    let mut sizing = Obj::new();
    sizing.insert("mode".into(), json!(mode));
    sizing.insert("status".into(), json!(if calibration_anchors.is_some() { "manual" } else { "not_requested" }));
    sizing.insert("reasons".into(), json!([]));
    sizing.insert("matched".into(), json!([]));
    sizing.insert("unmatched".into(), json!([]));
    sizing.insert("diagnostics".into(), json!({}));
    if mode == "auto" {
        let ri = reference.unwrap();
        if !morphology {
            let peaks: Vec<Value> = channels[ri]["peaks"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|p| {
                    let s = real(&p["scan"]).unwrap_or(f64::NAN);
                    standard_start as f64 <= s && s <= end as f64
                })
                .cloned()
                .collect();
            let out = match_size_standard(&Value::Array(peaks), &expected_value, &max_residual_v, &json!(0.5));
            calibration_anchors = out.calibration;
            let mut res = out.result.as_object().cloned().unwrap();
            res.remove("calibration");
            sizing = res;
        } else {
            let values = channels[ri]["values"].as_array().unwrap().clone();
            let (ints, float_trace) = match int_values(&values) {
                Some(v) => (v, false),
                None => (vec![0i64; values.len()], true),
            };
            let mut p = SizeParams::defaults();
            p.detection_threshold = channel_thresholds.get(&standard_channel).and_then(|v| v.as_f64()).unwrap_or(threshold.unwrap());
            p.height_mode = height_mode.as_str().unwrap().to_string();
            p.coordinate_mode = "standard_wrapper".into();
            p.baseline_window = det.baseline_window;
            p.smooth_passes = det.smooth_passes;
            p.sg_window = det.standard_window;
            p.sg_degree = det.degree;
            p.min_width = det.min_width;
            p.tau_begin = det.tau_begin;
            p.tau_end = det.tau_end;
            p.float_trace = float_trace;
            let out = analyze_size_standard(&ints, &expected_value, num_extra_peaks, quality_threshold, &p);
            let result = out.result;
            calibration_anchors = out.calibration;
            standard_selected = result["selected_peaks"].as_array().cloned().unwrap_or_default();
            let rpeaks = result["peaks"].as_array().cloned().unwrap_or_default();
            let pubp = public_sg_peaks(&rpeaks, &values)?;
            channels[ri].insert("peaks".into(), Value::Array(pubp));
            channels[ri].insert(
                "processing".into(),
                json!({"preparation": result["preparation"], "detector": result["detector"],
                    "coordinate_conversion": result["coordinate_conversion"], "autoheight": result["autoheight"]}),
            );
            let matching = if truthy(&result["matching"]) { result["matching"].clone() } else { json!({}) };
            let filtered = if truthy(&result["filtering"]) { result["filtering"].clone() } else { json!({}) };
            let matched: Vec<Value> = matching
                .get("matched")
                .and_then(|m| m.as_array())
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|p| {
                    let mut d = p.as_object().cloned().unwrap();
                    d.insert("holdout_error_bp".into(), Value::Null);
                    Value::Object(d)
                })
                .collect();
            let unmatched: Vec<Value> = result["selected_peaks"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|p| p.get("matched_bp").map_or(true, |v| v.is_null()))
                .cloned()
                .collect();
            let pick = |k: &str| filtered.get(k).cloned().unwrap_or(Value::Null);
            sizing = Obj::new();
            sizing.insert("status".into(), result["status"].clone());
            sizing.insert("stage".into(), result["stage"].clone());
            sizing.insert("reasons".into(), result["reasons"].clone());
            sizing.insert("matched".into(), Value::Array(matched));
            sizing.insert("unmatched".into(), Value::Array(unmatched));
            sizing.insert("candidate_count".into(), json!(rpeaks.len()));
            sizing.insert("expected_count".into(), json!(expected.as_ref().unwrap().len()));
            sizing.insert("diagnostics".into(), matching.get("diagnostics").cloned().unwrap_or(json!({})));
            sizing.insert("matching".into(), matching);
            sizing.insert("base_quality".into(), result["base_quality"].clone());
            sizing.insert("autoheight".into(), result["autoheight"].clone());
            sizing.insert("preparation".into(), result["preparation"].clone());
            sizing.insert("detector".into(), result["detector"].clone());
            sizing.insert("coordinate_conversion".into(), result["coordinate_conversion"].clone());
            sizing.insert("prefilter".into(), json!({"status": pick("status"), "reasons": pick("reasons"), "diagnostics": pick("diagnostics")}));
            sizing.insert(
                "settings".into(),
                json!({"height_mode": height_mode, "quality_threshold": quality_threshold_v, "num_extra_peaks": num_extra_v,
                    "coordinate_mode": "standard_wrapper", "ratio_factor": 0.8, "ratio_score_threshold": 0.75,
                    "ambiguity_margin": 0.01, "max_bp_per_scan": 0.25, "selected_data_group": selected_group}),
            );
            sizing.insert("not_applied".into(), result["not_applied"].clone());
            sizing.insert("external_validation".into(), json!(false));
            sizing.insert("original_runtime_validated".into(), json!(false));
        }
        let list = match &expected_value {
            Value::Array(a) => a.clone(),
            _ => return Err("'standard_bp' is not iterable".into()),
        };
        sizing.insert("mode".into(), json!("auto"));
        sizing.insert("reference_channel".into(), standard_channel_v.clone());
        sizing.insert("scan_range".into(), json!([standard_start, end]));
        sizing.insert("expected_bp".into(), Value::Array(list));
        sizing.insert("max_residual_bp".into(), max_residual_v.clone());
    }
    if let Some(ov) = kw(kwargs, "sizing_override") {
        if !ov.is_null() {
            calibration_anchors = Some(apply_sizing_override(&mut sizing, ov, &mode, &engine)?);
        }
    }
    sizing.insert("analysis_engine".into(), json!(engine));
    sizing.insert("calibration_model".into(), calibration_model.clone());
    sizing.insert("calibration_diagnostics".into(), json!({}));
    let mut calibration: Option<Calibration> = None;
    if let Some(a) = &calibration_anchors {
        let anchors_v = Value::Array(a.anchors.iter().map(|&(s, b)| json!([fv(s), fv(b)])).collect());
        let res = (|| -> Res<Calibration> {
            let curve = build_calibration(&anchors_v, Some(&calibration_model))?;
            sizing.insert("calibration_diagnostics".into(), Value::Object(curve.diagnostics()));
            if mode == "auto" {
                let err = curve.diagnostics().get("max_abs_anchor_error_bp").and_then(|v| v.as_f64()).unwrap_or(0.0);
                if err > max_residual.unwrap_or(f64::INFINITY) {
                    return Err("Calibration fit exceeds the permitted anchor error".into());
                }
            }
            Ok(curve)
        })();
        match res {
            Ok(c) => calibration = Some(c),
            Err(e) => {
                if mode != "auto" {
                    return Err(e);
                }
                sizing.insert("status".into(), json!("rejected"));
                if let Some(Value::Array(r)) = sizing.get_mut("reasons") {
                    r.push(json!(format!("Calibration model rejected: {}", e)));
                }
                sizing.insert("stage".into(), json!("calibration_model"));
            }
        }
    }
    if let Some(c) = &calibration {
        for ch in channels.iter_mut() {
            if let Some(Value::Array(peaks)) = ch.get_mut("peaks") {
                for p in peaks.iter_mut() {
                    let size = c.size(&p["scan"])?;
                    p.as_object_mut().unwrap().insert("size_bp".into(), size.map(fv).unwrap_or(Value::Null));
                }
            }
        }
    }
    sizing.insert("anchors".into(), calibration.as_ref().map(|c| c.anchors()).unwrap_or(json!([])));
    if mode == "auto" {
        let status_accepted = sizing.get("status") == Some(&json!("accepted"));
        if let Some(Value::Array(matched)) = sizing.get_mut("matched") {
            for m in matched.iter_mut() {
                m.as_object_mut().unwrap().insert("tentative".into(), json!(!status_accepted));
            }
        }
        sizing.insert("calibration_applied".into(), json!(calibration.is_some()));
    }
    if !sizing_quality_definition.is_null() {
        let pq = crate::post_sizing_quality::evaluate_post_sizing_quality(
            sizing.get("base_quality").unwrap_or(&null),
            &Value::Array(standard_selected.clone()),
            calibration.as_ref(),
            &sizing_quality_definition,
            &json!(standard_channel),
        )?;
        let evaluated = pq["status"] == json!("evaluated");
        sizing.insert("post_quality".into(), pq);
        if evaluated {
            let na: Vec<Value> = sizing
                .get("not_applied")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|s| s != &json!("ay broad-width multiplier"))
                .collect();
            sizing.insert("not_applied".into(), Value::Array(na));
        }
    }
    let mut artifacts: Option<Obj> = None;
    if !spike_definition.is_null() {
        let schema = spike_definition["schema_version"].as_i64().unwrap_or(0);
        let mut a = Obj::new();
        for (k, v) in [("status", json!("evaluated_subset")), ("classified", json!(0)), ("current_spikes", json!(0)), ("overlaps", json!(0)), ("unavailable", json!(0)),
            ("scope", json!("initial_non_reference_classification")), ("reference_channel_unchanged", json!(true)),
            ("downstream_spike_adjustments_applied", json!(false)), ("automatic_pullup_applied", json!(false))] {
            a.insert(k.into(), v);
        }
        if schema >= 3 {
            a.insert("morphology_measurement_source".into(), json!("baseline_corrected_companion"));
            a.insert("morphology_scope".into(), json!("jag_and_ideal_gaussian_fit_only"));
            a.insert("true_peak_restoration_applied".into(), json!(false));
            a.insert("poor_fit_peaks".into(), json!(0));
        }
        if schema == 4 {
            a.insert("morphology_scope".into(), json!("side_collection_true_peak_and_fresh_restoration"));
            a.insert("restored_peaks".into(), json!(0));
            a.insert("height_only_peaks".into(), json!(0));
            a.insert("restoration_unavailable".into(), json!(0));
        }
        let sg = selected_group.clone().unwrap_or_default();
        for (ci, ch) in channels.iter_mut().enumerate() {
            if ch["group"].as_str().unwrap() != sg || Some(ci) == reference {
                continue;
            }
            let signal_length = ch["values"].as_array().unwrap().len();
            let peaks = ch.get_mut("peaks").and_then(|p| p.as_array_mut()).unwrap();
            for peak in peaks.iter_mut() {
                let pobj = peak.as_object_mut().unwrap();
                if let (Some(c), Some(Value::Object(features))) = (&calibration, pobj.get("spike_features").cloned()) {
                    if !features.is_empty() && features.get("status") == Some(&json!("evaluated_subset")) {
                        let dims = crate::quality_control::dimensions_in_bp(
                            &json!({"begin_scan": pobj["begin"], "end_scan": pobj["end"], "fwhm_scan": pobj["fwhm_scan"], "area": pobj["area"]}),
                            c,
                        )?;
                        if dims["status"] == json!("evaluated") {
                            let scale = dims["bp_per_scan_span"].as_f64().ok_or("bp_per_scan_span")?;
                            let mut f = features.clone();
                            for name in ["exhalf_width", "near_base_width"] {
                                let w = f[&format!("{}_scan", name)].as_f64().ok_or("width scan")?;
                                f.insert(format!("{}_bp", name), if w >= 0.0 { fv(w * scale) } else { json!(-1) });
                            }
                            f.insert("bp_per_scan_span".into(), fv(scale));
                            pobj.insert("spike_features".into(), Value::Object(f));
                        }
                    }
                }
                let features = pobj.get("spike_features").cloned().unwrap_or(Value::Null);
                let mut classified = crate::spike::classify_spike(&features, &pobj["height"], &pobj["offscaled"], &spike_definition)?;
                if schema == 4 {
                    let restored = crate::true_peak::restore_peak(&Value::Object(pobj.clone()), &classified, calibration.as_ref(), &json!(signal_length))?;
                    for (k, v) in restored.as_object().unwrap() {
                        pobj.insert(k.clone(), v.clone());
                    }
                    let history = pobj["true_peak_restoration"].clone();
                    let wrote = truthy(&history["writes"]);
                    classified.as_object_mut().unwrap().insert("true_peak_restoration_applied".into(), json!(wrote));
                    let prev = a["true_peak_restoration_applied"].as_bool().unwrap_or(false);
                    a.insert("true_peak_restoration_applied".into(), json!(prev | wrote));
                    let status = history["status"].as_str().unwrap_or("");
                    for (key, label) in [("restored_peaks", "restored"), ("height_only_peaks", "height_only"), ("restoration_unavailable", "unavailable")] {
                        let n = a[key].as_i64().unwrap_or(0) + i64::from(status == label);
                        a.insert(key.into(), json!(n));
                    }
                }
                pobj.insert("spike_classification".into(), classified.clone());
                if schema >= 3 {
                    let n = a["poor_fit_peaks"].as_i64().unwrap_or(0) + i64::from(classified.get("peak_deformity") == Some(&json!("poor_fit")));
                    a.insert("poor_fit_peaks".into(), json!(n));
                }
                if classified["status"] == json!("evaluated_subset") {
                    pobj.insert("spike".into(), classified["current_spike"].clone());
                    let inc = |a: &mut Obj, k: &str, by: i64| {
                        let n = a[k].as_i64().unwrap_or(0) + by;
                        a.insert(k.into(), json!(n));
                    };
                    inc(&mut a, "classified", 1);
                    inc(&mut a, "current_spikes", i64::from(truthy(&classified["current_spike"])));
                    inc(&mut a, "overlaps", i64::from(truthy(&classified["overlap_spike"])));
                } else {
                    let n = a["unavailable"].as_i64().unwrap_or(0) + 1;
                    a.insert("unavailable".into(), json!(n));
                }
            }
        }
        if a["unavailable"].as_i64().unwrap_or(0) != 0 {
            a.insert("status".into(), json!("partial"));
        }
        artifacts = Some(a);
    }
    let mut pullup_detection: Value = Value::Null;
    // Python rebinds `detector` to the pull-up detector here; it later ends up in peak_settings.
    let mut peak_detector_override: Option<Value> = None;
    if !pullup_definition.is_null() && truthy(&pullup_definition["detector"]) {
        let sg = selected_group.clone().unwrap_or_default();
        let present: std::collections::HashSet<String> =
            channels.iter().filter(|c| c["group"].as_str() == Some(&sg)).map(|c| c["tag"].as_str().unwrap().to_string()).collect();
        let detector = pullup_definition["detector"].clone();
        peak_detector_override = Some(detector.clone());
        let pairs = detector["channel_pairs"].as_array().cloned().unwrap_or_default();
        let kept: Vec<Value> = pairs
            .iter()
            .filter(|r| {
                present.contains(r["candidate_channel"].as_str().unwrap_or("")) && present.contains(r["reference_channel"].as_str().unwrap_or(""))
            })
            .cloned()
            .collect();
        if kept.len() != pairs.len() {
            let mut d = detector.as_object().cloned().unwrap();
            d.insert("channel_pairs".into(), Value::Array(kept));
            let mut pd = pullup_definition.as_object().cloned().unwrap();
            pd.insert("detector".into(), Value::Object(d));
            pullup_definition = Value::Object(pd);
        }
    }
    if !pullup_definition.is_null() {
        let sg = selected_group.clone().unwrap_or_default();
        let all = Value::Array(channels.iter().cloned().map(Value::Object).collect());
        let detection = crate::pullup::apply_pullup_detection(&all, &pullup_definition, &json!(sg), &json!(standard_channel))?;
        if let Some(by) = detection["peaks_by_channel"].as_object() {
            for ch in channels.iter_mut() {
                let tag = ch["tag"].as_str().unwrap().to_string();
                if let Some(p) = by.get(&tag) {
                    ch.insert("peaks".into(), p.clone());
                }
            }
        }
        pullup_detection = detection["summary"].clone();
        if !pullup_definition["detector"].is_null() {
            let a = artifacts.get_or_insert_with(|| {
                let mut a = Obj::new();
                for (k, v) in [("status", json!("evaluated_subset")), ("classified", json!(0)), ("current_spikes", json!(0)), ("overlaps", json!(0)),
                    ("unavailable", json!(0)), ("reference_channel_unchanged", json!(true)), ("downstream_spike_adjustments_applied", json!(false))] {
                    a.insert(k.into(), v);
                }
                a
            });
            a.insert("automatic_pullup_applied".into(), json!(true));
            a.insert("pullup_routing_scope".into(), json!("explicit_experimental_reconstruction_original_route_unconfirmed"));
        }
    }
    let mut output = summary_head(&sample);
    output.insert("channels".into(), Value::Array(channels.iter().cloned().map(Value::Object).collect()));
    output.insert("directory".into(), directory(&sample));
    output.insert("run_info".into(), run_info(&sample)?);
    output.insert("calibration".into(), calibration.as_ref().map(|c| c.anchors()).unwrap_or(json!([])));
    output.insert("sizing".into(), Value::Object(sizing));
    output.insert("source_sha256".into(), json!(format!("{:x}", Sha256::digest(raw))));
    output.insert("analysis_version".into(), json!(analysis_version()));
    let mut ps = Obj::new();
    ps.insert("threshold".into(), threshold_v.clone());
    ps.insert("min_distance".into(), if engine == "residual" { distance_v.clone() } else { Value::Null });
    ps.insert("analysis_engine".into(), json!(engine));
    ps.insert("selected_data_group".into(), selected_group.clone().map(|s| json!(s)).unwrap_or(Value::Null));
    ps.insert("height_mode".into(), if morphology { height_mode.clone() } else { json!("explicit") });
    output.insert("peak_settings".into(), Value::Object(ps));
    output.insert("research_only".into(), json!(true));
    if let Some(m) = matrix_info {
        output.insert("matrix".into(), m);
    }
    output.insert(
        "normalization".into(),
        json!({"sample_file": sfn_status, "factor": sfn_factor.map(fv).unwrap_or(Value::Null), "requested": normalization,
            "applied": norm_scale.is_some(), "scope": if norm_scale.is_some() { json!("sample dye channels (size standard unscaled)") } else { Value::Null }}),
    );
    if let Some(a) = &artifacts {
        output.insert("artifacts".into(), Value::Object(a.clone()));
    }
    let ps = output.get_mut("peak_settings").and_then(|v| v.as_object_mut()).unwrap();
    if !channel_thresholds.is_empty() {
        ps.insert("channel_thresholds".into(), Value::Object(channel_thresholds.clone()));
    }
    if let Some((s, e)) = analysis_range {
        ps.insert("analysis_range".into(), json!([s, e]));
    }
    if let Some(d) = peak_detector_override {
        ps.insert("detector".into(), d);
    } else if !det_is_default {
        ps.insert("detector".into(), Value::Object(det.obj.clone()));
    }
    if !spike_definition.is_null() {
        ps.insert("spike_definition".into(), spike_definition.clone());
    }
    if !sizing_quality_definition.is_null() {
        ps.insert("sizing_quality_definition".into(), sizing_quality_definition.clone());
    }
    if !pullup_definition.is_null() {
        ps.insert("pullup_definition".into(), pullup_definition.clone());
    }
    if !spike_definition.is_null() {
        output.insert("artifacts".into(), artifacts.clone().map(Value::Object).unwrap_or(Value::Null));
    }
    if !pullup_definition.is_null() {
        output.insert("pullup_detection".into(), pullup_detection);
    }
    Ok(Value::Object(output))
}
