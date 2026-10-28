//! Counterpart of scratchpad/ladwf_check.py. Kinds: def, gate. Lines `RO <i> OK|ERR`.
use opentrace_core::run_offsets as ro;
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
        let g = |k: &str| req.get(k).cloned().unwrap_or(Value::Null);
        let r = match req["fn"].as_str().unwrap() {
            "validate" => ro::validate_run_offset_state(&g("state")),
            "remove" => ro::remove_run_ladder(&g("state"), &g("ladder_id")),
            "window" => ro::ladder_window_offset(&g("state"), &g("marker"), &g("local")),
            "upsert" => ro::upsert_run_offsets(&g("state"), &g("ladder_id"), &g("quality_code"), &g("result")),
            _ => ro::aggregate_run_offsets(&g("entries"), &g("kit")),
        };
        match r {
            Ok(v) => println!("RO {} OK {}", i, serde_json::to_string(&canon(&v)).unwrap()),
            Err(e) => println!("RO {} ERR {}", i, e),
        }
    }
}

