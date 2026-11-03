//! Rust counterpart of scratchpad/pullup_check.py. Request kinds (field `fn`): load, apply, eval, attach.
//! Lines: `PU <i> OK <canonical json>` or `PU <i> ERR <message>`.
use opentrace_core::pullup as pu;
use opentrace_core::pullup_quality as pq;
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

fn run(req: &Value) -> Result<Value, String> {
    let g = |k: &str| req.get(k).cloned().unwrap_or(Value::Null);
    match req["fn"].as_str().unwrap() {
        "load" => pu::load_pullup_definition(&g("definition")),
        "apply" => pu::apply_pullup_detection(&g("channels"), &g("definition"), &g("selected_group"), &g("reference_channel")),
        "eval" => pq::evaluate_pullup_quality(&g("report"), &g("definition")),
        "attach" => pq::attach_pullup_quality(&g("report"), &g("definition")),
        other => Err(format!("unknown kind {}", other)),
    }
}

fn main() {
    let path = std::env::args().nth(1).expect("requests file");
    let file = File::open(path).expect("open requests");
    for (i, line) in BufReader::new(file).lines().enumerate() {
        let line = line.expect("read line");
        let req = decode(&serde_json::from_str::<Value>(&line).expect("json"));
        match run(&req) {
            Ok(v) => println!("PU {} OK {}", i, serde_json::to_string(&canon(&v)).unwrap()),
            Err(e) => println!("PU {} ERR {}", i, e),
        }
    }
}
