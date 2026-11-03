//! Counterpart of scratchpad/pr_check.py (project_runs scenarios).
use opentrace_core::project_runs as pr;
use opentrace_core::project_store::{self as ps, ProjectStore, R};
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
    for (si, line) in BufReader::new(file).lines().enumerate() {
        let ops = decode(&serde_json::from_str::<Value>(&line.unwrap()).unwrap());
        run(si, ops.as_array().unwrap());
    }
}

fn run(si: usize, ops: &[Value]) {
    ps::set_deterministic(true);
    let mut store: Option<ProjectStore> = None;
    let mut runs: Vec<String> = vec![];
    let mut revs: Vec<String> = vec![];
    let mut outs: Vec<String> = vec![];
    let mut last: Option<String> = None;
    let emit = |i: usize, v: Value| println!("PR {} {} OK {}", si, i, serde_json::to_string(&canon(&v)).unwrap());
    for (i, op) in ops.iter().enumerate() {
        let k = op["op"].as_str().unwrap();
        let g = |key: &str| op.get(key).cloned().unwrap_or(Value::Null);
        let res = |v: Value, runs: &Vec<String>, revs: &Vec<String>| -> Value {
            if v == json!("@run") {
                return json!(runs.last().cloned().unwrap_or("unresolved".into()));
            }
            if let Some(s) = v.as_str() {
                if let Some(r) = s.strip_prefix("@rev[") {
                    let j: i64 = r[..r.len() - 1].parse().unwrap();
                    let n = revs.len() as i64;
                    return if -n <= j && j < n { json!(revs[((j + n) % n) as usize]) } else { json!("unresolved") };
                }
            }
            v
        };
        let r: R<()> = (|| {
            match k {
                "new" => {
                    let s = ProjectStore::memory(&json!("P"))?;
                    emit(i, Value::Object(s.project()?));
                    store = Some(s);
                }
                "create" | "revise" | "ladder" => {
                    let s = store.as_ref().ok_or_else(|| ps::other("'NoneType' object has no attribute"))?;
                    let r = match k {
                        "create" => pr::create_project_run(s, &g("name"), &g("inputs"), &g("actor"), &g("reason"))?,
                        "revise" => pr::revise_project_run(s, &res(g("run_id"), &runs, &revs), &res(g("parent"), &runs, &revs), &g("inputs"), &g("actor"), &g("reason"))?,
                        _ => pr::change_run_ladder(s, &res(g("run_id"), &runs, &revs), &res(g("parent"), &runs, &revs), &g("ladder_id"), &g("analysis"), &g("remove"), &g("actor"), &g("reason"))?,
                    };
                    if k == "create" {
                        runs.push(r["run_id"].as_str().unwrap().to_string());
                    }
                    revs.push(r["revision_id"].as_str().unwrap().to_string());
                    outs = r["outputs"].as_array().unwrap().iter().map(|o| o["analysis_id"].as_str().unwrap().to_string()).collect();
                    emit(i, r);
                }
                "effective" => {
                    let kk = op["k"].as_u64().unwrap() as usize;
                    if kk < outs.len() {
                        emit(i, store.as_ref().unwrap().effective_analysis(&json!(outs[kk]))?);
                    }
                }
                "list_revs" => emit(i, Value::Array(store.as_ref().unwrap().list_run_revisions(&Value::Null)?)),
                "export" => {
                    let e = store.as_ref().unwrap().export_json()?;
                    last = Some(e.clone());
                    emit(i, json!(e));
                }
                "reimport" => {
                    let le = last.clone().unwrap();
                    let s2 = ProjectStore::from_json(&json!(le))?;
                    emit(i, json!(s2.export_json()? == le));
                }
                _ => panic!("{}", k),
            }
            Ok(())
        })();
        if let Err(e) = r {
            println!("PR {} {} ERR {}", si, i, e.msg);
        }
    }
}
