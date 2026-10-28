//! Counterpart of scratchpad/sv_check.py (sizing_value). Usage: sv_dump req.jsonl
use opentrace_core::sizing_value::{analyze_size_standard, SizeParams};
use serde_json::{json, Map, Value};
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
    let path = std::env::args().nth(1).unwrap();
    for (i, line) in BufReader::new(std::fs::File::open(path).unwrap()).lines().enumerate() {
        let r: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let values: Vec<i64> = r["values"].as_array().unwrap().iter().map(|v| v.as_i64().unwrap()).collect();
        let expected = decode(&r["expected"]);
        let p = decode(&r["p"]);
        let mut sp = SizeParams::defaults();
        let gi = |k: &str| p[k].as_i64().unwrap();
        let gf = |k: &str| p[k].as_f64().unwrap();
        sp.baseline_window = gi("baseline_window");
        sp.spike_window = gi("spike_window");
        sp.baseline_smoothing_window = gi("baseline_smoothing_window");
        sp.smooth_passes = gi("smooth_passes");
        sp.detection_threshold = gf("detection_threshold");
        sp.sg_window = gi("sg_window");
        sp.min_width = gf("min_width");
        sp.tau_begin = gf("tau_begin");
        sp.tau_end = gf("tau_end");
        sp.group_gap = p["group_gap"].clone();
        sp.ratio_factor = gf("ratio_factor");
        sp.min_accepted_score = gf("min_accepted_score");
        sp.ambiguity_margin = gf("ambiguity_margin");
        sp.allow_prefix = p["allow_prefix"].as_bool().unwrap();
        sp.max_bp_per_scan = p["max_bp_per_scan"].clone();
        sp.offscale_intervals = p["offscale_intervals"].clone();
        sp.height_mode = p["height_mode"].as_str().unwrap().to_string();
        sp.coordinate_mode = p["coordinate_mode"].as_str().unwrap().to_string();
        sp.sg_degree = gi("sg_degree");
        let out = analyze_size_standard(&values, &expected, gi("num_extra_peaks"), gf("quality_threshold"), &sp);
        let mut res = out.result;
        if let Some(c) = out.calibration {
            let anchors: Vec<Value> = c.anchors.iter().map(|&(a, b)| json!([a, b])).collect();
            res["calibration"] = json!({"anchors": anchors});
        }
        println!("{} OK {}", i, serde_json::to_string(&canon(&res)).unwrap());
    }
}
