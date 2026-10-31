//! Counterpart of scratchpad/sr_check.py (sizing_residual).
use opentrace_core::abif::Num;
use opentrace_core::sizing_residual::*;
use serde_json::{json, Map, Value};
use std::io::{BufRead, BufReader};

fn decode(v: &Value) -> Value {
    match v {
        Value::Object(m) if m.len() == 1 && m.contains_key("__f") => {
            let x = f64::from_bits(u64::from_str_radix(m["__f"].as_str().unwrap(), 16).unwrap());
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
    let path = std::env::args().nth(1).unwrap();
    for (i, line) in BufReader::new(std::fs::File::open(path).unwrap()).lines().enumerate() {
        let r: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let a = decode(&r["a"]);
        let kind = r["kind"].as_str().unwrap();
        let res: Result<Value, String> = match kind {
            "fp" => {
                let values: Vec<Num> = a["values"].as_array().unwrap().iter().map(|v| if v.is_i64() { Num::Int(v.as_i64().unwrap()) } else { Num::Float(v.as_f64().unwrap()) }).collect();
                find_peaks(&values, a["threshold"].as_f64().unwrap(), a["min_distance"].as_f64().unwrap(), a["baseline_window"].as_i64().unwrap()).map(Value::Array)
            }
            "anchors" => parse_anchors(a["text"].as_str().unwrap()).map(|c| match c {
                None => Value::Null,
                Some(c) => json!({"anchors": c.anchors.iter().map(|&(s, b)| json!([s, b])).collect::<Vec<_>>()}),
            }),
            "expected" => parse_expected_bp(a["text"].as_str().unwrap()).map(|v| Value::Array(v.into_iter().map(|x| json!(x)).collect())),
            _ => {
                let o = match_size_standard(&a["peaks"], &a["expected"], &a["mr"], &a["am"]);
                let mut res = o.result;
                if let Some(c) = o.calibration {
                    res["calibration"] = json!({"anchors": c.anchors.iter().map(|&(s, b)| json!([s, b])).collect::<Vec<_>>()});
                }
                Ok(res)
            }
        };
        match res {
            Ok(v) => println!("{} OK {}", i, serde_json::to_string(&canon(&v)).unwrap()),
            Err(e) => println!("{} ERR ValueError {}", i, e),
        }
    }
}
