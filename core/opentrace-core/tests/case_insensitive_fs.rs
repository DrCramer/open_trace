//! Sample ids that differ only by letter case (W7, docs/desktop/BASELINE_AUDIT.md): kept apart on Linux, refused
//! on case-insensitive file systems instead of one sample silently overwriting the other.
use opentrace_core::idx_projects::ProjectLibrary;
use serde_json::json;

#[test]
fn case_only_sample_ids_never_overwrite_each_other() {
    let dir = std::env::temp_dir().join(format!("ot-case-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let lib = ProjectLibrary::new(Some(&dir));
    lib.request(&json!({"action": "save_meta", "name": "P", "project": {"samples": []}, "create": true}), None, None).unwrap();
    let a = opentrace_core::matrix::b64encode(b"ABIF-first-sample");
    let b = opentrace_core::matrix::b64encode(b"ABIF-second-sample");
    lib.request(&json!({"action": "save_sample", "name": "P", "id": "S1", "fsa_b64": a, "data": {"v": 1}}), None, None).unwrap();
    let second = lib.request(&json!({"action": "save_sample", "name": "P", "id": "s1", "fsa_b64": b, "data": {"v": 2}}), None, None);
    let first = lib.request(&json!({"action": "open_sample", "name": "P", "id": "S1"}), None, None).unwrap();
    if cfg!(any(windows, target_os = "macos")) {
        let e = second.unwrap_err();
        assert!(format!("{:?}", e).contains("collides"), "{:?}", e);
    } else {
        second.unwrap();
        let other = lib.request(&json!({"action": "open_sample", "name": "P", "id": "s1"}), None, None).unwrap();
        assert_eq!(other["data"]["v"], 2);
    }
    assert_eq!(first["fsa_b64"], json!(a), "the first sample is intact");
    assert_eq!(first["data"]["v"], 1);
    let _ = std::fs::remove_dir_all(&dir);
}
