//! Rust counterpart of scratchpad/adjust_check.py. Request kinds (field `fn`):
//! validate(policy), ladder(peaks, observations, policy), sample(peaks, markers, policy), apply(peaks, summary),
//! marker(marker). Lines: `J <i> OK <canonical json>` or `J <i> ERR <message>`.
use opentrace_core::allele_calling::prepare_peaks;
use opentrace_core::spike_adjustment::{adjust_ladder_spikes, apply_peak_states, marker_has_spike, validate_adjustments, SpikeState};
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
    let policy = req.get("policy").cloned().unwrap_or(Value::Null);
    match req["fn"].as_str().unwrap() {
        "validate" => validate_adjustments(&policy),
        "ladder" => adjust_ladder_spikes(&req["peaks"], &req["observations"], &policy),
        "sample" => {
            let prepared = prepare_peaks(&req["peaks"])?;
            let mut state = SpikeState::new(prepared, &policy)?;
            let mut markers: Vec<Value> = req["markers"].as_array().cloned().unwrap_or_default();
            for marker in markers.iter_mut() {
                let map = marker.as_object_mut().ok_or("marker must be an object")?;
                state.after_sample_marker(map)?;
            }
            let summary = state.finish_sample(&markers)?;
            Ok(json!({"markers": markers, "summary": summary}))
        }
        "apply" => apply_peak_states(&req["peaks"], &req["summary"]),
        "marker" => marker_has_spike(&req["marker"]).map(|b| json!(b)),
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
            Ok(v) => println!("J {} OK {}", i, serde_json::to_string(&canon(&v)).unwrap()),
            Err(e) => println!("J {} ERR {}", i, e),
        }
    }
}
