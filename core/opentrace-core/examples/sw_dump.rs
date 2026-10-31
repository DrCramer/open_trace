//! Counterpart of scratchpad/ladwf_check.py. Kinds: def, gate. Lines `SW <i> OK|ERR`.
use opentrace_core::run_workflow as rw;
use opentrace_core::str_workflow as sw;
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
        let policy = req.get("policy").and_then(|p| p.as_str()).unwrap_or("reject").to_string();
        let qd = g("qd");
        let r = match req["fn"].as_str().unwrap() {
            "str" => sw::run_str_workflow(&g("sample"), &g("ladder"), &g("kit"), &policy, if qd.is_null() { None } else { Some(&qd) }),
            "analysis" => {
                let channels: std::collections::BTreeSet<String> = g("channels").as_array().unwrap().iter().map(|c| c.as_str().unwrap().to_string()).collect();
                sw::analysis(&g("report"), "sample", &channels)
            }
            fnname => {
                let opts = rw::RunOptions {
                    ladder_quality_definition: &qd,
                    missing_offset_policy: &policy,
                    marker_gate: g("marker_gate") == json!(true),
                    homozygous_min_height: req.get("hom").cloned().unwrap_or(json!(200.0)),
                    heterozygous_min_height: req.get("het").cloned().unwrap_or(json!(100.0)),
                };
                if fnname == "run" {
                    rw::run_str_run_workflow(&g("sample"), &g("ladders"), &g("kit"), &opts)
                } else {
                    rw::run_str_batch_workflow(&g("samples"), &g("ladders"), &g("kit"), &opts)
                }
            }
        };
        match r {
            Ok(v) => println!("SW {} OK {}", i, serde_json::to_string(&canon(&v)).unwrap()),
            Err(e) => println!("SW {} ERR {}", i, e),
        }
    }
}

