//! End-to-end tests of API v1 and of the legacy web endpoints mounted next to it, through the real router.
use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use opentrace_server::config::{ServerConfig, Transport};
use opentrace_server::state::Shared;
use serde_json::{json, Value};
use std::net::SocketAddr;
use tower::ServiceExt;

const PORT: u16 = 8770;

struct Srv {
    st: Shared,
    app: Router,
    _dir: Option<tempfile::TempDir>,
}

fn cfg(dir: &std::path::Path) -> ServerConfig {
    ServerConfig { data_dir: dir.to_path_buf(), transport: Transport::Http, lease_ttl_seconds: 60, ..Default::default() }
}

fn start_in(dir: &std::path::Path) -> Srv {
    let st = opentrace_server::build_state(cfg(dir), PORT, true).unwrap();
    let app = opentrace_server::router(st.clone()).layer(MockConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))));
    Srv { st, app, _dir: None }
}

fn start() -> Srv {
    let dir = tempfile::tempdir().unwrap();
    let mut s = start_in(dir.path());
    set_passwords(&s.st);
    s._dir = Some(dir);
    s
}

fn set_passwords(st: &Shared) {
    let mut store = st.legacy.security_store();
    for name in ["Lab Director", "Casework Analyst", "Databasing Analyst", "Casework Tech", "Practice User"] {
        let mut u = store.user(&json!(name)).unwrap();
        u["password"] = json!("secret-pw-1");
        store.save_user(&u, &json!(name)).unwrap();
    }
    // Lab Director approves laboratory standards.
    store.save_group(&json!({"name": opentrace_server::auth::APPROVER_GROUP, "description": "", "members": ["Lab Director"]}), &json!("")).unwrap();
}

struct Resp {
    status: StatusCode,
    json: Value,
}

impl Srv {
    async fn call(&self, method: &str, uri: &str, token: Option<&str>, headers: &[(&str, String)], body: Body) -> Resp {
        let mut b = Request::builder().method(method).uri(uri);
        if let Some(t) = token {
            b = b.header("authorization", format!("Bearer {}", t));
        }
        for (k, v) in headers {
            b = b.header(*k, v);
        }
        let resp = self.app.clone().oneshot(b.body(body).unwrap()).await.unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Resp { status, json }
    }
    async fn get(&self, uri: &str, token: Option<&str>) -> Resp {
        self.call("GET", uri, token, &[], Body::empty()).await
    }
    async fn post(&self, uri: &str, token: Option<&str>, headers: &[(&str, String)], v: Value) -> Resp {
        let mut h = headers.to_vec();
        h.push(("content-type", "application/json".into()));
        self.call("POST", uri, token, &h, Body::from(v.to_string())).await
    }
    async fn login(&self, user: &str) -> String {
        let r = self.post("/api/v1/auth/login", None, &[], json!({"username": user, "password": "secret-pw-1", "client": "test"})).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.json);
        r.json["token"].as_str().unwrap().to_string()
    }
    /// Legacy web UI request (session token of the page + loopback Host, like the browser sends).
    async fn legacy(&self, v: Value) -> Resp {
        let h = [("host", format!("127.0.0.1:{}", PORT)), ("x-session-token", self.st.legacy.token.clone()), ("content-type", "application/json".into())];
        self.call("POST", "/api/idx-project", None, &h, Body::from(v.to_string())).await
    }
}

fn lease_headers(l: &Value, rev: i64) -> Vec<(&'static str, String)> {
    vec![
        ("x-lease-id", l["lease_id"].as_str().unwrap().to_string()),
        ("x-fencing-token", l["fencing_token"].to_string()),
        ("x-expected-revision", rev.to_string()),
    ]
}

fn save_meta(comment: &str, create: bool) -> Value {
    json!({"action": "save_meta", "project": {"samples": [], "comment": comment}, "create": create, "overwrite": !create})
}

