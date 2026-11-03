//! Counterpart of scratchpad/ps_check.py: replays scenario files against `ProjectStore`.
use opentrace_core::project_store::{self as ps, ProjectStore, Review, R};
use opentrace_core::pyjson;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

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

struct Vars {
    analysis: Vec<String>,
    event: Vec<String>,
    revision: Vec<String>,
    prev: HashMap<String, Value>,
}

fn idx(list: &[String], s: &str) -> Value {
    let i: i64 = s.parse().unwrap();
    let n = list.len() as i64;
    if -n <= i && i < n {
        json!(list[((i + n) % n) as usize])
    } else {
        json!("unresolved")
    }
}

fn resolve(v: &Value, vars: &Vars) -> Value {
    match v {
        Value::String(s) if s.starts_with('@') => {
            if let Some(r) = s.strip_prefix("@analysis[") {
                return idx(&vars.analysis, &r[..r.len() - 1]);
            }
            if let Some(r) = s.strip_prefix("@event[") {
                return idx(&vars.event, &r[..r.len() - 1]);
            }
            if s == "@parent" {
                return json!(vars.revision.last().cloned().unwrap_or("unresolved".into()));
            }
            if let Some(k) = s.strip_prefix("@prev:") {
                return vars.prev.get(k).cloned().unwrap_or(json!("unresolved"));
            }
            v.clone()
        }
        Value::Array(a) => Value::Array(a.iter().map(|x| resolve(x, vars)).collect()),
        Value::Object(m) => Value::Object(m.iter().map(|(k, x)| (k.clone(), resolve(x, vars))).collect()),
        o => o.clone(),
    }
}

fn nonestore(name: &str) -> ps::SErr {
    ps::other(format!("'NoneType' object has no attribute '{}'", name))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let tmp = args[2].clone();
    let file = File::open(&args[1]).unwrap();
    for (si, line) in BufReader::new(file).lines().enumerate() {
        let ops = decode(&serde_json::from_str::<Value>(&line.unwrap()).unwrap());
        run(si, ops.as_array().unwrap(), &tmp);
    }
    for (j, d) in [Value::Null, json!(5), json!(""), json!("[]"), json!("{}"), json!("{\"format\":\"opentrace-local-project\"}"), json!("{\"a\":1,\"a\":2}"), json!("\u{feff}{}"), json!("xxxxxxxxxx")]
        .iter()
        .enumerate()
    {
        match ProjectStore::from_json(d) {
            Ok(_) => println!("RAW {} OK", j),
            Err(e) => println!("RAW {} ERR {}", j, e),
        }
    }
}

