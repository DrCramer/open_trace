//! ID-X profile comparison semantics ("idx": true), derived from the decompiled ProfileConcordance classes.
use opentrace_core::profiles::profile_request;
use serde_json::{json, Value};

fn s(id: &str, m: Value) -> Value {
    json!({"id": id, "label": format!("W | {} | {}", id, id), "markers": m})
}
fn run(req: Value) -> Value {
    profile_request(&req).unwrap()
}

#[test]
fn percent_counts_only_markers_present_in_comparison() {
    // reference has 3 markers (6 alleles); the comparison only knows 2 markers: ID-X ignores the third.
    let a = s("A", json!({"D3": ["14","15"], "vWA": ["16","17"], "FGA": ["20","21"]}));
    let b = s("B", json!({"D3": ["14","15"], "vWA": ["16","18"]}));
    let r = run(json!({"action":"compare","mode":"samples","idx":true,"threshold":50,"samples":[a,b]}));
    let rows = r["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    // A->B: 3 of 4 shared reference alleles = 75 %; B->A: 3 of 4 = 75 %; the pair is reported once
    assert_eq!(rows[0]["percent"], json!(75.0));
    assert_eq!(rows[0]["total"], json!(4));
}

#[test]
fn legacy_mode_unchanged() {
    let a = s("A", json!({"D3": ["14","15"], "vWA": ["16","17"], "FGA": ["20","21"]}));
    let b = s("B", json!({"D3": ["14","15"], "vWA": ["16","18"]}));
    let r = run(json!({"action":"compare","mode":"samples","threshold":50,"samples":[a,b]}));
    // legacy denominator includes the marker missing in B: A->B = 3/6 = 50; B->A = 3/4 = 75 (best direction)
    assert_eq!(r["rows"][0]["percent"], json!(75.0));
}

#[test]
fn groups_are_named_per_type_and_mixed_needs_two_triallelic_markers() {
    let mk = |tri: usize| {
        let mut m = serde_json::Map::new();
        for (i, n) in ["D3", "vWA", "FGA"].iter().enumerate() {
            m.insert(n.to_string(), if i < tri { json!(["1","2","3"]) } else { json!(["1","2"]) });
        }
        Value::Object(m)
    };
    // one triallelic marker is still a Single sample, two make it Mixed
    let samples = vec![s("S1", mk(1)), s("S2", mk(1)), s("M1", mk(2)), s("M2", mk(2))];
    let r = run(json!({"action":"compare","mode":"concordance","idx":true,"samples":samples}));
    let g = r["groups"].as_array().unwrap();
    assert_eq!(g.len(), 2);
    assert_eq!(g[0]["name"], json!("Single Source-Group 1"));
    assert_eq!(g[0]["kind"], json!("single"));
    assert_eq!(g[1]["name"], json!("Mixed Source-Group 1"));
}

#[test]
fn mixed_group_is_never_reference_of_single() {
    let single = s("S", json!({"D3": ["14","15"], "vWA": ["16","17"]}));
    let mixed = s("M", json!({"D3": ["14","15","16"], "vWA": ["16","17","18"]}));
    let r = run(json!({"action":"compare","mode":"samples","idx":true,"threshold":50,"samples":[single, mixed]}));
    let rows = r["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["section"], json!("single-mixed"));
    assert_eq!(rows[0]["reference"], json!("W | S | S"));
    assert_eq!(rows[0]["percent"], json!(100.0));
}

#[test]
fn lab_reference_uses_selected_groups_and_picks_higher_direction() {
    let a = s("A", json!({"D3": ["14","15"]}));
    let b = s("B", json!({"D3": ["14","15"]}));
    let c = s("C", json!({"D3": ["14","15","16"], "vWA": ["1","2","3"]}));
    let prof = json!({"id":"P1","type":"Lab Reference","markers":{"D3":["14","15"]}});
    let r = run(json!({"action":"compare","mode":"lab_reference","idx":true,"threshold":50,"samples":[a,b,c],"selected":["A","B"],"profiles":[prof]}));
    let rows = r["rows"].as_array().unwrap();
    // A and B form one group, C was not selected
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["comparison"], json!("Single Source-Group 1"));
    assert_eq!(rows[0]["reference"], json!("P1"));
}
