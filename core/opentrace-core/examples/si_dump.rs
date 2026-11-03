//! Counterpart of scratchpad/si_check.py: args = fsa paths; prints `name|OK|json` or `name|ERR|msg`.
use opentrace_core::sample_info::sample_info;
use serde_json::{json, Map, Value};

fn canon(v: &Value) -> Value {
    match v {
        Value::Number(n) if n.is_f64() => json!({"__f": format!("{:016x}", n.as_f64().unwrap().to_bits())}),
        Value::Array(a) => Value::Array(a.iter().map(canon).collect()),
        Value::Object(m) => Value::Object(m.iter().map(|(k, x)| (k.clone(), canon(x))).collect::<Map<String, Value>>()),
        other => other.clone(),
    }
}
fn main() {
    for path in std::env::args().skip(1) {
        let data = std::fs::read(&path).unwrap();
        let name = path.rsplit('/').next().unwrap().to_string();
        match sample_info(&data, &name) {
            Ok(v) => println!("{}|OK|{}", name, serde_json::to_string(&canon(&v)).unwrap()),
            Err(e) => println!("{}|ERR|{}", name, e),
        }
    }
}
