//! Port of `genemapper_open/allele_calling.py`, built up one function at a time.
//! Done so far: `prepare_peaks` (Python `_prepare_peaks`, input validation and normalization).
use serde_json::{json, Value};

pub type Res<T> = Result<T, String>;

pub const MAX_INPUT_PEAKS: usize = 4096;

/// Python `allele_calling._number`: a finite real that is not a bool. JSON numbers are always finite.
fn is_number(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Number(_)))
}

fn as_f64(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        _ => f64::NAN,
    }
}

fn fv(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

/// Python `re.fullmatch(r'DATA[1-9][0-9]{0,5}', channel)`.
fn canonical_channel(s: &str) -> bool {
    let rest = match s.strip_prefix("DATA") {
        Some(r) => r.as_bytes(),
        None => return false,
    };
    (1..=6).contains(&rest.len()) && (b'1'..=b'9').contains(&rest[0]) && rest[1..].iter().all(|c| c.is_ascii_digit())
}

/// Python `_prepare_peaks`: validates raw peak records and returns normalized records sorted by (size_bp, peak_index).
pub fn prepare_peaks(peaks: &Value) -> Res<Vec<Value>> {
    let limit_message = format!("peaks must be a list of at most {} records", MAX_INPUT_PEAKS);
    let items = match peaks {
        Value::Array(a) if a.len() <= MAX_INPUT_PEAKS => a,
        _ => return Err(limit_message),
    };
    let mut prepared: Vec<(f64, usize, Value)> = Vec::with_capacity(items.len());
    for (index, peak) in items.iter().enumerate() {
        let map = peak.as_object().ok_or_else(|| format!("peak {} must be an object", index))?;
        let channel = match map.get("channel") {
            Some(Value::String(s)) if canonical_channel(s) => s.clone(),
            _ => return Err(format!("peak {} needs a canonical DATA channel", index)),
        };
        for key in ["scan", "size_bp", "height"] {
            if !is_number(map.get(key)) {
                return Err(format!("peak {} needs finite {}", index, key));
            }
        }
        let scan = as_f64(&map["scan"]);
        let size_bp = as_f64(&map["size_bp"]);
        let height = as_f64(&map["height"]);
        if !(0.0 <= scan && scan <= 100_000_000.0 && 0.0 < size_bp && size_bp <= 1_000_000.0
            && 0.0 < height && height <= 1e12)
        {
            return Err(format!("peak {} needs bounded scan and positive bounded size/height", index));
        }
        let mut flags: Vec<String> = match map.get("flags") {
            None => Vec::new(),
            Some(Value::Object(flag_map)) => {
                if flag_map.values().any(|v| !matches!(v, Value::Bool(_))) {
                    return Err(format!("peak {} flag map must contain string/boolean pairs", index));
                }
                flag_map
                    .iter()
                    .filter(|(_, v)| **v == Value::Bool(true))
                    .map(|(name, _)| name.clone())
                    .collect()
            }
            Some(Value::Array(list)) => {
                let mut names = Vec::with_capacity(list.len());
                for f in list {
                    match f {
                        Value::String(s) => names.push(s.clone()),
                        _ => return Err(format!("peak {} flags must be strings or a boolean flag map", index)),
                    }
                }
                names
            }
            Some(_) => return Err(format!("peak {} flags must be strings or a boolean flag map", index)),
        };
        // Python `list(dict.fromkeys(flags))`: first occurrence wins.
        let mut unique: Vec<String> = Vec::with_capacity(flags.len());
        for f in flags.drain(..) {
            if !unique.contains(&f) {
                unique.push(f);
            }
        }
        let mut flags = unique;
        for flag in ["spike", "pullup"] {
            if map.contains_key(flag) && !matches!(map[flag], Value::Bool(_)) {
                return Err(format!("peak {} {} must be boolean", index, flag));
            }
            if map.get(flag) == Some(&Value::Bool(true)) && !flags.iter().any(|f| f == flag) {
                flags.push(flag.to_string());
            }
        }
        prepared.push((
            size_bp,
            index,
            json!({
                "peak_index": index,
                "channel": channel,
                "scan": fv(scan),
                "size_bp": fv(size_bp),
                "height": fv(height),
                "flags": flags,
            }),
        ));
    }
    prepared.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.cmp(&b.1)));
    Ok(prepared.into_iter().map(|(_, _, v)| v).collect())
}
