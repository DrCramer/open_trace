//! Rust counterpart of scratchpad/am_check.py: runs every request line through `method_request`, prints canonical
//! JSON (floats as IEEE bit hex) or the error message, one line per request.
use opentrace_core::analysis_method::method_request;
use serde_json::{json, Map, Value};
use std::io::{BufRead, BufReader};
use std::path::Path;

fn canon(v: &Value) -> Value {
    match v {
        Value::Number(n) if n.is_f64() => json!({"__f": format!("{:016x}", n.as_f64().unwrap().to_bits())}),
        Value::Array(items) => Value::Array(items.iter().map(canon).collect()),
        Value::Object(map) => {
            let mut out = Map::new();
            for (k, x) in map {
                out.insert(k.clone(), canon(x));
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let requests = BufReader::new(std::fs::File::open(&args[1]).unwrap());
    let shipped = Path::new("/home/claude/ot/genemapper_open/data/shipped_methods.json");
    for (i, line) in requests.lines().enumerate() {
        let line = line.unwrap();
        let req: Value = serde_json::from_str(&line).unwrap();
        match method_request(&req, shipped) {
            Ok(res) => println!("R {} OK {}", i, serde_json::to_string(&canon(&res)).unwrap()),
            Err(e) => println!("R {} ERR {}", i, e),
        }
    }
}
