//! Rust counterpart of scratchpad/svd_check.py: prints canonical JSON (floats as bit hex) per request.
use opentrace_core::small_svd::{polynomial_value, solve_value};
use serde_json::{json, Map, Value};
use std::fs::File;
use std::io::{BufRead, BufReader};

fn decode(v: &Value) -> Value {
    match v {
        Value::Object(m) if m.len() == 1 && m.contains_key("__f") => {
            let hex = m["__f"].as_str().unwrap();
            let x = f64::from_bits(u64::from_str_radix(hex, 16).unwrap());
            match serde_json::Number::from_f64(x) {
                Some(n) => Value::Number(n),
                None => Value::String("nonfinite".into()),
            }
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
        let get = |k: &str| req.get(k).cloned().unwrap_or(Value::Null);
        let result = match req["fn"].as_str().unwrap() {
            "solve" => solve_value(&get("matrix"), &get("rhs"), &get("max_sweeps")),
            _ => polynomial_value(&get("x"), &get("y"), &get("degree"), &get("max_sweeps")),
        };
        match result {
            Ok(v) => println!("S {} OK {}", i, serde_json::to_string(&canon(&v)).unwrap()),
            Err(e) => println!("S {} ERR {}", i, e),
        }
    }
}
