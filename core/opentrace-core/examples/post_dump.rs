//! Rust counterpart of scratchpad/post_check.py. Lines: `PS <i> OK <canonical json>` or `PS <i> ERR <message>`.
use opentrace_core::calibration::build_calibration;
use opentrace_core::post_sizing_quality as ps;
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

fn run(req: &Value) -> Result<Value, String> {
    let g = |k: &str| req.get(k).cloned().unwrap_or(Value::Null);
    match req["fn"].as_str().unwrap() {
        "def" => ps::load_sizing_quality_definition(&g("definition")),
        _ => {
            let cal = if g("anchors").is_null() { None } else { Some(build_calibration(&g("anchors"), Some(&g("model")))?) };
            ps::evaluate_post_sizing_quality(&g("base"), &g("peaks"), cal.as_ref(), &g("definition"), &g("reference_channel"))
        }
    }
}

fn main() {
    let file = File::open(std::env::args().nth(1).expect("requests file")).unwrap();
    for (i, line) in BufReader::new(file).lines().enumerate() {
        let req = decode(&serde_json::from_str::<Value>(&line.unwrap()).unwrap());
        match run(&req) {
            Ok(v) => println!("PS {} OK {}", i, serde_json::to_string(&canon(&v)).unwrap()),
            Err(e) => println!("PS {} ERR {}", i, e),
        }
    }
}
