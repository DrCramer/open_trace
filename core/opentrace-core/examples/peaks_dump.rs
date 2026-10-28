//! Rust counterpart of scratchpad/peaks_check.py: runs `prepare_peaks` on each request line and prints
//! canonical JSON (floats as IEEE bit hex). Lines: `A <i> OK <json>` or `A <i> ERR <message>`.
use opentrace_core::allele_calling::prepare_peaks;
use serde_json::{Map, Value};
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
        Value::Number(n) if n.is_f64() => serde_json::json!({"__f": format!("{:016x}", n.as_f64().unwrap().to_bits())}),
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
        let peaks = req.get("peaks").cloned().unwrap_or(Value::Null);
        match prepare_peaks(&peaks) {
            Ok(list) => println!("A {} OK {}", i, serde_json::to_string(&canon(&Value::Array(list))).unwrap()),
            Err(e) => println!("A {} ERR {}", i, e),
        }
    }
}
