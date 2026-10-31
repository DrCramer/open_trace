//! Regression: the run workflow drops sample calls below the min-call-height floors from the root list only,
//! so marker quality must apply the same floors to the marker's own calls before comparing the two.
use opentrace_core::server::{App, Config, Request};
use serde_json::{json, Value};

fn post(app: &App, port: u16, path: &str, body: &Value) -> (u16, Value) {
    let headers = vec![
        ("host".to_string(), format!("127.0.0.1:{port}")),
        ("x-session-token".to_string(), app.token.clone()),
        ("content-type".to_string(), "application/json".to_string()),
    ];
    let req = Request { method: "POST".into(), target: path.into(), headers, body: serde_json::to_vec(body).unwrap() };
    let r = app.handle(&req);
    (r.code as u16, serde_json::from_slice(&r.body).unwrap())
}

#[test]
fn weak_marker_calls_below_the_floors_do_not_break_marker_quality() {
    let tmp = std::env::temp_dir().join(format!("opentrace-floors-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let port = 18102u16;
    let cfg = Config { port, projects_dir: Some(tmp.join("p")), security_dir: Some(tmp.join("s")), ..Config::default() };
    let app = App::new(&cfg).expect("app");
    let (code, demo) = post(&app, port, "/api/demo-str-run", &json!({}));
    assert_eq!(code, 200);
    let (report, def) = (demo["report"].clone(), demo["marker_quality_definition"].clone());
    let ok = |r: &Value| post(&app, port, "/api/marker-quality", &json!({"report": r, "definition": def}));
    assert_eq!(ok(&report).0, 200);

    let mut weak = report.clone();
    let marker = weak["allele_calling"]["markers"].as_array_mut().unwrap().iter_mut().find(|m| !m["calls"].as_array().unwrap().is_empty()).unwrap();
    let mut call = marker["calls"][0].clone();
    call["height"] = json!(5.0);
    call["peak_index"] = json!(0);
    call["label"] = json!("OL");
    call["size_bp"] = json!(1.5);
    marker["calls"].as_array_mut().unwrap().insert(0, call);
    assert!(weak["settings"]["call_thresholds"].is_object());
    assert_eq!(ok(&weak).0, 200, "floors recorded in the report are applied to the marker calls");

    weak["settings"].as_object_mut().unwrap().remove("call_thresholds");
    let (code, body) = ok(&weak);
    assert_ne!(code, 200, "without recorded floors the strict comparison remains");
    assert!(body["error"].as_str().unwrap().contains("inconsistent root and marker calls"));
    let _ = std::fs::remove_dir_all(&tmp);
}
