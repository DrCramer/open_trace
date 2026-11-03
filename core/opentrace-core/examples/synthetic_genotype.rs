//! Genotypes the synthetic run files (`demo::synthetic_genotype_fsa`) with the synthetic kit through the same core
//! requests the web UI sends (analyze, import-gm-kit, str-run) and compares the calls with the alleles put into the
//! files: `cargo run --release --example synthetic_genotype -- [N]`. Prints one JSON summary; exit code 1 on mismatch.
use opentrace_core::server::{App, Config, Request};
use serde_json::{json, Value};

fn post(app: &App, target: &str, body: Vec<u8>) -> Value {
    let req = Request {
        method: "POST".into(),
        target: target.into(),
        headers: vec![("host".into(), format!("127.0.0.1:{}", app.port)), ("x-session-token".into(), app.token.clone())],
        body,
    };
    let r = app.post(&req);
    let v: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
    assert_eq!(r.code, 200, "{} -> {}: {}", target, r.code, v);
    v
}

fn compact(a: &Value) -> Value {
    let mut out = serde_json::Map::new();
    for k in ["source_sha256", "analysis_version", "peak_settings", "sizing", "standard_definition_context", "pullup_detection"] {
        if let Some(v) = a.get(k) {
            out.insert(k.into(), v.clone());
        }
    }
    out.insert("channels".into(), Value::Array(a["channels"].as_array().unwrap().iter().map(|c| json!({"tag": c["tag"], "group": c["group"], "peaks": c["peaks"]})).collect()));
    Value::Object(out)
}

fn main() {
    let n: u64 = std::env::args().nth(1).and_then(|v| v.parse().ok()).unwrap_or(4);
    let tmp = std::env::temp_dir().join(format!("opentrace-synth-{}", std::process::id()));
    let app = App::new(&Config { port: 8765, projects_dir: Some(tmp.join("p")), security_dir: Some(tmp.join("s")), ..Default::default() }).unwrap();
    let shipped: Value = serde_json::from_slice(&std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources/data/shipped_methods.json")).unwrap()).unwrap();
    let method = shipped["methods"][0].clone();
    let gs500 = [75, 100, 139, 150, 160, 200, 300, 340, 350, 400, 450];
    let st = post(&app, "/api/analysis-method", serde_json::to_vec(&json!({"action": "analyze_settings", "method": method, "standard_bp": gs500, "standard_dye": "orange"})).unwrap());
    let q: Vec<String> = st["settings"].as_object().unwrap().iter().filter(|(_, v)| !v.is_null())
        .map(|(k, v)| format!("{}={}", k, url_enc(&match v { Value::String(s) => s.clone(), o => o.to_string() }))).collect();
    let q = q.join("&");
    let (_kit, panel, panels, bins, stutter) = opentrace_core::demo::synthetic_kit_texts();
    let built = post(&app, "/api/import-gm-kit", serde_json::to_vec(&json!({"panels": panels, "bins": bins, "stutter": stutter, "panel": panel, "analysis_method": method})).unwrap());
    let ladder = post(&app, &format!("/api/analyze?{}&filename=ladder.fsa", q), opentrace_core::demo::synthetic_ladder_fsa());
    let ladders = json!([{"ladder_id": "ladder.fsa", "analysis": compact(&ladder)}]);
    let (mut ok, mut bad) = (0, Vec::new());
    let mut first_report = Value::Null;
    for seed in 0..n {
        let (fsa, expected) = opentrace_core::demo::synthetic_genotype_fsa(seed);
        let a = post(&app, &format!("/api/analyze?{}&filename=s{}.fsa", q, seed), fsa);
        let rep = post(&app, "/api/str-run", serde_json::to_vec(&json!({"sample": compact(&a), "ladders": ladders, "kit": built["kit"],
            "ladder_quality_definition": built["ladder_quality_definition"], "missing_offset_policy": "forward_fill"})).unwrap());
        if first_report.is_null() {
            first_report = rep.clone();
        }
        for (marker, labels) in expected {
            let calls = calls_of(&rep, &marker);
            if calls == labels { ok += 1 } else { bad.push(json!({"seed": seed, "marker": marker, "expected": labels, "called": calls})) }
        }
    }
    let keys: Vec<String> = first_report.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
    println!("{}", json!({"markers_ok": ok, "mismatches": bad.len(), "first_mismatches": bad.iter().take(8).collect::<Vec<_>>(), "report_keys": keys}));
    let _ = std::fs::remove_dir_all(&tmp);
    if !bad.is_empty() {
        std::process::exit(1);
    }
}

fn url_enc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{:02X}", b) }).collect()
}

/// Allele labels called for a marker in a str-run report (`calls`: one entry per labelled peak; sorted, unique).
fn calls_of(rep: &Value, marker: &str) -> Vec<String> {
    let mut out: Vec<String> = rep["calls"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| c["marker"] == marker)
        .filter_map(|c| c["label"].as_str().map(String::from))
        .collect();
    out.sort();
    out.dedup();
    out
}
