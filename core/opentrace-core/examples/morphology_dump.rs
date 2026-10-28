//! Rust counterpart of scratchpad/morph_check.py: prints canonical JSON (floats as bit hex) per request.
use opentrace_core::peak_morphology::{
    gaussian_fit_score_checked, gaussian_samples_checked, jag_metrics, measure, Definition,
};
use serde_json::{json, Map, Value};
use std::fs::File;
use std::io::{BufRead, BufReader};

fn decode(v: &Value) -> Value {
    match v {
        Value::Object(m) if m.len() == 1 && m.contains_key("__f") => {
            let hex = m["__f"].as_str().unwrap();
            let x = f64::from_bits(u64::from_str_radix(hex, 16).unwrap());
            match serde_json::Number::from_f64(x) {
                Some(n) => Value::Number(n),
                None => Value::String("nonfinite".into()),
            }
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

fn nums(v: &Value) -> Vec<f64> {
    v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
}

fn run(req: &Value) -> Result<Value, String> {
    let get = |k: &str| req.get(k).cloned().unwrap_or(Value::Null);
    match req["fn"].as_str().unwrap() {
        "jag" => {
            let j = jag_metrics(&get("values"), &get("threshold"))?;
            Ok(json!({"score": j.score, "count": j.count, "maximum": j.maximum}))
        }
        "gs" => {
            let (e, o) = gaussian_samples_checked(&get("values"), &get("width"), &get("height"), &get("center"))?;
            Ok(json!([e, o]))
        }
        "fit" => Ok(json!(gaussian_fit_score_checked(&get("values"), &get("width"), &get("height"), &get("center"))?)),
        "measure" => {
            let d = get("definition");
            let def = Definition {
                near_base_fraction: d["near_base_fraction"].as_f64().unwrap(),
                max_base_height: d["max_base_height"].as_f64().unwrap(),
                schema_version: d["schema_version"].as_i64().unwrap(),
                jag_threshold: d["morphology"]["jag_threshold"].as_f64().unwrap(),
            };
            measure(
                &nums(&get("signal")),
                get("lo").as_i64().unwrap(),
                get("hi").as_i64().unwrap(),
                get("center").as_i64().unwrap(),
                get("width").as_f64().unwrap(),
                &def,
            )
        }
        other => Err(format!("unknown fn {}", other)),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let reader = BufReader::new(File::open(&args[1]).unwrap());
    for (i, line) in reader.lines().enumerate() {
        let raw: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let req = decode(&raw);
        match run(&req) {
            Ok(v) => println!("M {} OK {}", i, serde_json::to_string(&canon(&v)).unwrap()),
            Err(e) => println!("M {} ERR {}", i, e),
        }
    }
}
