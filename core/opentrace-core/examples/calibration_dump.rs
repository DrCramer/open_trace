//! Rust counterpart of scratchpad/cal_check.py: runs each request line through `build_calibration` and prints
//! canonical JSON (floats as IEEE bit hex), one line per request. `{"hypot": [...]}` lines print `math.hypot`.
use opentrace_core::calibration::{build_calibration, py_hypot};
use serde_json::{json, Map, Value};
use std::fs::File;
use std::io::{BufRead, BufReader};

/// Turns `{"__f": "<hex>"}` into a float; non-finite values become a string, which the module rejects like Python.
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

fn fv(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

fn list_of(req: &Value, key: &str) -> Vec<Value> {
    match req.get(key) {
        Some(Value::Array(a)) => a.clone(),
        _ => vec![],
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let reader = BufReader::new(File::open(&args[1]).unwrap());
    for (i, line) in reader.lines().enumerate() {
        let raw: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let req = decode(&raw);
        if let Some(Value::Array(vals)) = req.get("hypot") {
            let xs: Vec<f64> = vals.iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect();
            println!("H {} {:016x}", i, py_hypot(&xs).to_bits());
            continue;
        }
        let anchors = req.get("anchors").cloned().unwrap_or(Value::Null);
        match build_calibration(&anchors, req.get("model")) {
            Err(e) => println!("C {} ERR {}", i, e),
            Ok(c) => {
                let size: Vec<Value> = list_of(&req, "queries")
                    .iter()
                    .map(|q| match c.size(q) {
                        Ok(Some(x)) => fv(x),
                        Ok(None) => Value::Null,
                        Err(e) => json!({"__err": e}),
                    })
                    .collect();
                let std_vals: Vec<Value> = list_of(&req, "std")
                    .iter()
                    .map(|q| match c.standard_map_size(q) {
                        Ok(x) => fv(x),
                        Err(e) => json!({"__err": e}),
                    })
                    .collect();
                let mut out = Map::new();
                out.insert("anchors".into(), c.anchors());
                out.insert("diag".into(), Value::Object(c.diagnostics()));
                out.insert("size".into(), Value::Array(size));
                out.insert("std".into(), Value::Array(std_vals));
                println!("C {} OK {}", i, serde_json::to_string(&canon(&Value::Object(out))).unwrap());
            }
        }
    }
}
