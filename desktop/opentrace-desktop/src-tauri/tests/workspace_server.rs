//! ProjectWorkspace against a real OpenTrace Server (in-process, real HTTP on 127.0.0.1): two experts on one project.
use opentrace_desktop_lib::remote::Remote;
use opentrace_desktop_lib::workspace::Workspace;
use serde_json::{json, Value};
use std::sync::Arc;

struct Server {
    url: String,
    _dir: tempfile::TempDir,
    _rt: tokio::runtime::Runtime,
}

fn server() -> Server {
    let dir = tempfile::tempdir().unwrap();
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let listener = rt.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
    let port = listener.local_addr().unwrap().port();
    let cfg = opentrace_server::config::ServerConfig {
        data_dir: dir.path().to_path_buf(),
        listen: format!("127.0.0.1:{}", port),
        transport: opentrace_server::config::Transport::Http,
        lease_ttl_seconds: 60,
        ..Default::default()
    };
    let st = opentrace_server::build_state(cfg, port, true).unwrap();
    {
        let mut store = st.legacy.security_store();
        for name in ["Casework Analyst", "Databasing Analyst", "Lab Director", "Casework Tech"] {
            let mut u = store.user(&json!(name)).unwrap();
            u["password"] = json!("ws-secret-1");
            store.save_user(&u, &json!(name)).unwrap();
        }
        store.save_group(&json!({"name": opentrace_server::auth::APPROVER_GROUP, "description": "", "members": ["Lab Director"]}), &json!("")).unwrap();
    }
    let app = opentrace_server::router(st).into_make_service_with_connect_info::<std::net::SocketAddr>();
    rt.spawn(async move { axum::serve(listener, app).await.unwrap() });
    Server { url: format!("http://127.0.0.1:{}", port), _dir: dir, _rt: rt }
}

fn client(s: &Server, user: &str) -> (Arc<Remote>, Workspace) {
    let r = Arc::new(Remote::new(false));
    r.login(&s.url, None, user, "ws-secret-1").unwrap();
    let w = Workspace::new(r.clone(), Arc::new(|| None));
    (r, w)
}

fn meta(comment: &str, create: bool) -> Value {
    json!({"action": "save_meta", "name": "Case 1", "project": {"samples": [{"id": "1", "file": "a.fsa", "name": "a", "status": "analyzed"}], "comment": comment},
           "create": create, "overwrite": !create})
}

#[test]
fn two_experts_one_editor_read_only_conflicts_and_binary_fsa() {
    let s = server();
    let (_ra, a) = client(&s, "Casework Analyst");
    let (_rb, b) = client(&s, "Databasing Analyst");
    let fsa = opentrace_core::demo::demo_bytes();

    // A creates the project on the server (lease taken implicitly) and saves a sample with its FSA.
    a.handle(&meta("v1", true)).unwrap();
    a.handle(&json!({"action": "save_sample", "name": "Case 1", "id": "1",
                     "fsa_b64": opentrace_core::matrix::b64encode(&fsa), "data": {"analysis": {"x": 1}}})).unwrap();
    assert!(a.status()["editing"].as_bool().unwrap());

    // B sees it in the list with A as the editor, opens it read-only and cannot save.
    let list = b.handle(&json!({"action": "list"})).unwrap();
    let row = list["projects"].as_array().unwrap().iter().find(|p| p["name"] == "Case 1").unwrap().clone();
    assert_eq!(row["lease"]["user"], "Casework Analyst");
    let opened = b.open("Case 1").unwrap();
    assert_eq!(opened["comment"], "v1");
    assert!(opened["opentrace_server"]["readonly"].as_str().unwrap().contains("Casework Analyst"));
    let e = b.handle(&meta("from B", false)).unwrap_err();
    assert_eq!(e.status, 409);
    assert!(e.body["error"].as_str().unwrap().contains("только для чтения"), "{}", e.body);

    // The FSA went over as binary; opening returns its SHA-256 only (the bridge fills the bytes from the local cache
    // or one download), and the download is byte-identical.
    let smp = b.handle(&json!({"action": "open_sample", "name": "Case 1", "id": "1"})).unwrap();
    let sha = smp["fsa_sha256"].as_str().expect("fsa by sha").to_string();
    assert!(smp["fsa_b64"].is_null());
    assert_eq!(_rb.fsa_get(None, &sha).unwrap(), fsa);
    // Large bodies went gzip-compressed once the server had advertised it.
    assert_eq!(_ra.wire_status()["gzip_requests"], true);

    // A closes (releases); B re-opens and now edits.
    a.release_all();
    b.open("Case 1").unwrap();
    assert!(b.status()["editing"].as_bool().unwrap());
    b.handle(&meta("v2 by B", false)).unwrap();

    // A opens meanwhile: read-only because B edits.
    let ao = a.open("Case 1").unwrap();
    assert!(ao["opentrace_server"]["readonly"].is_string());
    assert_eq!(ao["comment"], "v2 by B");

    // Admin force-releases B's lease: B's heartbeat notices, B's next save is refused (fenced), nothing overwritten.
    let admin = Remote::new(false);
    admin.login(&s.url, None, "Lab Director", "ws-secret-1").unwrap();
    admin.post_json(None, "/api/v1/projects/Case%201/lease/force-release", &[], &json!({"reason": "test"})).unwrap();
    b.renew_all();
    assert!(b.status()["readonly"].as_str().unwrap().contains("потеряна"));
    assert_eq!(b.handle(&meta("late B", false)).unwrap_err().status, 409);

    // A takes over; a stale revision (simulated) is reported as a conflict, not written.
    a.open("Case 1").unwrap();
    a.handle(&meta("v3 by A", false)).unwrap();
    let viewer_rev = b.open("Case 1").unwrap()["opentrace_server"]["revision"].as_i64().unwrap();
    assert!(viewer_rev >= 4);

    // Read-only user (technician) never gets an edit lease.
    let (_rt, t) = client(&s, "Casework Tech");
    assert!(t.open("Case 1").unwrap()["opentrace_server"]["readonly"].as_str().unwrap().contains("Нет права"));

    // Destructive library actions are not offered from the desktop in server mode.
    assert_eq!(a.handle(&json!({"action": "delete", "name": "Case 1"})).unwrap_err().status, 400);
}

