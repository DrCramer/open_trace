//! Counterpart of scratchpad/ladwf_check.py. Kinds: def, gate. Lines `LW <i> OK|ERR`.
use opentrace_core::ladder_quality_workflow as lw;
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
            "def" => lw::load_ladder_quality_definition(&g("definition")),
            _ => {
                let obs = g("observations");
                let policy = req.get("policy").cloned().unwrap_or(json!("reject"));
                lw::gate_ladder_offsets(&g("peaks"), &g("kit"), &g("matching"), &g("definition"), &g("sizing"), &policy, if obs.is_null() { None } else { Some(&obs) })
                    .map(|(a, b)| json!([a, b]))
            }
        };
        match r {
            Ok(v) => println!("LW {} OK {}", i, serde_json::to_string(&canon(&v)).unwrap()),
            Err(e) => println!("LW {} ERR {}", i, e),
        }
    }
}
