//! Port of `genemapper_open/standard_wrapper.py`: projection of size-standard peaks before matching.
use serde_json::{json, Map, Value};
use std::collections::HashSet;

pub type Res<T> = Result<T, String>;

fn finite(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Number(n)) => n.as_f64().filter(|f| f.is_finite()),
        _ => None,
    }
}

fn is_int(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) if n.is_i64() => n.as_i64(),
        _ => None,
    }
}

pub fn wrap_standard_peaks(peaks: &Value, signal_length: &Value, offset: &Value, offscale: &Value) -> Res<Value> {
    let sl = match is_int(signal_length) {
        Some(x) if (1..=1_000_000).contains(&x) => x,
        _ => return Err("signal_length must be an integer in [1, 1000000]".into()),
    };
    let off = match is_int(offset) {
        Some(x) if 0 <= x && x <= (1i64 << 31) - sl => x,
        _ => return Err("offset must keep projected coordinates within nonnegative signed32 range".into()),
    };
    let peaks = match peaks {
        Value::Array(a) if a.len() <= 4096 => a,
        _ => return Err("peaks must be a list or tuple of at most4096 mappings".into()),
    };
    let intervals = match offscale {
        Value::Array(a) if a.len() <= 10000 => a,
        _ => return Err("offscale_intervals must be a bounded list or tuple".into()),
    };
    let mut ivs: Vec<(f64, f64)> = Vec::new();
    for iv in intervals {
        let bad = || "offscale intervals require finite ordered nonnegative signed32 coordinates".to_string();
        let a = match iv {
            Value::Array(a) if a.len() == 2 => a,
            _ => return Err(bad()),
        };
        let (x, y) = match (finite(Some(&a[0])), finite(Some(&a[1]))) {
            (Some(x), Some(y)) => (x, y),
            _ => return Err(bad()),
        };
        if !(0.0 <= x && x <= y && y <= 2147483647.0) {
            return Err(bad());
        }
        ivs.push((x, y));
    }
    let mut output: Vec<Map<String, Value>> = Vec::new();
    let (mut changed, mut new_off) = (0i64, 0i64);
    for peak in peaks {
        let pm = peak.as_object().ok_or("Each peak must be a coordinate mapping")?;
        let mut c = [0f64; 3];
        for (i, k) in ["begin", "scan", "end"].iter().enumerate() {
            c[i] = finite(pm.get(*k)).ok_or("Each peak requires finite begin, scan and end")?;
        }
        if !(0.0 <= c[0] && c[0] <= c[1] && c[1] <= c[2] && c[2] < sl as f64) {
            return Err("Peak coordinates must lie within the local signal".into());
        }
        let previous = match pm.get("offscaled") {
            None => false,
            Some(Value::Bool(b)) => *b,
            Some(_) => return Err("offscaled must be bool when supplied".into()),
        };
        if pm.contains_key("wrapper_source_local") {
            return Err("Peaks must be projected once; wrapper_source_local is reserved".into());
        }
        let mut item = pm.clone();
        let mut src = Map::new();
        for k in ["begin", "scan", "end"] {
            src.insert(k.into(), pm[k].clone());
        }
        item.insert("wrapper_source_local".into(), Value::Object(src));
        for (i, k) in ["begin", "scan", "end"].iter().enumerate() {
            if c[i] != c[i].trunc() {
                changed += 1;
            }
            item.insert((*k).into(), json!(c[i].trunc() as i64 + off));
        }
        let b = item["begin"].as_i64().unwrap() as f64;
        let e = item["end"].as_i64().unwrap() as f64;
        let now = previous || ivs.iter().any(|&(a, bb)| a <= e && bb >= b);
        item.insert("offscaled".into(), Value::Bool(now));
        if now && !previous {
            new_off += 1;
        }
        output.push(item);
    }
    output.sort_by_key(|p| p["scan"].as_i64().unwrap());
    let uniq: HashSet<i64> = output.iter().map(|p| p["scan"].as_i64().unwrap()).collect();
    Ok(json!({"peaks": output.iter().cloned().map(Value::Object).collect::<Vec<_>>(),
        "diagnostics": {"input_count": peaks.len(), "output_count": output.len(), "signal_length": sl, "offset": off,
        "fractional_coordinates_truncated": changed, "new_offscaled_peaks": new_off,
        "duplicate_projected_scans": output.len() - uniq.len(), "offscale_interval_count": ivs.len(),
        "output_coordinate_scope": if off == 0 {"local"} else {"absolute"}, "original_runtime_validated": false}}))
}
