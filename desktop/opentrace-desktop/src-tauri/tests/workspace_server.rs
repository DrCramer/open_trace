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

    // The FSA went over as binary and comes back byte-identical.
    let smp = b.handle(&json!({"action": "open_sample", "name": "Case 1", "id": "1"})).unwrap();
    assert_eq!(opentrace_core::matrix::b64decode_strict(smp["fsa_b64"].as_str().unwrap()).unwrap(), fsa);

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
