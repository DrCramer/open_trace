//! Desktop user-interface layout (schema `desktop_ui_layout_v1`): visibility and order of menus, menu items, toolbar
//! buttons, workspace tabs and panels, theme, plot background, density. It is a per-user, per-computer preference stored
//! next to `desktop.json` (`ui_layout.json`, written atomically). It never contains analysis methods, panels,
//! standards, project data, credentials or tokens, and it is not sent to the organization server.
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: u64 = 1;
const MAX_BYTES: usize = 512 * 1024;

pub fn path_for(cfg_path: &Path) -> PathBuf {
    cfg_path.with_file_name("ui_layout.json")
}

/// Shape check of a layout sent by the page (the page validates field values itself; this keeps garbage, oversized or
/// future-schema files out). Only known top-level keys are kept.
pub fn check(v: &Value) -> Result<Value, String> {
    let o = v.as_object().ok_or("layout is not an object")?;
    let ver = o.get("schema_version").and_then(Value::as_u64).ok_or("schema_version is missing")?;
    if ver == 0 || ver > SCHEMA_VERSION {
        return Err(format!("unsupported schema_version {ver}"));
    }
    let mut out = serde_json::Map::new();
    for k in ["schema_version", "chrome", "theme", "plot_surface", "density", "labels", "active_preset", "hidden", "order", "custom", "saved_at"] {
        if let Some(x) = o.get(k) {
            out.insert(k.to_string(), x.clone());
        }
    }
    for k in ["hidden", "order"] {
        if let Some(x) = out.get(k) {
            if !x.is_object() {
                return Err(format!("{k} is not an object"));
            }
        }
    }
    let v = Value::Object(out);
    if serde_json::to_vec(&v).map(|b| b.len()).unwrap_or(usize::MAX) > MAX_BYTES {
        return Err("layout is too large".into());
    }
    Ok(v)
}

/// Stored layout, or None (no file, unreadable or wrong schema: the page then uses its factory layout and says so).
pub fn load(path: &Path) -> Option<Value> {
    let b = std::fs::read(path).ok()?;
    let v: Value = serde_json::from_slice(&b).ok()?;
    check(&v).ok()
}

pub fn save(path: &Path, v: &Value) -> Result<(), String> {
    let v = check(v)?;
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&v).unwrap()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn roundtrip_keeps_known_keys_only() {
        let d = std::env::temp_dir().join(format!("ot-uilayout-{}", std::process::id()));
        let p = path_for(&d.join("desktop.json"));
        let l = json!({"schema_version": 1, "theme": "dark", "hidden": {"main.toolbar": ["help"]}, "order": {}, "token": "x"});
        save(&p, &l).unwrap();
        let back = load(&p).unwrap();
        assert_eq!(back["theme"], "dark");
        assert_eq!(back["hidden"]["main.toolbar"][0], "help");
        assert!(back.get("token").is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn rejects_wrong_schema_and_shapes() {
        assert!(check(&json!({"theme": "dark"})).is_err());
        assert!(check(&json!({"schema_version": 2})).is_err());
        assert!(check(&json!({"schema_version": 1, "hidden": []})).is_err());
        assert!(check(&json!([1, 2])).is_err());
        assert!(check(&json!({"schema_version": 1, "hidden": {"a": ["x".repeat(600_000)]}})).is_err());
    }

    #[test]
    fn unreadable_file_gives_none() {
        let d = std::env::temp_dir().join(format!("ot-uilayout-bad-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("ui_layout.json");
        std::fs::write(&p, b"{not json").unwrap();
        assert!(load(&p).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}
