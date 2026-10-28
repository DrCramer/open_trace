//! Counterpart of scratchpad/ip_check.py (idx_projects scenarios). Usage: ip_dump req.jsonl base_dir
use opentrace_core::idx_projects::{set_fake_clock, LErr, ProjectLibrary, LR};
use opentrace_core::matrix::{b64decode_strict, b64encode};
use serde_json::{json, Map, Value};
use std::fs;
use std::io::{BufRead, BufReader, Read};
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
fn gunzip(b: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    flate2::read::MultiGzDecoder::new(b).read_to_end(&mut out).ok()?;
    Some(out)
}
fn parse(b: &[u8]) -> Option<Value> {
    let t = std::str::from_utf8(b).ok()?;
    opentrace_core::pyjson::loads(t, false).ok()
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, Value)>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    entries.sort();
    for p in entries {
        let rel = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
        if p.is_dir() {
            out.push((rel.clone(), json!(["d", rel])));
            walk(root, &p, out);
        } else {
            let b = fs::read(&p).unwrap();
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let c = if name.ends_with(".json.gz") {
                match gunzip(&b).and_then(|r| parse(&r)) {
                    Some(v) => json!(["gz", v]),
                    None => json!(["badgz"]),
                }
            } else if name == "project.json" {
                match parse(&b) {
                    Some(v) => json!(["json", v]),
                    None => json!(["badjson"]),
                }
            } else {
                json!(["raw", b.len(), fnv(&b)])
            };
            out.push((rel.clone(), json!(["f", rel, c])));
        }
    }
}

fn tree(root: &Path) -> Value {
    if !root.exists() {
        return json!(["<noroot>"]);
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Value::Array(out.into_iter().map(|(_, v)| v).collect())
}

fn summarize_export(res: &Value) -> Value {
    let mut m = res.as_object().unwrap().clone();
    let pkg = m.remove("package_b64").unwrap();
    m.remove("bytes");
    let raw = b64decode_strict(pkg.as_str().unwrap()).unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(raw)).unwrap();
    let mut mem: Vec<(String, Value)> = Vec::new();
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let name = f.name().to_string();
        let ct = match f.compression() {
            zip::CompressionMethod::Stored => 0,
            zip::CompressionMethod::Deflated => 8,
            _ => -1,
        };
        let mut data = Vec::new();
        f.read_to_end(&mut data).unwrap();
        let c = if name.ends_with(".json.gz") {
            parse(&gunzip(&data).unwrap()).unwrap()
        } else if name.ends_with(".json") {
            parse(&data).unwrap()
        } else {
            json!(["raw", data.len(), fnv(&data)])
        };
        mem.push((name, json!([ct, c])));
    }
    mem.sort_by(|a, b| a.0.cmp(&b.0));
    let mut mm = Map::new();
    for (k, v) in mem {
        mm.insert(k, v);
    }
    m.insert("members".into(), Value::Object(mm));
    Value::Object(m)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let base = PathBuf::from(&args[2]);
    let rd = BufReader::new(fs::File::open(&args[1]).unwrap());
    for (si, line) in rd.lines().enumerate() {
        let scen: Value = decode(&serde_json::from_str::<Value>(&line.unwrap()).unwrap());
        set_fake_clock(true);
        let sdir = base.join(format!("s{}", si));
        let root = sdir.join("lib");
        let root_arg = if si % 3 == 0 { format!("{}//./lib//", sdir.display()) } else { root.display().to_string() };
        let lib = ProjectLibrary::new(Some(Path::new(&root_arg)));
        for (i, op) in scen.as_array().unwrap().iter().enumerate() {
            if op["t"] == "fs" {
                if let Some(w) = op.get("write").and_then(|w| w.as_object()) {
                    for (rel, spec) in w {
                        let p = root.join(rel);
                        fs::create_dir_all(p.parent().unwrap()).unwrap();
                        fs::write(&p, b64decode_strict(spec.as_str().unwrap()).unwrap()).unwrap();
                    }
                }
                if let Some(r) = op.get("remove").and_then(|w| w.as_array()) {
                    for rel in r {
                        let p = root.join(rel.as_str().unwrap());
                        if p.is_dir() {
                            fs::remove_dir_all(&p).unwrap();
                        } else if p.exists() {
                            fs::remove_file(&p).unwrap();
                        }
                    }
                }
                println!("IP {} {} FS", si, i);
            } else {
                let deny: Option<Vec<String>> = op.get("deny").and_then(|d| d.as_array()).map(|a| a.iter().map(|x| x.as_str().unwrap().to_string()).collect());
                let acc = |o: &str, g: &Value, _ow: &Value, check: bool| -> LR<bool> {
                    let gs = match g {
                        Value::String(s) => s.clone(),
                        Value::Null => "None".into(),
                        other => other.to_string(),
                    };
                    let d = deny.as_ref().unwrap();
                    if d.iter().any(|x| x == o) || d.iter().any(|x| *x == format!("{}:{}", o, gs)) {
                        if check {
                            return Ok(false);
                        }
                        return Err(LErr::Denied(format!("denied {}", o)));
                    }
                    Ok(true)
                };
                let access: Option<&dyn Fn(&str, &Value, &Value, bool) -> LR<bool>> = if deny.is_some() { Some(&acc) } else { None };
                let user = op.get("user").and_then(|u| u.as_str());
                let req = &op["req"];
                match lib.request(req, access, user) {
                    Ok(mut res) => {
                        if req.get("action").and_then(|a| a.as_str()) == Some("export") {
                            res = summarize_export(&res);
                        }
                        if let Some(r) = res.get_mut("root") {
                            if let Some(s) = r.as_str() {
                                *r = Value::String(s.replace(&base.display().to_string(), "<base>"));
                            }
                        }
                        println!("IP {} {} OK {}", si, i, dumps(&res));
                    }
                    Err(LErr::Denied(m)) => println!("IP {} {} ERR D {}", si, i, m),
                    Err(LErr::Value(m)) => println!("IP {} {} ERR V {}", si, i, m),
                    Err(LErr::Other(_)) => println!("IP {} {} ERR O", si, i),
                }
            }
            println!("IP {} {} TREE {}", si, i, dumps(&tree(&root)));
        }
    }
    let _ = b64encode;
}
