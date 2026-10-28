//! Port of `genemapper_open/sample_info.py::sample_info` (Sample Info / Raw Data / EPT views).
//!
//! Known gap: non-finite floats cannot be represented in `serde_json::Value` and become null;
//! Python TypeError paths for malformed date tags (list-of-list or float list) return an error string.
use crate::abif::{AbifFile, Num, Value as AV};
use serde_json::{json, Map, Value};

pub type Res<T> = Result<T, String>;

fn num_json(n: &Num) -> Value {
    match n {
        Num::Int(i) => json!(i),
        Num::Float(f) => serde_json::Number::from_f64(*f).map(Value::Number).unwrap_or(Value::Null),
    }
}

fn fv(f: f64) -> Value {
    serde_json::Number::from_f64(f).map(Value::Number).unwrap_or(Value::Null)
}

fn is_leap(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if is_leap(y) {
                29
            } else {
                28
            }
        }
    }
}

/// Seconds since 0001-01-01 (proleptic Gregorian) for a validated date/time.
fn seconds(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64) -> i64 {
    let mut days = 0i64;
    let y1 = y - 1;
    days += y1 * 365 + y1 / 4 - y1 / 100 + y1 / 400;
    for m in 1..mo {
        days += days_in_month(y, m);
    }
    days += d - 1;
    days * 86400 + h * 3600 + mi * 60 + s
}

#[derive(Clone, Copy)]
struct Stamp {
    y: i64,
    mo: i64,
    d: i64,
    h: i64,
    mi: i64,
    s: i64,
}

impl Stamp {
    fn secs(&self) -> i64 {
        seconds(self.y, self.mo, self.d, self.h, self.mi, self.s)
    }
    fn fmt(&self) -> String {
        format!("{}-{:02}-{:02} {:02}:{:02}:{:02}", self.y, self.mo, self.d, self.h, self.mi, self.s)
    }
}

fn int_list(v: &AV) -> Res<Option<Vec<i64>>> {
    Ok(match v {
        AV::Date(l) | AV::Ints(l) => Some(l.clone()),
        AV::Dates(l) if l.len() >= 3 => return Err("TypeError: datetime component is a list".into()),
        AV::Floats(l) if l.len() >= 3 => return Err("TypeError: datetime component is a float".into()),
        _ => None,
    })
}

fn date(a: &AbifFile, n: i32) -> Res<Option<Stamp>> {
    let d = a.value("RUND", n);
    let t = a.value("RUNT", n);
    let d = match d.as_ref().map(int_list).transpose()?.flatten() {
        Some(l) if l.len() >= 3 => l,
        _ => return Ok(None),
    };
    let (y, mo, dd) = (d[0], d[1], d[2]);
    let tl = t.as_ref().map(int_list).transpose()?.flatten();
    if !(1..=9999).contains(&y) || !(1..=12).contains(&mo) || dd < 1 || dd > days_in_month(y, mo) {
        return Ok(None);
    }
    let (h, mi, s) = match tl {
        Some(l) if l.len() >= 3 => (l[0], l[1], l[2]),
        _ => (0, 0, 0),
    };
    if !(0..24).contains(&h) || !(0..60).contains(&mi) || !(0..60).contains(&s) {
        return Ok(None);
    }
    Ok(Some(Stamp { y, mo, d: dd, h, mi, s }))
}

fn py_isspace(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r' | '\x1c' | '\x1d' | '\x1e' | '\x1f' | '\u{85}' | '\u{a0}' | '\u{1680}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}')
        || ('\u{2000}'..='\u{200a}').contains(&c)
}

/// `_text`: stripped string, number, or None.
fn text(a: &AbifFile, name: &str, n: i32) -> Value {
    match a.value(name, n) {
        Some(AV::Str(s)) => json!(s.trim_matches(py_isspace)),
        Some(AV::Int(i)) => json!(i),
        Some(AV::Float(f)) => fv(f),
        _ => Value::Null,
    }
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::String(s) => !s.is_empty(),
        Value::Number(n) => n.as_f64().map_or(true, |f| f != 0.0),
        _ => true,
    }
}

fn or(a: Value, b: Value) -> Value {
    if truthy(&a) {
        a
    } else {
        b
    }
}

fn items(pairs: Vec<(&str, Value)>) -> Vec<Value> {
    pairs
        .into_iter()
        .filter(|(_, v)| !(v.is_null() || v == &json!("")))
        .map(|(k, v)| json!({"key": k, "value": v}))
        .collect()
}

fn av_json(v: &AV) -> Value {
    match v {
        AV::Bytes(b) => json!({"__bytes": b.iter().map(|x| format!("{:02x}", x)).collect::<String>()}),
        AV::Str(s) => json!(s),
        AV::Int(i) => json!(i),
        AV::Float(f) => fv(*f),
        AV::Ints(l) | AV::Date(l) => json!(l),
        AV::Floats(l) => Value::Array(l.iter().map(|f| fv(*f)).collect()),
        AV::Dates(l) => json!(l),
    }
}

fn nums(v: &AV) -> Option<Vec<Num>> {
    match v {
        AV::Ints(l) => Some(l.iter().map(|i| Num::Int(*i)).collect()),
        AV::Floats(l) => Some(l.iter().map(|f| Num::Float(*f)).collect()),
        _ => None,
    }
}