#[test]
fn revision_conflict_after_concurrent_web_change_turns_read_only() {
    let s = server();
    let (_ra, a) = client(&s, "Casework Analyst");
    a.handle(&meta("v1", true)).unwrap();
    a.release_all();
    // A opens (lease, revision r). The lease is then force-released and somebody else writes; A's save must fail.
    a.open("Case 1").unwrap();
    let admin = Remote::new(false);
    admin.login(&s.url, None, "Lab Director", "ws-secret-1").unwrap();
    admin.post_json(None, "/api/v1/projects/Case%201/lease/force-release", &[], &json!({"reason": "test"})).unwrap();
    let (_rb, b) = client(&s, "Databasing Analyst");
    b.open("Case 1").unwrap();
    b.handle(&meta("B wins", false)).unwrap();
    b.release_all();
    let e = a.handle(&meta("A late", false)).unwrap_err();
    assert_eq!(e.status, 409, "{}", e.body);
    let (_rc, c) = client(&s, "Casework Analyst");
    assert_eq!(c.open("Case 1").unwrap()["comment"], "B wins");
}

#[test]
fn desktop_and_server_hash_standards_identically() {
    for v in [
        json!({"b": 1, "a": {"y": [1, 2.5, "x"], "x": null}, "é": "ü"}),
        json!({"threshold": 50.0, "list": [{"z": 1e-7, "a": 1e21}], "flag": true}),
        json!({}),
    ] {
        assert_eq!(opentrace_desktop_lib::canonical_json(&v), opentrace_server::standards::canonical(&v));
    }
}

