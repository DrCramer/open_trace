//! The synthetic kit (`demo::synthetic_kit_texts`) as the desktop's laboratory standards import stores it: kit, bin set
//! and stutter contents in the Panel Manager library shape (parsed with the core's GeneMapper parsers). Used to set up
//! a test server with an approved profile for the genotyping self-test: `cargo run --example synthetic_kit_store`.
use opentrace_core::gm_kit_import::{parse_bins, parse_panels, parse_stutter};
use serde_json::json;

fn main() {
    let (kit, _panel, panels, bins, stutter) = opentrace_core::demo::synthetic_kit_texts();
    let p = parse_panels(&json!(panels)).unwrap();
    let b = parse_bins(&json!(bins)).unwrap();
    let s = parse_stutter(&json!(stutter)).unwrap();
    let k = &p["kits"][0];
    let kit_content = json!({"name": kit, "type": p.get("kit_type").cloned().unwrap_or(json!("MICROSATELLITE")), "comment": "", "panelsVersion": p["version"],
        "panels": k["panels"].as_array().unwrap().iter().map(|x| json!({"name": x["name"], "comment": "", "markers": x["markers"]})).collect::<Vec<_>>()});
    let set_name = b["bin_set"].as_str().map(String::from).unwrap_or_else(|| format!("{}_Bins", kit));
    let out = json!({
        "kit": kit_content,
        "bin_set_name": format!("{} · {}", kit, set_name),
        "bin_set": {"kit": kit, "name": set_name, "version": b["version"], "panels": b["panels"]},
        "stutter": {"kit": kit, "stutter": {"version": s["version"], "panels": s["panels"]}},
    });
    println!("{}", serde_json::to_string(&out).unwrap());
}
