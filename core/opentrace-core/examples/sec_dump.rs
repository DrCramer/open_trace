//! Counterpart of scratchpad/sec_check.py (security scenarios). Usage: sec_dump req.jsonl base_dir
use opentrace_core::idx_projects::{LErr, LR};
use opentrace_core::matrix::b64decode_strict;
use opentrace_core::security::*;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

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
fn dumps(v: &Value) -> String {
    serde_json::to_string(&canon(v)).unwrap()
}
fn fnv(b: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for x in b {
        h = (h ^ *x as u64).wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", h)
}
fn resolve(v: &Value, ctx: &HashMap<String, Value>) -> Value {
    match v {
        Value::String(s) if s.starts_with('@') => ctx.get(s).cloned().unwrap_or_else(|| v.clone()),
        Value::Array(a) => Value::Array(a.iter().map(|x| resolve(x, ctx)).collect()),
        Value::Object(m) => Value::Object(m.iter().map(|(k, x)| (k.clone(), resolve(x, ctx))).collect()),
        o => o.clone(),
    }
}
fn state_line(root: &PathBuf, store: &SecurityStore) -> String {
    let mut parts: Vec<String> = Vec::new();
    for n in ["security.json", "audit.jsonl", "audit.key"] {
        let p = root.join(n);
        if p.exists() {
            let b = fs::read(&p).unwrap();
            parts.push(fnv(&b));
            if std::env::var("SEC_DUMP").is_ok() && n != "audit.key" {
                parts.push(format!("\n{}", String::from_utf8_lossy(&b)));
            }
        } else {
            parts.push("-".into());
        }
    }
    format!("{} T{}", parts.join(" "), store.tickets.len())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let base = PathBuf::from(&args[2]);
    set_iterations(50);
    let rd = BufReader::new(fs::File::open(&args[1]).unwrap());
    for (si, line) in rd.lines().enumerate() {
        let scen: Value = decode(&serde_json::from_str::<Value>(&line.unwrap()).unwrap());
        let start: i64 = 123456;
        let mut t: i64 = start;
        let epoch: i64 = 1704067200_000000;
        set_fake_time(Some(epoch + t));
        set_fake_rand(true);
        let root = base.join(format!("s{}", si));
        let mut store = SecurityStore::open(root.to_str().unwrap()).unwrap();
        let mut ctx: HashMap<String, Value> = HashMap::new();
        for (i, op) in scen.as_array().unwrap().iter().enumerate() {
            let kind = op["t"].as_str().unwrap();
            let res: LR<Value> = match kind {
                "adv" => {
                    t += op["us"].as_i64().unwrap();
                    set_fake_time(Some(epoch + t));
                    Ok(json!({"t": t}))
                }
                "reopen" => match SecurityStore::open(root.to_str().unwrap()) {
                    Ok(s) => {
                        store = s;
                        Ok(json!({"users": store.doc["users"].as_array().unwrap().len()}))
                    }
                    Err(e) => Err(e),
                },
                "fs" => {
                    if let Some(w) = op.get("write").and_then(|w| w.as_object()) {
                        for (rel, spec) in w {
                            fs::write(root.join(rel), b64decode_strict(spec.as_str().unwrap()).unwrap()).unwrap();
                        }
                    }
                    Ok(json!("fs"))
                }
                "oa" => (|| -> LR<Value> {
                    let user = store.user(&op["user"]);
                    let owner = op.get("owner").cloned().unwrap_or(Value::Null);
                    let allows = store.object_allows(user.as_ref(), &op["group"], op["op"].as_str().unwrap(), &owner);
                    let mut rights = store.object_rights(user.as_ref(), &op["group"]);
                    rights.sort();
                    let can = match op.get("fn").and_then(|f| f.as_str()) {
                        Some(f) => json!(store.can(user.as_ref(), f)?),
                        None => Value::Null,
                    };
                    let req = match store.require_object(user.as_ref(), &op["group"], op["op"].as_str().unwrap(), &owner) {
                        Ok(()) => "ok".to_string(),
                        Err(LErr::Denied(m)) => m,
                        Err(e) => return Err(e),
                    };
                    Ok(json!({"allows": allows, "rights": rights, "can": can, "req": req}))
                })(),
                _ => {
                    let ticket = resolve(&op["ticket"], &ctx);
                    let req = resolve(&op["req"], &ctx);
                    let r = security_request(&mut store, ticket.as_str(), &req);
                    if let Ok(res) = &r {
                        if let Some(tk) = res.get("ticket").and_then(|x| x.as_str()) {
                            if !tk.is_empty() {
                                ctx.insert("@ticket".into(), json!(tk));
                            }
                        }
                        if let Some(txt) = res.get("text").and_then(|x| x.as_str()) {
                            match req.get("action").and_then(|a| a.as_str()) {
                                Some("export") => {
                                    ctx.insert("@export".into(), json!(txt));
                                    ctx.insert("@export-mut".into(), json!(txt.replacen("\"profile\": \"Admin Profile\"", "\"profile\": \"ZZZ\"", 1)));
                                }
                                Some("backup") => {
                                    ctx.insert("@backup".into(), json!(txt));
                                }
                                _ => {}
                            }
                        }
                        if let Some(x) = res.get("xml").and_then(|x| x.as_str()) {
                            ctx.insert("@maps".into(), json!(x));
                        }
                    }
                    r
                }
            };
            match res {
                Ok(v) => println!("SC {} {} OK {}", si, i, dumps(&v)),
                Err(LErr::Denied(m)) => println!("SC {} {} ERR D {}", si, i, m),
                Err(LErr::Value(m)) => println!("SC {} {} ERR V {}", si, i, m),
                Err(LErr::Other(_)) => println!("SC {} {} ERR O", si, i),
            }
            println!("SC {} {} STATE {}", si, i, state_line(&root, &store));
        }
    }
}