#[test]
fn provenance_of_saved_samples_is_posted_once_and_never_guessed() {
    let s = server();
    let (r, a) = client(&s, "Casework Analyst");
    let fsa = opentrace_core::demo::demo_bytes();
    let other = opentrace_core::demo::synthetic_run_fsa(7);
    let sha = |b: &[u8]| opentrace_desktop_lib::provenance::sha256_hex(b);
    // The bridge notes every analysis; here directly: one file once, another one with two different settings.
    a.ledger.record(Some("threshold=100&filename=a.fsa&analysis_engine=residual"), &fsa, b"{\"result\":1}");
    a.ledger.record(Some("threshold=100"), &other, b"r1");
    a.ledger.record(Some("threshold=150"), &other, b"r2");
    a.handle(&meta("v1", true)).unwrap();
    let save = |id: &str, bytes: &[u8]| {
        a.handle(&json!({"action": "save_sample", "name": "Case 1", "id": id, "fsa_b64": opentrace_core::matrix::b64encode(bytes),
                         "data": {"analysis": {"source_sha256": sha(bytes), "channels": []}}}))
            .unwrap()
    };
    save("1", &fsa);
    save("1", &fsa); // saved again unchanged: not posted twice
    save("2", &other); // ambiguous: nothing posted
    let list = r.get_json(None, "/api/v1/projects/Case%201/analyses").unwrap();
    let rows = list["analyses"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{}", list);
    assert_eq!(rows[0]["sample_id"], "1");
    assert_eq!(rows[0]["fsa_sha256"], sha(&fsa));
    assert_eq!(rows[0]["result_sha256"], sha(b"{\"result\":1}"));
    assert_eq!(rows[0]["settings"], json!({"analysis_engine": "residual", "threshold": "100"}));
    assert_eq!(rows[0]["analysis_version"], opentrace_core::server_analyze::analysis_version());
    assert_eq!(rows[0]["created_by"], "Casework Analyst");
    assert!(rows[0]["lab_profile"].is_null());
    let st = a.status()["provenance"].clone();
    assert_eq!((st["posted"].as_u64(), st["ambiguous"].as_u64(), st["failed"].as_u64()), (Some(1), Some(1), Some(0)), "{}", st);
}

/// Draft by `author`, submitted, approved by the Lab Director.
fn approved(author: &Remote, director: &Remote, kind: &str, name: &str, content: Value) {
    let path = format!("/api/v1/standards/{}/{}", kind, opentrace_desktop_lib::workspace::enc(name));
    let v = author.post_json(None, &path, &[], &json!({"content": content})).unwrap();
    let ver = v["version"].as_i64().unwrap();
    author.post_json(None, &format!("{}/{}/submit", path, ver), &[], &json!({})).unwrap();
    let a = director.post_json(None, &format!("{}/{}/approve", path, ver), &[], &json!({})).unwrap();
    assert_eq!(a["status"], "approved", "{}", a);
}

/// What the web UI sends for a settings object (`analyzeSettings`: strings, objects as JSON).
fn ui_query(settings: &serde_json::Map<String, Value>) -> String {
    let enc = |s: &str| opentrace_desktop_lib::workspace::enc(s);
    let mut q: Vec<String> = settings
        .iter()
        .map(|(k, v)| format!("{}={}", enc(k), enc(&match v { Value::String(s) => s.clone(), other => other.to_string() })))
        .collect();
    q.push("filename=x.fsa".into());
    q.join("&")
}

#[test]
fn analyses_are_attributed_to_the_lab_profile_only_with_its_exact_settings() {
    let s = server();
    let (ra, a) = client(&s, "Casework Analyst");
    let director = Remote::new(false);
    director.login(&s.url, None, "Lab Director", "ws-secret-1").unwrap();
    let shipped: Value = serde_json::from_slice(&std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../resources/data/shipped_methods.json")).unwrap()).unwrap();
    let method = shipped["methods"][0].clone();
    approved(&ra, &director, "analysis_method", method["name"].as_str().unwrap(), method.clone());
    approved(&ra, &director, "size_standard", "CE_G5_HID_GS500", json!({"name": "CE_G5_HID_GS500", "dye": "orange", "sizes": [75, 100, 139, 150, 160, 200, 300, 340, 350, 400, 450], "description": ""}));
    approved(&ra, &director, "kit", "Kit A", json!({"name": "Kit A", "markers": []}));
    approved(&ra, &director, "bin_set", "Kit A · Bins", json!({"kit": "Kit A", "name": "Bins"}));
    let members = json!([
        {"kind": "analysis_method", "name": method["name"], "version": 1}, {"kind": "size_standard", "name": "CE_G5_HID_GS500", "version": 1},
        {"kind": "kit", "name": "Kit A", "version": 1}, {"kind": "bin_set", "name": "Kit A · Bins", "version": 1}]);
    approved(&ra, &director, "lab_profile", "Lab GF", json!({"members": members}));

    let dir = tempfile::tempdir().unwrap();
    let core = opentrace_core::server::App::new(&opentrace_core::server::Config {
        projects_dir: Some(dir.path().join("p")), security_dir: Some(dir.path().join("s")), ..Default::default()
    })
    .unwrap();
    // A draft or a missing version cannot be activated.
    assert!(opentrace_desktop_lib::labprofile::resolve(&ra, None, &core, "Lab GF", 2).is_err());
    let active = opentrace_desktop_lib::labprofile::resolve(&ra, None, &core, "Lab GF", 1).unwrap();
    assert_eq!((active.method.as_str(), active.size_standard.as_str()), (method["name"].as_str().unwrap(), "CE_G5_HID_GS500"));
    assert!(active.expected.contains_key("standard_bp"), "{:?}", active.expected.keys().collect::<Vec<_>>());
    *a.profile.lock().unwrap() = Ok(Some(active.clone()));

    let f1 = opentrace_core::demo::synthetic_run_fsa(1);
    let f2 = opentrace_core::demo::synthetic_run_fsa(2);
    a.ledger.record(Some(&ui_query(&active.expected)), &f1, b"res1");
    let mut changed = active.expected.clone();
    changed.insert("threshold".into(), json!(9999)); // manual change: not the profile any more
    a.ledger.record(Some(&ui_query(&changed)), &f2, b"res2");
    a.handle(&meta("v1", true)).unwrap();
    for (id, f) in [("1", &f1), ("2", &f2)] {
        a.handle(&json!({"action": "save_sample", "name": "Case 1", "id": id, "fsa_b64": opentrace_core::matrix::b64encode(f),
                         "data": {"analysis": {"source_sha256": opentrace_desktop_lib::provenance::sha256_hex(f)}}}))
            .unwrap();
    }
    let rows = ra.get_json(None, "/api/v1/projects/Case%201/analyses").unwrap()["analyses"].as_array().unwrap().clone();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["lab_profile"], "Lab GF");
    assert_eq!(rows[0]["lab_profile_version"], 1);
    assert_eq!(rows[0]["lab_profile_sha256"], active.content_sha256.as_str());
    assert!(rows[1]["lab_profile"].is_null(), "{}", rows[1]);
    let st = a.status()["provenance"].clone();
    assert_eq!(st["profile_mismatch"], 1);
    assert!(st["last_mismatch"].as_str().unwrap().contains("threshold"), "{}", st);
}

const PANELS: &str = "Version\tGMID-X v 1.4\r\nKit type:\tMICROSATELLITE\r\nChemistry Kit\tKit G\tnull\r\nPanel\tP1\tnull\r\nM1\tblue\t100.0\t140.0\tnone\t4\tnone\tfalse\t10, 11, 12, \r\n";
fn bins_text(center: &str) -> String {
    format!("Version\tGMID-X v 1.2\r\nChemistry Kit\tKit G\r\nBinSet Name\tBins G\r\nPanel Name\tP1\r\nMarker Name\tM1\r\n10\t{}\t0.5\t0.5\r\n11\t108.0\t0.5\t0.5\r\n12\t112.0\t0.5\t0.5\r\n", center)
}

/// JSON round trip through JavaScript: integral floats lose their `.0`.
fn via_js(v: &Value) -> Value {
    fn walk(v: &Value) -> Value {
        match v {
            Value::Number(n) => match n.as_f64() {
                Some(f) if f.fract() == 0.0 && f.abs() < 1e15 => json!(f as i64),
                _ => v.clone(),
            },
            Value::Array(a) => Value::Array(a.iter().map(walk).collect()),
            Value::Object(m) => Value::Object(m.iter().map(|(k, x)| (k.clone(), walk(x))).collect()),
            _ => v.clone(),
        }
    }
    walk(v)
}

#[test]
fn genotyping_is_attributed_to_the_profile_only_with_its_kit_bins_and_method() {
    use opentrace_core::gm_kit_import::{import_request, parse_bins, parse_panels};
    let s = server();
    let (ra, a) = client(&s, "Casework Analyst");
    let director = Remote::new(false);
    director.login(&s.url, None, "Lab Director", "ws-secret-1").unwrap();
    let shipped: Value = serde_json::from_slice(&std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../resources/data/shipped_methods.json")).unwrap()).unwrap();
    let method = shipped["methods"][0].clone();
    // Library contents exactly as the Panel Manager stores parsed files (KitStore) and the standards page imports them.
    let p = parse_panels(&json!(PANELS)).unwrap();
    let pk = &p["kits"][0];
    let kit = json!({"name": "Kit G", "type": "MICROSATELLITE", "comment": "", "panelsVersion": p["version"],
                     "panels": pk["panels"].as_array().unwrap().iter().map(|x| json!({"name": x["name"], "comment": "", "markers": x["markers"]})).collect::<Vec<_>>()});
    let b = parse_bins(&json!(bins_text("104.0"))).unwrap();
    let bin_set = json!({"kit": "Kit G", "name": "Bins G", "version": b["version"], "panels": b["panels"]});
    approved(&ra, &director, "analysis_method", method["name"].as_str().unwrap(), method.clone());
    approved(&ra, &director, "size_standard", "CE_G5_HID_GS500", json!({"name": "CE_G5_HID_GS500", "dye": "orange", "sizes": [75, 100, 139, 150, 160, 200, 300, 340, 350, 400, 450], "description": ""}));
    approved(&ra, &director, "kit", "Kit G", kit);
    approved(&ra, &director, "bin_set", "Kit G · Bins G", bin_set);
    let members = json!([
        {"kind": "analysis_method", "name": method["name"], "version": 1}, {"kind": "size_standard", "name": "CE_G5_HID_GS500", "version": 1},
        {"kind": "kit", "name": "Kit G", "version": 1}, {"kind": "bin_set", "name": "Kit G · Bins G", "version": 1}]);
    approved(&ra, &director, "lab_profile", "Lab G", json!({"members": members}));
    let dir = tempfile::tempdir().unwrap();
    let core = opentrace_core::server::App::new(&opentrace_core::server::Config {
        projects_dir: Some(dir.path().join("p")), security_dir: Some(dir.path().join("s")), ..Default::default()
    })
    .unwrap();
    let active = opentrace_desktop_lib::labprofile::resolve(&ra, None, &core, "Lab G", 1).unwrap();
    *a.profile.lock().unwrap() = Ok(Some(active.clone()));

    // Two kit builds as the UI sends them (texts as KitStore.panelsText/binsText write them: `104`, not `104.0`;
    // checked against the real kits.js): from the approved library, and with one bin moved by 0.5 bp.
    let build = |bins: String| json!({"panels": PANELS, "bins": bins, "stutter": null, "panel": "P1", "analysis_method": method});
    let (good, bad) = (build(bins_text("104").replace("\t108.0\t", "\t108\t").replace("\t112.0\t", "\t112\t")), build(bins_text("104.5")));
    assert_eq!(opentrace_desktop_lib::labprofile::check_kit_build(&active, &good), Ok(()));
    assert!(opentrace_desktop_lib::labprofile::check_kit_build(&active, &bad).unwrap_err().contains("бины"));
    let norm = opentrace_desktop_lib::labprofile::norm_hash;
    for req in [&good, &bad] {
        let kit = import_request(req).unwrap()["kit"].clone();
        a.ledger.note_kit_build(norm(&kit), opentrace_desktop_lib::labprofile::check_kit_build(&active, req));
    }
    let kit_good = via_js(&import_request(&good).unwrap()["kit"]);
    let kit_bad = via_js(&import_request(&bad).unwrap()["kit"]);
    assert_ne!(norm(&kit_good), norm(&kit_bad));

    // Sample 1: profile settings, genotyped with the approved kit. Sample 2: same settings, genotyped with the moved bin.
    // Sample 3: genotyped with a kit never built while the profile was active.
    let files: Vec<Vec<u8>> = (11..14).map(opentrace_core::demo::synthetic_run_fsa).collect();
    for f in &files {
        a.ledger.record(Some(&ui_query(&active.expected)), f, f);
    }
    let sha = |b: &[u8]| opentrace_desktop_lib::provenance::sha256_hex(b);
    a.ledger.note_genotyping(&sha(&files[0]), norm(&kit_good));
    a.ledger.note_genotyping(&sha(&files[1]), norm(&kit_bad));
    a.ledger.note_genotyping(&sha(&files[2]), norm(&json!({"name": "other"})));
    a.handle(&meta("v1", true)).unwrap();
    for (i, f) in files.iter().enumerate() {
        a.handle(&json!({"action": "save_sample", "name": "Case 1", "id": (i + 1).to_string(), "fsa_b64": opentrace_core::matrix::b64encode(f),
                         "data": {"analysis": {"source_sha256": sha(f)}}}))
            .unwrap();
    }
    let rows = ra.get_json(None, "/api/v1/projects/Case%201/analyses").unwrap()["analyses"].as_array().unwrap().clone();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0]["lab_profile"], "Lab G", "{}", rows[0]);
    assert!(rows[1]["lab_profile"].is_null() && rows[2]["lab_profile"].is_null());
    let st = a.status()["provenance"].clone();
    assert_eq!(st["profile_mismatch"], 2, "{}", st);
    assert!(st["last_mismatch"].as_str().unwrap().contains("до выбора профиля"), "{}", st);
}
