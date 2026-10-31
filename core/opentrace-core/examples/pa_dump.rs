//! Counterpart of scratchpad/pa_check.py (project_api scenarios).
use opentrace_core::project_api::project_action;
use opentrace_core::project_store as ps;
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

#[derive(Default)]
struct St {
    doc: Value,
    sel: Value,
    run: Value,
    calls: Vec<Value>,
    events: Vec<Value>,
    revs: Vec<Value>,
    anas: Vec<Value>,
}

fn pick(list: &[Value], token: &str) -> Value {
    let a = token.find('[').unwrap();
    let j: i64 = token[a + 1..token.len() - 1].parse().unwrap();
    let n = list.len() as i64;
    if n > 0 && -n <= j && j < n {
        list[((j + n) % n) as usize].clone()
    } else {
        json!("unresolved")
    }
}

fn resolve(v: &Value, st: &St) -> Value {
    match v {
        Value::String(s) if s.starts_with('@') => {
            if s == "@doc" {
                return st.doc.clone();
            }
            if s == "@sel" {
                return st.sel.clone();
            }
            if s == "@run" {
                return st.run.clone();
            }
            if s.starts_with("@call[") {
                return pick(&st.calls, s);
            }
            if s.starts_with("@event[") {
                return pick(&st.events, s);
            }
            if s.starts_with("@rev[") {
                return pick(&st.revs, s);
            }
            if s.starts_with("@ana[") {
                return pick(&st.anas, s);
            }
            v.clone()
        }
        Value::Array(a) => Value::Array(a.iter().map(|x| resolve(x, st)).collect()),
        Value::Object(m) => Value::Object(m.iter().map(|(k, x)| (k.clone(), resolve(x, st))).collect()),
        o => o.clone(),
    }
}

fn main() {
    let file = File::open(std::env::args().nth(1).unwrap()).unwrap();
    for (si, line) in BufReader::new(file).lines().enumerate() {
        let ops = decode(&serde_json::from_str::<Value>(&line.unwrap()).unwrap());
        ps::set_deterministic(true);
        let mut st = St::default();
        for (i, op) in ops.as_array().unwrap().iter().enumerate() {
            let req = if op.is_object() { resolve(op, &st) } else { op.clone() };
            match project_action(&req) {
                Ok(r) => {
                    st.doc = r["document"].clone();
                    st.sel = r["selected_analysis_id"].clone();
                    st.run = r["selected_run_id"].clone();
                    st.calls = match r["effective"].get("machine_result").and_then(|m| m.get("calls")).and_then(Value::as_array) {
                        Some(c) => c.iter().map(|x| x["id"].clone()).collect(),
                        None => vec![],
                    };
                    st.events = r["events"].as_array().unwrap().iter().map(|e| e["event_id"].clone()).collect();
                    st.revs = r["run_revisions"].as_array().unwrap().iter().map(|e| e["revision_id"].clone()).collect();
                    st.anas = r["analyses"].as_array().unwrap().iter().map(|e| e["analysis_id"].clone()).collect();
                    println!("PA {} {} OK {}", si, i, serde_json::to_string(&canon(&r)).unwrap());
                }
                Err(e) => println!("PA {} {} ERR {}", si, i, e.msg),
            }
        }
    }
}