fn run(si: usize, ops: &[Value], tmp: &str) {
    ps::set_deterministic(true);
    let mut store: Option<ProjectStore> = None;
    let mut vars = Vars { analysis: vec![], event: vec![], revision: vec![], prev: HashMap::new() };
    let mut last_export: Option<String> = None;
    let emit = |i: usize, v: Value| println!("PS {} {} OK {}", si, i, serde_json::to_string(&canon(&v)).unwrap());
    for (i, op) in ops.iter().enumerate() {
        let op = resolve(op, &vars);
        let g = |k: &str| -> Value { op.get(k).cloned().unwrap_or(Value::Null) };
        let k = op["op"].as_str().unwrap().to_string();
        let r: R<()> = (|| {
            macro_rules! st {
                ($name:expr) => {
                    store.as_ref().ok_or_else(|| nonestore($name))?
                };
            }
            match k.as_str() {
                "new" => {
                    store = None;
                    let s = ProjectStore::memory(&g("name"))?;
                    emit(i, Value::Object(s.project()?));
                    store = Some(s);
                }
                "add_sample" => {
                    let s = st!("add_sample");
                    emit(i, json!(s.add_sample(&g("sha"), &g("meta"), &g("sample_id"), &g("created_at"))?));
                }
                "add_analysis" => {
                    let s = st!("add_analysis");
                    let r = s.add_analysis(&g("sample_id"), &g("input_sha"), &g("algorithm"), &g("settings"), &g("result"), &g("status"), &g("analysis_id"), &g("created_at"))?;
                    vars.analysis.push(r.clone());
                    emit(i, json!(r));
                }
                "review" => {
                    let s = st!("append_review");
                    let (aid, action, actor, reason, tid, val, eid, cat, rev) =
                        (g("analysis_id"), g("action"), g("actor"), g("reason"), g("target_id"), g("value"), g("event_id"), g("created_at"), g("reverses"));
                    let mut rv = Review::new(&aid, &action, &actor, &reason);
                    if op.get("target_kind").is_some() {
                        rv.target_kind = g("target_kind");
                    }
                    rv.target_id = &tid;
                    rv.value = &val;
                    rv.event_id = &eid;
                    rv.created_at = &cat;
                    rv.reverses_event_id = &rev;
                    let r = s.append_review(&rv)?;
                    vars.event.push(r.clone());
                    emit(i, json!(r));
                }
                "undo" => {
                    let s = st!("undo_review");
                    let r = s.undo_review(&g("analysis_id"), &g("event_id"), &g("actor"), &g("reason"))?;
                    vars.event.push(r.clone());
                    emit(i, json!(r));
                }
                "noop_forget" => {
                    vars.prev.remove(op["key"].as_str().unwrap());
                }
                "add_run" => {
                    let s = st!("add_run");
                    emit(i, json!(s.add_run(&g("name"), &g("run_id"), &g("created_at"))?));
                }
                "append_run" => {
                    let s = st!("append_run_revision");
                    let r = s.append_run_revision(&g("run_id"), &g("inputs"), &g("outputs"), &g("parent"), &g("actor"), &g("reason"), &g("revision_id"), &g("created_at"))?;
                    vars.revision.push(r.clone());
                    if let Some(a) = op["outputs"].as_array() {
                        for o in a {
                            if let Some(m) = o.as_object() {
                                if let Some(k) = m.get("sample_key").and_then(|k| k.as_str()) {
                                    vars.prev.insert(k.to_string(), m.get("analysis_id").cloned().unwrap_or(Value::Null));
                                }
                            }
                        }
                    }
                    emit(i, json!(r));
                }
                "effective" => emit(i, st!("effective_analysis").effective_analysis(&g("analysis_id"))?),
                "analysis_ctx" => emit(i, Value::Array(st!("analysis_run_contexts").analysis_run_contexts(&g("analysis_id"))?)),
                "get_analysis" => emit(i, st!("get_analysis").get_analysis(&g("analysis_id"))?),
                "get_run" => emit(i, st!("get_run").get_run(&g("run_id"))?),
                "list_samples" => emit(i, Value::Array(st!("list_samples").list_samples()?)),
                "list_analyses" => emit(i, Value::Array(st!("list_analyses").list_analyses()?)),
                "list_runs" => emit(i, Value::Array(st!("list_runs").list_runs()?)),
                "list_run_revisions" => emit(i, Value::Array(st!("list_run_revisions").list_run_revisions(&g("run_id"))?)),
                "review_events" => emit(i, Value::Array(st!("review_events").review_events(&g("analysis_id"))?)),
                "export" => {
                    let e = st!("export_json").export_json()?;
                    last_export = Some(e.clone());
                    emit(i, json!(e));
                }
                "reimport" => {
                    let le = last_export.clone().map_or(Value::Null, |s| json!(s));
                    let s2 = ProjectStore::from_json(&le)?;
                    let leq = last_export.clone().unwrap();
                    emit(i, json!(s2.export_json()? == leq));
                    let path = Path::new(tmp).join(format!("s{}.sqlite", si));
                    let _ = std::fs::remove_file(&path);
                    let s3 = ProjectStore::import_json(&path, &le)?;
                    emit(i, json!(s3.export_json()? == leq));
                    drop(s3);
                    let s4 = ProjectStore::open(&path)?;
                    emit(i, json!([s4.export_json()? == leq, s4.project()?["schema_version"].clone()]));
                    drop(s4);
                    let fix = |e: ps::SErr| json!(e.msg.replace(tmp, "<TMP>"));
                    if let Err(e) = ProjectStore::create(&path, &json!("dup")) {
                        emit(i, fix(e));
                    }
                    let missing: PathBuf = Path::new(tmp).join("missing.sqlite");
                    if let Err(e) = ProjectStore::open(&missing) {
                        emit(i, fix(e));
                    }
                    let _ = std::fs::remove_file(&path);
                    let cpath = Path::new(tmp).join(format!("c{}.sqlite", si));
                    let _ = std::fs::remove_file(&cpath);
                    let s5 = ProjectStore::create(&cpath, &json!("Created"))?;
                    emit(i, json!([s5.export_json()?, s5.project()?["schema_version"].clone()]));
                    drop(s5);
                    let _ = std::fs::remove_file(&cpath);
                }
                "upgraded" => {
                    let u = st!("upgraded").upgraded()?;
                    emit(i, json!(u.export_json()?));
                }
                "tamper" => {
                    let Some(le) = last_export.clone() else { return Err(ps::other("object of type 'NoneType' has no len()")) };
                    let t: Vec<char> = le.chars().collect();
                    let n = t.len();
                    let pos = n * op["permille"].as_u64().unwrap() as usize / 1000;
                    let text: Vec<char> = op["text"].as_str().unwrap().chars().collect();
                    let cat = |a: &[char], b: &[char], c: &[char]| -> String { a.iter().chain(b).chain(c).collect() };
                    let t2 = match op["kind"].as_str().unwrap() {
                        "insert" => cat(&t[..pos], &text, &t[pos..]),
                        "delete" => cat(&t[..pos], &[], &t[(pos + text.len()).min(n)..]),
                        "replace" => cat(&t[..pos], &text, &t[(pos + 1).min(n)..]),
                        _ => cat(&t[pos..], &[], &t[..pos]),
                    };
                    let s6 = ProjectStore::from_json(&json!(t2))?;
                    emit(i, json!(s6.export_json()? == le));
                }
                "tamper_field" => {
                    let Some(le) = last_export.clone() else {
                        return Err(ps::other("the JSON object must be str, bytes or bytearray, not NoneType"));
                    };
                    let mut d = pyjson::loads(&le, false).unwrap();
                    let w = op["which"].as_str().unwrap();
                    let nonempty = |d: &Value, k: &str| d[k].as_array().map_or(false, |a| !a.is_empty());
                    let text = if w == "sha" && nonempty(&d, "analyses") {
                        d["analyses"][0]["result_sha256"] = json!("0".repeat(64));
                        serde_json::to_string(&d).unwrap()
                    } else if w == "ts" && nonempty(&d, "samples") {
                        d["samples"][0]["created_at"] = json!("2024-01-01T00:00:00");
                        serde_json::to_string(&d).unwrap()
                    } else if w == "drop" {
                        d.as_object_mut().unwrap().shift_remove("samples");
                        serde_json::to_string(&d).unwrap()
                    } else if w == "version" {
                        d["schema_version"] = json!(3);
                        serde_json::to_string(&d).unwrap()
                    } else if w == "dupkey" {
                        "{\"format\":\"x\",\"format\":\"y\"}".to_string()
                    } else {
                        d["analyses"].as_array_mut().unwrap().reverse();
                        d["samples"].as_array_mut().unwrap().reverse();
                        serde_json::to_string(&d).unwrap()
                    };
                    let s7 = ProjectStore::from_json(&json!(text))?;
                    emit(i, json!(s7.export_json()? == le));
                }
                other => panic!("{}", other),
            }
            Ok(())
        })();
        if let Err(e) = r {
            println!("PS {} {} ERR {}", si, i, e.msg.replace(tmp, "<TMP>"));
        }
    }
}
