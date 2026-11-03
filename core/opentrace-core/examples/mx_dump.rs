//! Counterpart of scratchpad/mx_check.py. Lines `MX <i> OK|ERR`.
use opentrace_core::matrix as mx;
use serde_json::{json, Map, Value};
use std::fs::File;
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
fn ints(v: &Value) -> Vec<i64> {
    v.as_array().unwrap().iter().map(|x| x.as_i64().unwrap()).collect()
}
fn floats(v: &Value) -> Vec<f64> {
    v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
}
fn fl(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}
fn main() {
    let file = File::open(std::env::args().nth(1).unwrap()).unwrap();
    for (i, line) in BufReader::new(file).lines().enumerate() {
        let req = decode(&serde_json::from_str::<Value>(&line.unwrap()).unwrap());
        let r: Result<Value, String> = match req["k"].as_str().unwrap() {
            "req" => mx::matrix_request(&req["request"]),
            "base" => mx::genescan_baseline(&ints(&req["signal"])).map(|v| json!(v)).map_err(|e| e.message()),
            "medfit" => mx::medfit(&floats(&req["x"]), &floats(&req["y"])).map(|v| Value::Array(v.iter().map(|&x| fl(x)).collect())).map_err(|e| e.message()),
            _ => {
                let tr: Vec<Vec<i64>> = req["traces"].as_array().unwrap().iter().map(ints).collect();
                let inv: Vec<Vec<f64>> = req["inverse"].as_array().unwrap().iter().map(floats).collect();
                mx::multicomponent(&tr, &inv, req["int16"].as_bool().unwrap()).map(|v| json!(v)).map_err(|e| e.message())
            }
        };
        match r {
            Ok(v) => println!("MX {} OK {}", i, serde_json::to_string(&canon(&v)).unwrap()),
            Err(e) => println!("MX {} ERR {}", i, e),
        }
    }
}
