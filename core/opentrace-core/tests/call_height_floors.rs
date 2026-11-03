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

#[test]
fn str_run_accepts_configurable_floors() {
    let tmp = std::env::temp_dir().join(format!("opentrace-floors2-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let port = 18103u16;
    let cfg = Config { port, projects_dir: Some(tmp.join("p")), security_dir: Some(tmp.join("s")), ..Config::default() };
    let app = App::new(&cfg).expect("app");
    let (_, demo) = post(&app, port, "/api/demo-str-run", &json!({}));
    let run = |extra: Value| {
        let mut body = json!({"sample": demo["sample"], "ladders": demo["ladders"], "kit": demo["kit"],
            "ladder_quality_definition": demo["ladder_quality_definition"], "missing_offset_policy": "reject"});
        for (k, v) in extra.as_object().unwrap() {
            body[k] = v.clone();
        }
        post(&app, port, "/api/str-run", &body)
    };
    let (code, r) = run(json!({}));
    assert_eq!(code, 200);
    assert_eq!(r["settings"]["call_thresholds"], json!({"homozygous": 200.0, "heterozygous": 100.0}));
    let n_default = r["calls"].as_array().unwrap().len();
    assert!(n_default > 0);
    let (code, r) = run(json!({"homozygous_min_height": 1e9, "heterozygous_min_height": 1e9}));
    assert_eq!(code, 200);
    assert_eq!(r["settings"]["call_thresholds"]["homozygous"], json!(1e9));
    assert!(r["calls"].as_array().unwrap().is_empty(), "every call is below the floors");
    let (code, _) = run(json!({"homozygous_min_height": -1}));
    assert_eq!(code, 400);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn pull_up_peaks_are_called_only_when_the_option_says_so() {
    let tmp = std::env::temp_dir().join(format!("opentrace-pullup-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let port = 18104u16;
    let cfg = Config { port, projects_dir: Some(tmp.join("p")), security_dir: Some(tmp.join("s")), ..Config::default() };
    let app = App::new(&cfg).expect("app");
    let (_, demo) = post(&app, port, "/api/demo-str-run", &json!({}));
    let calls_of = |r: &Value| r["calls"].as_array().map_or(0, |c| c.len());
    let base = demo["report"].clone();
    let n = calls_of(&base);
    assert!(n > 0);
    // Flag the first called peak of the sample as pull-up in the compact analysis.
    let first_peak = base["calls"][0]["source_peak_id"].clone();
    let mut sample = demo["sample"].clone();
    let mut flagged = false;
    for ch in sample["channels"].as_array_mut().unwrap() {
        let tag = ch["tag"].clone();
        for p in ch["peaks"].as_array_mut().unwrap().iter_mut() {
            if !flagged && base["peaks"].as_array().unwrap().iter().any(|q| q["id"] == first_peak && q["scan"] == p["scan"] && q["channel"] == tag) {
                p["pullup"] = json!(true);
                flagged = true;
            }
        }
    }
    assert!(flagged, "demo peak found");
    let run = |excludes: bool| {
        let body = json!({"sample": sample, "ladders": demo["ladders"], "kit": demo["kit"],
            "ladder_quality_definition": demo["ladder_quality_definition"], "missing_offset_policy": "reject",
            "pullup_excludes_calls": excludes});
        post(&app, port, "/api/str-run", &body)
    };
    let (c1, r1) = run(true);
    let (c2, r2) = run(false);
    assert_eq!((c1, c2), (200, 200), "{r1} {r2}");
    assert!(calls_of(&r1) < n, "pull-up peak excluded by default");
    assert_eq!(calls_of(&r2), n, "pull-up peak called when the option is off");
    assert_eq!(r2["settings"]["pullup_excludes_calls"], json!(false));
    let _ = std::fs::remove_dir_all(&tmp);
}
