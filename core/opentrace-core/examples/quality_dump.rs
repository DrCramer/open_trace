//! Rust counterpart of scratchpad/quality_check.py. Request kinds (field `fn`): cat, profile, composite, positive,
//! zero, negative, dims, broad, round. Lines: `Q <i> OK <canonical json>` or `Q <i> ERR <message>`.
use opentrace_core::calibration::build_calibration;
use opentrace_core::quality_control as qc;
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
        "cat" => qc::quality_category(&g("score"), &g("good"), &g("bad")),
        "profile" => qc::profile_pullup_category(&g("values"), &g("threshold")),
        "composite" => qc::composite_control_category(&g("values")),
        "positive" => qc::positive_control(&g("called"), &g("expected"), &g("is_y")),
        "zero" => qc::zero_peak_metric(&g("count"), &g("called"), &g("sensor"), &g("expected")),
        "negative" => qc::negative_control(&g("value")),
        "dims" => {
            let anchors = g("anchors");
            let model = g("model");
            let cal = build_calibration(&anchors, Some(&model))?;
            qc::dimensions_in_bp(&g("peak"), &cal)
        }
        "broad" => qc::broad_standard_penalty(
            &g("score"),
            &g("peaks"),
            &g("reference_channel"),
            &g("warning_width_bp"),
            &g("weight"),
            &g("is_hid"),
            &g("sq_overridden"),
        ),
        "round" => {
            let x = g("x").as_f64().unwrap();
            Ok(serde_json::Number::from_f64(qc::round5(x)).map(Value::Number).unwrap_or(Value::Null))
        }
        other => Err(format!("unknown kind {}", other)),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let reader = BufReader::new(File::open(&args[1]).unwrap());
    for (i, line) in reader.lines().enumerate() {
        let raw: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let req = decode(&raw);
        match run(&req) {
            Ok(v) => println!("Q {} OK {}", i, serde_json::to_string(&canon(&v)).unwrap()),
            Err(e) => println!("Q {} ERR {}", i, e),
        }
    }
}
