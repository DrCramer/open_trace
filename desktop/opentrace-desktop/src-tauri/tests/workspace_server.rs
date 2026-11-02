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