pub fn sample_info(data: &[u8], filename: &str) -> Res<Value> {
    let a = AbifFile::parse(data, filename).map_err(|e| e.0)?;
    let start = date(&a, 1)?;
    let stop = date(&a, 2)?;
    let mut duration = Value::Null;
    if let (Some(s), Some(e)) = (start, stop) {
        if e.secs() >= s.secs() {
            let sec = e.secs() - s.secs();
            duration = json!(format!("{}:{:02}:{:02}", sec / 3600, sec % 3600 / 60, sec % 60));
        }
    }
    let chans = a.channels().map_err(|e| e.0)?;
    let mut raw: Vec<(i32, Value, Vec<Num>, String)> = Vec::new();
    for ch in &chans {
        if ch.group == "raw" {
            let di = ch.dye_index.unwrap();
            raw.push((di, json!(ch.tag), ch.values.clone(), ch.dye.clone()));
        }
    }
    raw.sort_by_key(|r| r.0);
    let mut ept = Map::new();
    for (key, n) in [("voltage", 5), ("current", 6), ("power", 7), ("temperature", 8)] {
        if let Some(l) = a.value("DATA", n).as_ref().and_then(nums) {
            if l.is_empty() {
                continue;
            }
            let mut mn = l[0];
            let mut mx = l[0];
            let f = |x: &Num| match x {
                Num::Int(i) => *i as f64,
                Num::Float(f) => *f,
            };
            for x in &l[1..] {
                if f(x) < f(&mn) {
                    mn = *x;
                }
                if f(x) > f(&mx) {
                    mx = *x;
                }
            }
            ept.insert(
                key.into(),
                json!({"tag": format!("DATA{}", n), "values": l.iter().map(num_json).collect::<Vec<_>>(), "min": num_json(&mn), "max": num_json(&mx)}),
            );
        }
    }
    let scan_v = match a.value("SCAN", 1) {
        Some(AV::Int(i)) => Some(AV::Int(i)),
        _ => a.value("Scan", 1),
    };
    let scan_json = scan_v.as_ref().map_or(Value::Null, av_json);
    let scan_int = match &scan_v {
        Some(AV::Int(i)) => Some(*i),
        _ => None,
    };
    let dyes = raw.iter().filter(|r| !r.3.is_empty()).map(|r| r.3.clone()).collect::<Vec<_>>().join(", ");
    let dyes = if dyes.is_empty() { Value::Null } else { json!(dyes) };
    let t = |name: &str| text(&a, name, 1);
    let sections = vec![
        ("sample", items(vec![
            ("sampleFile", json!(a.filename)), ("sampleName", or(t("SpNm"), t("SMPL"))), ("well", t("TUBE")),
            ("plate", t("CTNM")), ("fileSource", json!(format!("ABIF v{:.2}", a.version as f64 / 100.0))), ("comment", t("CMNT")),
            ("sizeStandardName", t("StdF")), ("normalizationFactor", t("NrmS")), ("injection", t("InjN"))])),
        ("run", items(vec![
            ("userName", t("User")), ("instrumentID", t("MCHN")),
            ("instrumentType", or(text(&a, "HCFG", 3), t("MODL"))), ("instrumentConfig", text(&a, "HCFG", 4)),
            ("dataCollectionVersion", t("SVER")), ("firmwareVersion", text(&a, "SVER", 3)),
            ("runName", t("RunN")), ("runDate", start.map_or(Value::Null, |s| json!(s.fmt()))),
            ("runStop", stop.map_or(Value::Null, |s| json!(s.fmt()))), ("runDuration", duration),
            ("totalDataPoints", scan_json.clone())])),
        ("collection", items(vec![
            ("moduleFile", t("MODF")), ("runModuleName", t("RMdN")), ("runModuleVersion", t("RMdV")),
            ("runProtocol", t("RPrN")), ("runVoltage", t("EPVt")), ("injectionVoltage", t("InVt")),
            ("injectionDuration", t("InSc")), ("temperature", t("Tmpr")), ("laserPower", t("LsrP")),
            ("dyeSetName", t("DySN")), ("dyes", dyes),
            ("polymerType", t("GTyp")), ("polymerLot", t("SMLt")), ("polymerExpiration", t("SMED")),
            ("polymerInstalled", t("SMID")), ("resultsGroup", t("RGNm"))])),
        ("buffers", items(vec![
            ("anodeType", t("ABTp")), ("anodeLot", t("ABLt")), ("anodeExpiration", t("ABED")),
            ("anodeInstalled", t("ABID")), ("cathodeType", t("CBTp")), ("cathodeLot", t("CBLt")),
            ("cathodeExpiration", t("CBED")), ("cathodeInstalled", t("CBID"))])),
        ("capillary", items(vec![
            ("arrayLot", t("CALt")), ("arraySerial", t("CASN")), ("capillaryLength", t("LNTD")),
            ("capillaries", t("NLNE")), ("capillaryNumber", t("LANE")), ("arrayUsage", t("CARn")),
            ("arrayExpiration", t("CAED"))])),
    ];
    let sections: Vec<Value> = sections.into_iter().filter(|(_, i)| !i.is_empty()).map(|(id, i)| json!({"id": id, "items": i})).collect();
    let raw_json: Vec<Value> = raw
        .iter()
        .map(|(di, tag, vals, dye)| json!({"tag": tag, "dye_index": di, "dye": dye, "values": vals.iter().map(num_json).collect::<Vec<_>>()}))
        .collect();
    let scan_count = match scan_int {
        Some(i) => i,
        None => raw.first().map_or(0, |r| r.2.len() as i64),
    };
    Ok(json!({"filename": a.filename, "sections": sections, "raw": raw_json, "ept": Value::Object(ept), "scan_count": scan_count}))
}