#[tokio::test]
async fn health_capabilities_and_auth_required() {
    let s = start();
    let h = s.get("/api/v1/health", None).await;
    assert_eq!(h.status, StatusCode::OK);
    assert_eq!(h.json["api_version"], 1);
    assert_eq!(h.json["research_only"], true);
    assert!(s.get("/api/v1/capabilities", None).await.json["features"].as_array().unwrap().iter().any(|f| f == "projects.lease"));
    assert_eq!(s.get("/api/v1/projects", None).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(s.get("/api/v1/projects", Some("bogus")).await.status, StatusCode::UNAUTHORIZED);
    let bad = s.post("/api/v1/auth/login", None, &[], json!({"username": "Casework Analyst", "password": "wrong"})).await;
    assert_eq!(bad.status, StatusCode::UNAUTHORIZED);
    let t = s.login("Casework Analyst").await;
    let me = s.get("/api/v1/me", Some(&t)).await;
    assert!(me.json["roles"].as_array().unwrap().iter().any(|r| r == "expert"));
    let lo = s.post("/api/v1/auth/logout", Some(&t), &[], json!({})).await;
    assert_eq!(lo.status, StatusCode::OK);
    assert_eq!(s.get("/api/v1/me", Some(&t)).await.status, StatusCode::UNAUTHORIZED, "token revoked after logout");
    // Legacy web UI is still served.
    let page = s.call("GET", "/idx/", None, &[("host", format!("127.0.0.1:{}", PORT))], Body::empty()).await;
    assert_eq!(page.status, StatusCode::OK);
}

#[tokio::test]
async fn login_rate_limit() {
    let s = start();
    let mut last = StatusCode::OK;
    for _ in 0..12 {
        last = s.post("/api/v1/auth/login", None, &[], json!({"username": "nobody", "password": "x"})).await.status;
    }
    assert_eq!(last, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn two_editors_lease_revision_and_read_only() {
    let s = start();
    let alice = s.login("Casework Analyst").await;
    let bob = s.login("Databasing Analyst").await;
    let tech = s.login("Casework Tech").await;

    let la = s.post("/api/v1/projects/P1/lease", Some(&alice), &[], json!({})).await;
    assert_eq!(la.status, StatusCode::OK, "{}", la.json);
    let lb = s.post("/api/v1/projects/P1/lease", Some(&bob), &[], json!({})).await;
    assert_eq!(lb.status, StatusCode::CONFLICT);
    assert_eq!(lb.json["code"], "project_locked");
    assert_eq!(lb.json["locked_by"], "Casework Analyst");
    assert_eq!(lb.json["read_only_available"], true);
    // Viewer cannot take an edit lease at all.
    assert_eq!(s.post("/api/v1/projects/P1/lease", Some(&tech), &[], json!({})).await.status, StatusCode::FORBIDDEN);

    // Writes need the lease headers.
    assert_eq!(s.post("/api/v1/projects/P1/idx", Some(&alice), &[], save_meta("v1", true)).await.status, StatusCode::PRECONDITION_REQUIRED);
    let w = s.post("/api/v1/projects/P1/idx", Some(&alice), &lease_headers(&la.json, 0), save_meta("v1", true)).await;
    assert_eq!(w.status, StatusCode::OK, "{}", w.json);
    assert_eq!(w.json["revision"], 1);
    // Same base revision again -> conflict, no last-write-wins.
    let w2 = s.post("/api/v1/projects/P1/idx", Some(&alice), &lease_headers(&la.json, 0), save_meta("v1b", false)).await;
    assert_eq!(w2.status, StatusCode::CONFLICT);
    assert_eq!(w2.json["code"], "revision_conflict");
    // Bob (no lease) cannot write with Alice's lease id: lease belongs to her session.
    let wb = s.post("/api/v1/projects/P1/idx", Some(&bob), &lease_headers(&la.json, 1), save_meta("bob", false)).await;
    assert_eq!(wb.status, StatusCode::CONFLICT);

    // Read-only access works for everybody with READ_PROJECT while Alice edits.
    let ro = s.get("/api/v1/projects/P1", Some(&tech)).await;
    assert_eq!(ro.status, StatusCode::OK, "{}", ro.json);
    assert_eq!(ro.json["project"]["comment"], "v1");
    assert_eq!(ro.json["lease"]["user"], "Casework Analyst");
    assert_eq!(ro.json["revision"], 1);
    let list = s.get("/api/v1/projects", Some(&bob)).await;
    let p = list.json["projects"].as_array().unwrap().iter().find(|p| p["name"] == "P1").unwrap().clone();
    assert_eq!(p["lease"]["user"], "Casework Analyst");

    // The legacy web endpoint is refused while the lease is held (no bypass through the old UI).
    let web = s.legacy(json!({"action": "save_meta", "name": "P1", "project": {"samples": [], "comment": "web"}, "overwrite": true})).await;
    assert_eq!(web.status, StatusCode::CONFLICT, "{}", web.json);
    assert_eq!(web.json["code"], "project_locked");
    let web_rename = s.legacy(json!({"action": "rename", "name": "P1", "new_name": "P2"})).await;
    assert_eq!(web_rename.status, StatusCode::CONFLICT);
    let web_import = s.legacy(json!({"action": "import", "package_b64": "UEsFBgAAAAAAAAAAAAAAAAAAAAAAAA=="})).await;
    assert_eq!(web_import.status, StatusCode::CONFLICT, "import without explicit name while leased");
    // Web reads still work.
    assert_eq!(s.legacy(json!({"action": "open", "name": "P1"})).await.status, StatusCode::OK);

    // Release -> web write is accepted again and advances the revision ...
    let rel = s.call("DELETE", "/api/v1/projects/P1/lease", Some(&alice), &[("x-lease-id", la.json["lease_id"].as_str().unwrap().to_string())], Body::empty()).await;
    assert_eq!(rel.status, StatusCode::OK);
    let web = s.legacy(json!({"action": "save_meta", "name": "P1", "project": {"samples": [], "comment": "web"}, "overwrite": true})).await;
    assert_eq!(web.status, StatusCode::OK, "{}", web.json);
    // ... so a desktop editor still based on revision 1 gets a conflict instead of overwriting the web change.
    let lb = s.post("/api/v1/projects/P1/lease", Some(&bob), &[], json!({})).await;
    assert_eq!(lb.status, StatusCode::OK);
    assert!(lb.json["fencing_token"].as_i64().unwrap() > la.json["fencing_token"].as_i64().unwrap());
    let stale = s.post("/api/v1/projects/P1/idx", Some(&bob), &lease_headers(&lb.json, 1), save_meta("bob", false)).await;
    assert_eq!(stale.json["code"], "revision_conflict", "{}", stale.json);
    let fresh = s.post("/api/v1/projects/P1/idx", Some(&bob), &lease_headers(&lb.json, 2), save_meta("bob", false)).await;
    assert_eq!(fresh.status, StatusCode::OK, "{}", fresh.json);
}

#[tokio::test]
async fn admin_force_release_fences_the_old_holder() {
    let s = start();
    let alice = s.login("Casework Analyst").await;
    let bob = s.login("Databasing Analyst").await;
    let admin = s.login("Lab Director").await;
    let la = s.post("/api/v1/projects/P/lease", Some(&alice), &[], json!({})).await.json;
    assert_eq!(s.post("/api/v1/projects/P/lease/force-release", Some(&bob), &[], json!({"reason": "x"})).await.status, StatusCode::FORBIDDEN);
    assert_eq!(s.post("/api/v1/projects/P/lease/force-release", Some(&admin), &[], json!({})).await.status, StatusCode::BAD_REQUEST, "reason required");
    let f = s.post("/api/v1/projects/P/lease/force-release", Some(&admin), &[], json!({"reason": "workstation crashed"})).await;
    assert_eq!(f.json["released"]["user"], "Casework Analyst");
    let lb = s.post("/api/v1/projects/P/lease", Some(&bob), &[], json!({})).await.json;
    // Alice's delayed write (old lease, old token) must not land.
    let late = s.post("/api/v1/projects/P/idx", Some(&alice), &lease_headers(&la, 0), save_meta("late", true)).await;
    assert_eq!(late.status, StatusCode::CONFLICT);
    // Alice cannot renew a lease that is gone.
    let rn = s.post("/api/v1/projects/P/lease/renew", Some(&alice), &[], json!({"lease_id": la["lease_id"]})).await;
    assert_eq!(rn.status, StatusCode::CONFLICT);
    let ok = s.post("/api/v1/projects/P/idx", Some(&bob), &lease_headers(&lb, 0), save_meta("bob", true)).await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.json);
    let audit = s.get("/api/v1/audit", Some(&admin)).await;
    assert!(audit.json["audit"].as_array().unwrap().iter().any(|a| a["action"] == "lease.force_release" && a["detail"]["reason"] == "workstation crashed"));
}

#[tokio::test]
async fn fsa_upload_dedup_hash_check_and_project_link() {
    let s = start();
    let alice = s.login("Casework Analyst").await;
    let tech = s.login("Casework Tech").await;
    let fsa = opentrace_core::demo::demo_bytes();
    let sha = opentrace_server::db::sha256_hex(&fsa);
    let uri = format!("/api/v1/fsa/{}", sha);
    assert_eq!(s.call("HEAD", &uri, Some(&alice), &[], Body::empty()).await.status, StatusCode::NOT_FOUND);
    assert_eq!(s.call("PUT", &uri, Some(&tech), &[], Body::from(fsa.clone())).await.status, StatusCode::FORBIDDEN);
    let put = s.call("PUT", &uri, Some(&alice), &[], Body::from(fsa.clone())).await;
    assert_eq!(put.status, StatusCode::CREATED, "{}", put.json);
    let again = s.call("PUT", &uri, Some(&alice), &[], Body::from(fsa.clone())).await;
    assert_eq!(again.status, StatusCode::OK);
    assert_eq!(again.json["status"], "exists");
    assert_eq!(s.call("HEAD", &uri, Some(&tech), &[], Body::empty()).await.status, StatusCode::OK);
    // Exactly one physical copy.
    let n = walkdir_count(&s.st.cfg.fsa_dir());
    assert_eq!(n, 1);
    // Corrupted upload: bytes do not match the name.
    let mut bad = fsa.clone();
    bad[100] ^= 0xff;
    let r = s.call("PUT", &uri.replace(&sha[..4], "0000"), Some(&alice), &[], Body::from(bad)).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(walkdir_count(&s.st.cfg.fsa_dir()), 1);
    // Download is byte-identical.
    let resp = s.app.clone().oneshot(Request::get(&uri).header("authorization", format!("Bearer {}", tech)).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(resp.into_body().collect().await.unwrap().to_bytes().to_vec(), fsa);

    // Link to a project and save a sample by reference (no base64 over the wire).
    let l = s.post("/api/v1/projects/F/lease", Some(&alice), &[], json!({})).await.json;
    let m = s.post("/api/v1/projects/F/idx", Some(&alice), &lease_headers(&l, 0), save_meta("fsa", true)).await;
    assert_eq!(m.status, StatusCode::OK, "{}", m.json);
    let link = s.post("/api/v1/projects/F/fsa", Some(&alice), &lease_headers(&l, 1), json!({"sha256": sha, "filename": "demo.fsa"})).await;
    assert_eq!(link.status, StatusCode::OK, "{}", link.json);
    let ss = s
        .post("/api/v1/projects/F/idx", Some(&alice), &lease_headers(&l, 2), json!({"action": "save_sample", "id": 1, "fsa_sha256": sha, "data": {"file": "demo.fsa", "name": "demo"}}))
        .await;
    assert_eq!(ss.status, StatusCode::OK, "{}", ss.json);
    let files = s.get("/api/v1/projects/F/fsa", Some(&tech)).await;
    assert_eq!(files.json["files"][0]["filename"], "demo.fsa");
    let os = s.post("/api/v1/projects/F/idx", Some(&tech), &[], json!({"action": "open_sample", "id": 1})).await;
    assert_eq!(os.status, StatusCode::OK, "{}", os.json);
}

fn walkdir_count(dir: &std::path::Path) -> usize {
    let mut n = 0;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if e.file_type().unwrap().is_dir() {
                n += walkdir_count(&e.path());
            } else {
                n += 1;
            }
        }
    }
    n
}

#[tokio::test]
async fn standards_lifecycle_profile_and_pinned_provenance() {
    let s = start();
    let author = s.login("Practice User").await;
    let approver = s.login("Lab Director").await;
    let expert = s.login("Casework Analyst").await;
    let tech = s.login("Casework Tech").await;
    assert_eq!(s.post("/api/v1/standards/kit/K", Some(&tech), &[], json!({"content": {}})).await.status, StatusCode::FORBIDDEN);
    for (kind, name) in [("analysis_method", "HID"), ("kit", "GlobalFiler"), ("bin_set", "GF%20bins"), ("size_standard", "GS600")] {
        let d = s.post(&format!("/api/v1/standards/{}/{}", kind, name), Some(&author), &[], json!({"content": {"k": kind}})).await;
        assert_eq!(d.status, StatusCode::OK, "{}", d.json);
        assert_eq!(s.post(&format!("/api/v1/standards/{}/{}/1/submit", kind, name), Some(&author), &[], json!({})).await.status, StatusCode::OK);
        // Author cannot approve own work; experts lack the approver role.
        assert_eq!(s.post(&format!("/api/v1/standards/{}/{}/1/approve", kind, name), Some(&author), &[], json!({})).await.status, StatusCode::FORBIDDEN);
        let a = s.post(&format!("/api/v1/standards/{}/{}/1/approve", kind, name), Some(&approver), &[], json!({})).await;
        assert_eq!(a.json["status"], "approved", "{}", a.json);
    }
    let members = json!({"members": [
        {"kind": "analysis_method", "name": "HID", "version": 1}, {"kind": "kit", "name": "GlobalFiler", "version": 1},
        {"kind": "bin_set", "name": "GF bins", "version": 1}, {"kind": "size_standard", "name": "GS600", "version": 1}]});
    s.post("/api/v1/standards/lab_profile/Lab%20GF", Some(&author), &[], json!({"content": members})).await;
    s.post("/api/v1/standards/lab_profile/Lab%20GF/1/submit", Some(&author), &[], json!({})).await;
    let p = s.post("/api/v1/standards/lab_profile/Lab%20GF/1/approve", Some(&approver), &[], json!({})).await;
    assert_eq!(p.json["status"], "approved", "{}", p.json);
    let profiles = s.get("/api/v1/lab-profiles", Some(&tech)).await;
    assert_eq!(profiles.json["lab_profiles"][0]["name"], "Lab GF");
    // Approved content is immutable through the API too.
    let upd = s.call("PUT", "/api/v1/standards/kit/GlobalFiler/1", Some(&author), &[("content-type", "application/json".into())], Body::from(json!({"content": {"x": 1}}).to_string())).await;
    assert_eq!(upd.status, StatusCode::CONFLICT);

    // Analysis provenance pins the profile version + content hash and the exact settings.
    let fsa = opentrace_core::demo::demo_bytes();
    let sha = opentrace_server::db::sha256_hex(&fsa);
    s.call("PUT", &format!("/api/v1/fsa/{}", sha), Some(&expert), &[], Body::from(fsa)).await;
    let l = s.post("/api/v1/projects/A/lease", Some(&expert), &[], json!({})).await.json;
    let rec = s
        .post(
            "/api/v1/projects/A/analyses",
            Some(&expert),
            &lease_headers(&l, 0),
            json!({"sample_id": "1", "fsa_sha256": sha, "analysis_version": opentrace_core::server_analyze::analysis_version(),
                   "settings": {"threshold": 100, "distance": 8}, "lab_profile": {"name": "Lab GF", "version": 1}}),
        )
        .await;
    assert_eq!(rec.status, StatusCode::OK, "{}", rec.json);
    // A new profile version does not change the stored record.
    s.post("/api/v1/standards/lab_profile/Lab%20GF", Some(&author), &[], json!({"content": members})).await;
    let list = s.get("/api/v1/projects/A/analyses", Some(&tech)).await;
    let a0 = &list.json["analyses"][0];
    assert_eq!(a0["lab_profile_version"], 1);
    assert_eq!(a0["lab_profile_sha256"], p.json["content_sha256"]);
    assert_eq!(a0["settings"]["distance"], 8);
}

#[tokio::test]
async fn leases_survive_a_server_restart() {
    let dir = tempfile::tempdir().unwrap();
    let (la, alice_token) = {
        let s = start_in(dir.path());
        set_passwords(&s.st);
        let t = s.login("Casework Analyst").await;
        (s.post("/api/v1/projects/R/lease", Some(&t), &[], json!({})).await.json, t)
    };
    // Second process on the same directory is refused while the first runs; after "restart" state is intact.
    let s = start_in(dir.path());
    assert!(opentrace_server::build_state(cfg(dir.path()), PORT, true).is_err(), "single instance per data dir");
    let bob = s.login("Databasing Analyst").await;
    let lb = s.post("/api/v1/projects/R/lease", Some(&bob), &[], json!({})).await;
    assert_eq!(lb.json["code"], "project_locked");
    // Alice's session and lease are still valid after the restart.
    let w = s.post("/api/v1/projects/R/idx", Some(&alice_token), &lease_headers(&la, 0), save_meta("after restart", true)).await;
    assert_eq!(w.status, StatusCode::OK, "{}", w.json);
}

#[tokio::test]
async fn backup_verify_and_restore_roundtrip() {
    let s = start();
    let alice = s.login("Casework Analyst").await;
    let fsa = opentrace_core::demo::demo_bytes();
    let sha = opentrace_server::db::sha256_hex(&fsa);
    s.call("PUT", &format!("/api/v1/fsa/{}", sha), Some(&alice), &[], Body::from(fsa.clone())).await;
    let l = s.post("/api/v1/projects/B/lease", Some(&alice), &[], json!({})).await.json;
    s.post("/api/v1/projects/B/idx", Some(&alice), &lease_headers(&l, 0), save_meta("backed up", true)).await;
    let out = tempfile::tempdir().unwrap();
    let b = opentrace_server::backup::backup(&s.st.cfg, out.path()).unwrap();
    assert!(opentrace_server::backup::verify(&b).unwrap() >= 4);
    // Tampering is detected.
    let target = tempfile::tempdir().unwrap();
    let rcfg = cfg(target.path());
    let n = opentrace_server::backup::restore(&b, &rcfg).unwrap();
    assert!(n >= 4);
    assert!(opentrace_server::backup::restore(&b, &rcfg).is_err(), "refuses to overwrite");
    let r = start_in(target.path());
    let t = r.login("Casework Analyst").await;
    let p = r.get("/api/v1/projects/B", Some(&t)).await;
    assert_eq!(p.json["project"]["comment"], "backed up");
    assert_eq!(p.json["revision"], 1);
    let resp = r.app.clone().oneshot(Request::get(format!("/api/v1/fsa/{}", sha)).header("authorization", format!("Bearer {}", t)).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(resp.into_body().collect().await.unwrap().to_bytes().to_vec(), fsa);
    std::fs::write(b.join("opentrace.sqlite3"), b"tampered").unwrap();
    assert!(opentrace_server::backup::verify(&b).is_err());
}
