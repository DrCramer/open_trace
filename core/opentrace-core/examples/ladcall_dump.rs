//! Counterpart of scratchpad/ladcall_check.py. Kinds: call, validate. Lines `LC <i> OK|ERR`.
use opentrace_core::ladder_calling as lc;
use opentrace_core::ladder_observation_binding as lb;
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
fn main() {
    let file = File::open(std::env::args().nth(1).unwrap()).unwrap();
    for (i, line) in BufReader::new(file).lines().enumerate() {
        let req = decode(&serde_json::from_str::<Value>(&line.unwrap()).unwrap());
        let r = match req["fn"].as_str().unwrap() {
            "call" => lc::ladder_call_observations(&req["peaks"], &req["kit"], &req["matching"]),
            _ => lb::validate(&req["peaks"], &req["kit"], &req["matching"], &req["observations"]),
        };
        match r {
            Ok(v) => println!("LC {} OK {}", i, serde_json::to_string(&canon(&v)).unwrap()),
            Err(e) => println!("LC {} ERR {}", i, e),
        }
    }
}
