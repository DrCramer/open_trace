//! Rust counterpart of scratchpad/restore_check.py: runs each request line through `restore_peak` and prints
//! canonical JSON (floats as IEEE bit hex). Lines: `R <i> OK <json>` or `R <i> ERR <message>`.
use opentrace_core::calibration::build_calibration;
use opentrace_core::true_peak::restore_peak;
use serde_json::{json, Map, Value};
use std::fs::File;
use std::io::{BufRead, BufReader};

/// Turns `{"__f": "<hex>"}` into a float value.
fn decode(v: &Value) -> Value {
    match v {
        Value::Object(m) if m.len() == 1 && m.contains_key("__f") => {
            let hex = m["__f"].as_str().unwrap();
            let x = f64::from_bits(u64::from_str_radix(hex, 16).unwrap());
            serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
        }
        Value::Array(a) => Value::Array(a.iter().map(decode).collect()),
        Value::Object(m) => Value::Object(m.iter().map(|(k, x)| (k.clone(), decode(x))).collect::<Map<String, Value>>()),
        other => other.clone(),
    }
}

fn canon(v: &Value) -> Value {
    match v {
        Value::Number(n) if n.is_f64() => json!({"__f": format!("{:016x}", n.as_f64().unwrap().to_bits())}),
        Value::Array(a) => Value::Array(a.iter().map(canon).collect()),
        Value::Object(m) => Value::Object(m.iter().map(|(k, x)| (k.clone(), canon(x))).collect::<Map<String, Value>>()),
        other => other.clone(),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let reader = BufReader::new(File::open(&args[1]).unwrap());
    for (i, line) in reader.lines().enumerate() {
        let raw: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let req = decode(&raw);
        let peak = req.get("peak").cloned().unwrap_or(Value::Null);
        let class = req.get("classification").cloned().unwrap_or(Value::Null);
        let signal_length = req.get("signal_length").cloned().unwrap_or(Value::Null);
        let calibration = match req.get("calibration") {
            Some(Value::Object(cal)) => {
                let anchors = cal.get("anchors").cloned().unwrap_or(Value::Null);
                match build_calibration(&anchors, cal.get("model")) {
                    Ok(c) => Some(c),
                    Err(e) => {
                        println!("R {} ERR {}", i, e);
                        continue;
                    }
                }
            }
            _ => None,
        };
        match restore_peak(&peak, &class, calibration.as_ref(), &signal_length) {
            Ok(v) => println!("R {} OK {}", i, serde_json::to_string(&canon(&v)).unwrap()),
            Err(e) => println!("R {} ERR {}", i, e),
        }
    }
}
