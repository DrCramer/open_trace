//! Which instrument recorded a sample file: the identification and run tags of the ABIF directory (`/api/instrument`
//! and the `instrument` entry of `/api/analyze?slim=1`). Nothing here changes the analysis.
//!
//! `vendor` is a heuristic from the header signature: Applied Biosystems instruments write `HCFG1 = "CE"`, CPU boards
//! `ECPU…` (31xx) or `UCPU…` (3500), a 31xx array of 4 or 16 capillaries and Fragment Format 2.0; compatible
//! instruments that write files "as a 3130xl" differ in these (the reasons are listed).
use crate::abif::{AbifFile, Value as AV};
use serde_json::{json, Map, Value};

fn text(a: &AbifFile, name: &str, n: i32) -> Option<String> {
    let s = match a.value(name, n)? {
        AV::Str(s) => s,
        AV::Bytes(b) => b.iter().filter(|c| (32..127).contains(*c)).map(|&c| c as char).collect(),
        AV::Int(i) => i.to_string(),
        _ => return None,
    };
    // control characters (some instruments write \b or \t into calibration names) become spaces
    let s: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn int(a: &AbifFile, name: &str, n: i32) -> Option<i64> {
    match a.value(name, n)? {
        AV::Int(i) => Some(i),
        AV::Ints(v) => v.first().copied(),
        AV::Float(f) => Some(f as i64),
        AV::Str(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn date(a: &AbifFile, d: &str, t: &str) -> Option<String> {
    let ymd = match a.value(d, 1)? {
        AV::Date(v) | AV::Ints(v) if v.len() >= 3 => format!("{:04}-{:02}-{:02}", v[0], v[1], v[2]),
        _ => return None,
    };
    match a.value(t, 1) {
        Some(AV::Ints(v)) | Some(AV::Date(v)) if v.len() >= 3 => Some(format!("{} {:02}:{:02}:{:02}", ymd, v[0], v[1], v[2])),
        _ => Some(ymd),
    }
}

fn hcfg4_field(h: &str, key: &str) -> Option<String> {
    h.split(';').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k.trim() == key && !v.trim().is_empty()).then(|| v.trim().to_string())
    })
}

pub fn instrument_info(a: &AbifFile) -> Value {
    let hcfg4 = text(a, "HCFG", 4).unwrap_or_default();
    let class = text(a, "HCFG", 1);
    let family = text(a, "HCFG", 2);
    let model = text(a, "HCFG", 3).or_else(|| text(a, "MODL", 1));
    let cpu = hcfg4_field(&hcfg4, "CPUBoard");
    let array_size = hcfg4_field(&hcfg4, "ArraySize").and_then(|s| s.parse::<i64>().ok()).or_else(|| int(a, "NLNE", 1));
    // the serial number identifies the instrument; MCHN is the name set on it (often the serial again)
    let serial = hcfg4_field(&hcfg4, "SerialNumber").or_else(|| text(a, "MCHN", 1));
    let name = text(a, "MCHN", 1);
    let format = text(a, "SVER", 4);

    let mut reasons: Vec<String> = Vec::new();
    if let Some(c) = &class {
        if c != "CE" {
            reasons.push(format!("HCFG1 = {c} (AB: CE)"));
        }
    }
    if let Some(c) = &cpu {
        if !(c.starts_with("ECPU") || c.starts_with("UCPU")) {
            reasons.push(format!("CPUBoard = {c} (AB: ECPU…/UCPU…)"));
        }
    }
    let is31 = family.as_deref() == Some("31XX");
    if is31 {
        if let Some(n) = array_size {
            if n != 4 && n != 16 {
                reasons.push(format!("{n} capillaries in a 31xx (AB 31xx: 4 or 16)"));
            }
        }
        if format.as_deref() == Some("Fragment Format 1.0") && a.version / 100 == 1 {
            if text(a, "CASN", 1).is_some() || text(a, "ABLt", 1).is_some() {
                reasons.push("consumable tags (CASN/ABLt) of the 3500 in a 31xx file".into());
            }
        }
    }
    let vendor = if class.is_none() && cpu.is_none() {
        "unknown"
    } else if reasons.is_empty() {
        "applied_biosystems"
    } else {
        "compatible"
    };

    let mut m = Map::new();
    let mut put = |k: &str, v: Option<Value>| {
        m.insert(k.into(), v.unwrap_or(Value::Null));
    };
    put("serial", serial.map(Value::from));
    put("name", name.map(Value::from));
    put("model", model.map(Value::from));
    put("family", family.map(Value::from));
    put("class", class.map(Value::from));
    put("cpu_board", cpu.map(Value::from));
    put("unit_id", hcfg4_field(&hcfg4, "UnitID").map(Value::from));
    put("capillaries", array_size.map(Value::from));
    put("capillary_length_cm", int(a, "LNTD", 1).map(Value::from));
    put("array_serial", text(a, "CASN", 1).map(Value::from));
    put("array_run", int(a, "CARn", 1).map(Value::from));
    put("data_collection", text(a, "SVER", 1).map(Value::from));
    put("firmware", text(a, "SVER", 3).map(Value::from));
    put("file_format", format.map(Value::from));
    put("polymer", text(a, "GTyp", 1).map(Value::from));
    put("polymer_lot", text(a, "SMLt", 1).map(Value::from));
    put("buffer_lot", text(a, "ABLt", 1).map(Value::from));
    put("dye_set", text(a, "DySN", 1).map(Value::from));
    put("spectral", text(a, "SpeN", 1).map(Value::from));
    put("run_module", text(a, "MODF", 1).or_else(|| text(a, "RMdN", 1)).map(Value::from));
    put("injection_v", int(a, "InVt", 1).map(Value::from));
    put("injection_s", int(a, "InSc", 1).map(Value::from));
    put("run_start", date(a, "RUND", "RUNT").map(Value::from));
    put("user", text(a, "User", 1).map(Value::from));
    m.insert("vendor".into(), json!(vendor));
    m.insert("vendor_reasons".into(), json!(reasons));
    Value::Object(m)
}

/// `/api/instrument`: the instrument of one sample file.
pub fn instrument_request(data: &[u8], filename: &str) -> Result<Value, String> {
    let a = AbifFile::parse(data, filename).map_err(|e| e.0)?;
    Ok(instrument_info(&a))
}
