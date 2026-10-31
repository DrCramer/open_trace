//! JSON views of `AbifFile` used by `server.analyze`: `summary()` header, `directory()`, `run_info()` and channel dicts.
use crate::abif::{AbifFile, Channel, Num, Value as AV};
use crate::profiles::strip;
use serde_json::{json, Map, Value};

fn fv(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

pub fn num_value(n: &Num) -> Value {
    match n {
        Num::Int(i) => json!(i),
        Num::Float(f) => fv(*f),
    }
}

/// A `Channel` as the Python channel dict (`values` included, `truncated_to` only when present).
pub fn channel_obj(ch: &Channel) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("tag".into(), json!(ch.tag));
    m.insert("number".into(), json!(ch.number));
    m.insert("group".into(), json!(ch.group));
    m.insert("dye_index".into(), ch.dye_index.map(|d| json!(d)).unwrap_or(Value::Null));
    m.insert("dye".into(), json!(ch.dye));
    m.insert("wavelength_nm".into(), ch.wavelength_nm.map(|d| json!(d)).unwrap_or(Value::Null));
    m.insert("values".into(), Value::Array(ch.values.iter().map(num_value).collect()));
    if let Some(n) = ch.truncated_to {
        m.insert("truncated_to".into(), json!(n));
    }
    m
}

fn is_numeric_type(t: u16) -> bool {
    matches!(t, 1 | 3 | 4 | 5 | 7 | 8 | 13)
}

/// `summary()` without its `channels` entry (the caller replaces that with the full channel list).
pub fn summary_head(a: &AbifFile) -> Map<String, Value> {
    let unsupported: Vec<String> = a
        .order
        .iter()
        .filter_map(|k| a.tags.get(k))
        .filter(|t| t.name == "DATA" && !is_numeric_type(t.element_type))
        .map(|t| t.key())
        .collect();
    let smpl = a.value("SMPL", 1);
    let sample = match smpl {
        Some(AV::Str(s)) => s,
        _ => a.text("SpNm", 1),
    };
    let mut m = Map::new();
    m.insert("filename".into(), json!(a.filename));
    m.insert("version".into(), json!(a.version));
    m.insert("sample".into(), json!(sample));
    m.insert("instrument".into(), json!(a.text("MODL", 1)));
    m.insert("dye_count".into(), match a.value("Dye#", 1) {
        Some(AV::Int(i)) => json!(i),
        _ => Value::Null,
    });
    m.insert("tag_count".into(), json!(a.tags.len()));
    m.insert(
        "warnings".into(),
        if unsupported.is_empty() { json!([]) } else { json!([format!("Unsupported opaque DATA tags: {}", unsupported.join(", "))]) },
    );
    m
}

pub fn directory(a: &AbifFile) -> Value {
    Value::Array(
        a.order
            .iter()
            .filter_map(|k| a.tags.get(k))
            .map(|t| json!({"tag": t.key(), "type": t.element_type, "count": t.count, "bytes": t.size, "inline": t.size <= 4, "offset": t.offset}))
            .collect(),
    )
}

fn first(a: &AbifFile, names: &[&str]) -> Value {
    for n in names {
        if let Some(AV::Str(s)) = a.value(n, 1) {
            let t = strip(&s);
            if !t.is_empty() {
                return json!(t);
            }
        }
    }
    Value::Null
}

/// `"%04d-%02d-%02d" % tuple(v[:3])` style formatting of a list value; `Err` for the TypeError cases.
fn first3(v: &AV) -> Result<Option<[i64; 3]>, String> {
    match v {
        AV::Ints(l) | AV::Date(l) if l.len() >= 3 => Ok(Some([l[0], l[1], l[2]])),
        AV::Floats(l) if l.len() >= 3 => Ok(Some([l[0].trunc() as i64, l[1].trunc() as i64, l[2].trunc() as i64])),
        AV::Dates(l) if l.len() >= 3 => Err("%d format: a real number is required, not list".into()),
        _ => Ok(None),
    }
}

pub fn run_info(a: &AbifFile) -> Result<Value, String> {
    let date = a.value("RUND", 1);
    let time = a.value("RUNT", 1);
    let mut stamp = Value::Null;
    if let Some(d) = date.as_ref() {
        if let Some(d3) = first3(d)? {
            let mut s = format!("{:04}-{:02}-{:02}", d3[0], d3[1], d3[2]);
            if let Some(t) = time.as_ref() {
                if let Some(t3) = first3(t)? {
                    s.push_str(&format!(" {:02}:{:02}:{:02}", t3[0], t3[1], t3[2]));
                }
            }
            stamp = json!(s);
        }
    }
    let lane = match a.value("LANE", 1) {
        Some(AV::Int(i)) => json!(i),
        _ => Value::Null,
    };
    let instrument_type = match a.value("HCFG", 3) {
        Some(AV::Str(s)) => json!(s),
        _ => first(a, &["MODL"]),
    };
    Ok(json!({"sample_name": first(a, &["SpNm", "SMPL"]), "run_name": first(a, &["RunN"]), "instrument_type": instrument_type,
        "instrument_id": first(a, &["MCHN"]), "run_date": stamp, "well": first(a, &["TUBE"]),
        "plate": first(a, &["CTNM", "CTID"]), "capillary": lane, "comment": first(a, &["CMNT"])}))
}
