//! Rust counterpart of scratchpad/pick_check.py: `marker_offsets` then `pick_marker` per request line.
//! Lines: `M <i> OK <json {offsets: [record, window, metadata], pick}>` or `M <i> ERR <message>`.
use opentrace_core::allele_calling::{marker_offsets, pick_marker};
use serde_json::{json, Map, Value};
use std::fs::File;
use std::io::{BufRead, BufReader};

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
        let marker = req["marker"].clone();
        let ladder = req["ladder"].clone();
        let policy = req["policy"].as_str().unwrap_or("reject").to_string();
        let candidates: Vec<Value> = req["candidates"].as_array().cloned().unwrap_or_default();
        match marker_offsets(&marker, &ladder, &policy) {
            Err(e) => println!("M {} ERR {}", i, e),
            Ok((record, window, metadata)) => {
                match pick_marker(&marker, &candidates, &record, &window, &metadata) {
                    Err(e) => println!("M {} ERR {}", i, e),
                    Ok(pick) => {
                        let out = json!({
                            "offsets": [record, [window[0].clone(), window[1].clone()], metadata],
                            "pick": pick,
                        });
                        println!("M {} OK {}", i, serde_json::to_string(&canon(&out)).unwrap());
                    }
                }
            }
        }
    }
}
