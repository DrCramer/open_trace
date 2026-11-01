//! Laboratory standards: versioned, immutable once submitted, approved by a second person.
//!
//! Kinds: analysis methods, kits/panels, bin sets, stutter definitions, size standards, matrices, and the
//! `lab_profile` — a named set of exact (kind, name, version, content hash) references that an analysis run pins.
//! Lifecycle: draft -> pending_review -> approved (= published) -> deprecated -> archived. A new version never
//! changes results already stored: analyses keep the profile name, version and content hash they ran with.
use crate::db::{audit, now_ms, sha256_hex};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};

pub const KINDS: [&str; 7] = ["analysis_method", "kit", "bin_set", "stutter", "size_standard", "matrix", "lab_profile"];
pub const PROFILE_MEMBER_KINDS: [&str; 6] = ["analysis_method", "kit", "bin_set", "stutter", "size_standard", "matrix"];

#[derive(Debug, PartialEq, Eq)]
pub enum StdError {
    Invalid(String),
    NotFound,
    Conflict(String),
    Forbidden(String),
    Db(String),
}
impl From<rusqlite::Error> for StdError {
    fn from(e: rusqlite::Error) -> Self {
        StdError::Db(e.to_string())
    }
}
type SResult<T> = Result<T, StdError>;

/// Canonical JSON (object keys sorted recursively, no insignificant whitespace) — the hashed form.
pub fn canonical(v: &Value) -> String {
    fn sort(v: &Value) -> Value {
        match v {
            Value::Object(m) => {
                let mut keys: Vec<&String> = m.keys().collect();
                keys.sort();
                let mut out = serde_json::Map::new();
                for k in keys {
                    out.insert(k.clone(), sort(&m[k]));
                }
                Value::Object(out)
            }
            Value::Array(a) => Value::Array(a.iter().map(sort).collect()),
            other => other.clone(),
        }
    }
    serde_json::to_string(&sort(v)).expect("json")
}

pub fn content_hash(v: &Value) -> String {
    sha256_hex(canonical(v).as_bytes())
}

fn check_kind(kind: &str) -> SResult<()> {
    if KINDS.contains(&kind) {
        Ok(())
    } else {
        Err(StdError::Invalid(format!("kind must be one of {}", KINDS.join(", "))))
    }
}

fn check_name(name: &str) -> SResult<()> {
    let ok = !name.trim().is_empty() && name.chars().count() <= 120 && !name.chars().any(|c| c.is_control());
    if ok {
        Ok(())
    } else {
        Err(StdError::Invalid("name: 1..120 characters, no control characters".into()))
    }
}

fn row_json(r: &rusqlite::Row) -> rusqlite::Result<Value> {
    let content: String = r.get(4)?;
    Ok(json!({
        "kind": r.get::<_, String>(0)?, "name": r.get::<_, String>(1)?, "version": r.get::<_, i64>(2)?,
        "status": r.get::<_, String>(3)?, "content": serde_json::from_str::<Value>(&content).unwrap_or(Value::Null),
        "content_sha256": r.get::<_, String>(5)?, "created_by": r.get::<_, String>(6)?, "created_ms": r.get::<_, i64>(7)?,
        "submitted_by": r.get::<_, Option<String>>(8)?, "approved_by": r.get::<_, Option<String>>(9)?,
        "approved_ms": r.get::<_, Option<i64>>(10)?, "note": r.get::<_, Option<String>>(11)?,
    }))
}

const COLS: &str = "kind, name, version, status, content, content_sha256, created_by, created_ms, submitted_by, approved_by, approved_ms, note";

pub fn get(conn: &Connection, kind: &str, name: &str, version: i64) -> SResult<Value> {
    conn.query_row(&format!("SELECT {} FROM standard_versions WHERE kind = ?1 AND name = ?2 AND version = ?3", COLS), params![kind, name, version], row_json)
        .optional()?
        .ok_or(StdError::NotFound)
}

/// Newest version of every (kind, name), optionally filtered by kind and status.
pub fn list(conn: &Connection, kind: Option<&str>, status: Option<&str>) -> SResult<Vec<Value>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {} FROM standard_versions WHERE (?1 IS NULL OR kind = ?1) AND (?2 IS NULL OR status = ?2) ORDER BY kind, name, version",
        COLS
    ))?;
    let rows = stmt.query_map(params![kind, status], row_json)?;
    let mut out = Vec::new();
    for r in rows {
        let mut v = r?;
        if let Some(m) = v.as_object_mut() {
            m.remove("content");
        }
        out.push(v);
    }
    Ok(out)
}

/// New draft version (content may be edited while it is a draft; see `update_draft`).
pub fn create_draft(conn: &mut Connection, kind: &str, name: &str, content: &Value, user: &str, note: Option<&str>) -> SResult<Value> {
    check_kind(kind)?;
    check_name(name)?;
    if !content.is_object() {
        return Err(StdError::Invalid("content must be a JSON object".into()));
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if kind == "lab_profile" {
        validate_profile(&tx, content, false)?;
    }
    let next: i64 = tx.query_row("SELECT COALESCE(MAX(version), 0) + 1 FROM standard_versions WHERE kind = ?1 AND name = ?2", params![kind, name], |r| r.get(0))?;
    tx.execute(
        "INSERT INTO standard_versions (kind, name, version, status, content, content_sha256, created_by, created_ms, note)
         VALUES (?1, ?2, ?3, 'draft', ?4, ?5, ?6, ?7, ?8)",
        params![kind, name, next, canonical(content), content_hash(content), user, now_ms(), note],
    )?;
    audit(&tx, Some(user), "standard.draft", kind, Some(&format!("{}@{}", name, next)), &json!({"content_sha256": content_hash(content)})).map_err(StdError::Db)?;
    let v = get(&tx, kind, name, next)?;
    tx.commit()?;
    Ok(v)
}

pub fn update_draft(conn: &mut Connection, kind: &str, name: &str, version: i64, content: &Value, user: &str) -> SResult<Value> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let cur = get(&tx, kind, name, version)?;
    if cur["status"] != "draft" {
        return Err(StdError::Conflict(format!("{} {} v{} is {}; create a new version instead", kind, name, version, cur["status"].as_str().unwrap_or(""))));
    }
    if cur["created_by"] != user {
        return Err(StdError::Forbidden("only the author edits a draft".into()));
    }
    if kind == "lab_profile" {
        validate_profile(&tx, content, false)?;
    }
    tx.execute(
        "UPDATE standard_versions SET content = ?1, content_sha256 = ?2 WHERE kind = ?3 AND name = ?4 AND version = ?5",
        params![canonical(content), content_hash(content), kind, name, version],
    )?;
    let v = get(&tx, kind, name, version)?;
    tx.commit()?;
    Ok(v)
}

/// `members`: [{kind, name, version}] of approved standards (one per kind at most); hashes are pinned on submit.
fn validate_profile(conn: &Connection, content: &Value, require_approved: bool) -> SResult<Value> {
    let members = content.get("members").and_then(|m| m.as_array()).ok_or(StdError::Invalid("lab_profile content needs members: [...]".into()))?;
    let mut seen = std::collections::BTreeSet::new();
    let mut pinned = Vec::new();
    for m in members {
        let kind = m.get("kind").and_then(|k| k.as_str()).unwrap_or("");
        let name = m.get("name").and_then(|k| k.as_str()).unwrap_or("");
        let version = m.get("version").and_then(|k| k.as_i64()).ok_or(StdError::Invalid("member version must be an integer".into()))?;
        if !PROFILE_MEMBER_KINDS.contains(&kind) {
            return Err(StdError::Invalid(format!("member kind must be one of {}", PROFILE_MEMBER_KINDS.join(", "))));
        }
        if !seen.insert(kind.to_string()) {
            return Err(StdError::Invalid(format!("more than one {} in the profile", kind)));
        }
        let v = get(conn, kind, name, version).map_err(|e| match e {
            StdError::NotFound => StdError::Invalid(format!("member {} {} v{} does not exist", kind, name, version)),
            other => other,
        })?;
        if require_approved && v["status"] != "approved" {
            return Err(StdError::Conflict(format!("member {} {} v{} is {}, not approved", kind, name, version, v["status"].as_str().unwrap_or(""))));
        }
        pinned.push(json!({"kind": kind, "name": name, "version": version, "content_sha256": v["content_sha256"]}));
    }
    for required in ["analysis_method", "kit", "bin_set", "size_standard"] {
        if !seen.contains(required) && require_approved {
            return Err(StdError::Invalid(format!("a lab profile needs a {}", required)));
        }
    }
    Ok(Value::Array(pinned))
}

pub fn submit(conn: &mut Connection, kind: &str, name: &str, version: i64, user: &str) -> SResult<Value> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let cur = get(&tx, kind, name, version)?;
    if cur["status"] != "draft" {
        return Err(StdError::Conflict(format!("only drafts can be submitted (status {})", cur["status"].as_str().unwrap_or(""))));
    }
    if kind == "lab_profile" {
        // Pin member hashes into the content before it becomes immutable.
        let mut content = cur["content"].clone();
        let pinned = validate_profile(&tx, &content, true)?;
        content["members"] = pinned;
        tx.execute(
            "UPDATE standard_versions SET content = ?1, content_sha256 = ?2 WHERE kind = ?3 AND name = ?4 AND version = ?5",
            params![canonical(&content), content_hash(&content), kind, name, version],
        )?;
    }
    tx.execute(
        "UPDATE standard_versions SET status = 'pending_review', submitted_by = ?1, submitted_ms = ?2 WHERE kind = ?3 AND name = ?4 AND version = ?5",
        params![user, now_ms(), kind, name, version],
    )?;
    audit(&tx, Some(user), "standard.submit", kind, Some(&format!("{}@{}", name, version)), &json!({})).map_err(StdError::Db)?;
    let v = get(&tx, kind, name, version)?;
    tx.commit()?;
    Ok(v)
}

/// Approval by somebody other than the author (four-eyes rule). Publishing == approved.
pub fn approve(conn: &mut Connection, kind: &str, name: &str, version: i64, approver: &str) -> SResult<Value> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let cur = get(&tx, kind, name, version)?;
    if cur["status"] != "pending_review" {
        return Err(StdError::Conflict(format!("only pending_review versions can be approved (status {})", cur["status"].as_str().unwrap_or(""))));
    }
    if cur["created_by"] == approver || cur["submitted_by"] == approver {
        return Err(StdError::Forbidden("the author or submitter cannot approve their own version".into()));
    }
    if kind == "lab_profile" {
        validate_profile(&tx, &cur["content"], true)?;
        // Pinned hashes must still match the referenced versions (they are immutable, so this is a consistency check).
        for m in cur["content"]["members"].as_array().cloned().unwrap_or_default() {
            let v = get(&tx, m["kind"].as_str().unwrap_or(""), m["name"].as_str().unwrap_or(""), m["version"].as_i64().unwrap_or(0))?;
            if v["content_sha256"] != m["content_sha256"] {
                return Err(StdError::Conflict("member content hash changed".into()));
            }
        }
    }
    tx.execute(
        "UPDATE standard_versions SET status = 'approved', approved_by = ?1, approved_ms = ?2 WHERE kind = ?3 AND name = ?4 AND version = ?5",
        params![approver, now_ms(), kind, name, version],
    )?;
    audit(&tx, Some(approver), "standard.approve", kind, Some(&format!("{}@{}", name, version)), &json!({"content_sha256": cur["content_sha256"]})).map_err(StdError::Db)?;
    let v = get(&tx, kind, name, version)?;
    tx.commit()?;
    Ok(v)
}

pub fn reject(conn: &mut Connection, kind: &str, name: &str, version: i64, user: &str, reason: &str) -> SResult<Value> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let cur = get(&tx, kind, name, version)?;
    if cur["status"] != "pending_review" {
        return Err(StdError::Conflict("only pending_review versions can be rejected".into()));
    }
    // Rejected submissions are archived (immutable record of what was reviewed); the author drafts a new version.
    tx.execute("UPDATE standard_versions SET status = 'archived', note = ?1 WHERE kind = ?2 AND name = ?3 AND version = ?4", params![reason, kind, name, version])?;
    audit(&tx, Some(user), "standard.reject", kind, Some(&format!("{}@{}", name, version)), &json!({"reason": reason})).map_err(StdError::Db)?;
    let v = get(&tx, kind, name, version)?;
    tx.commit()?;
    Ok(v)
}

pub fn set_retired(conn: &mut Connection, kind: &str, name: &str, version: i64, user: &str, status: &str) -> SResult<Value> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let cur = get(&tx, kind, name, version)?;
    let ok = matches!((cur["status"].as_str().unwrap_or(""), status), ("approved", "deprecated") | ("deprecated", "archived") | ("approved", "archived"));
    if !ok {
        return Err(StdError::Conflict(format!("cannot go from {} to {}", cur["status"].as_str().unwrap_or(""), status)));
    }
    tx.execute("UPDATE standard_versions SET status = ?1 WHERE kind = ?2 AND name = ?3 AND version = ?4", params![status, kind, name, version])?;
    audit(&tx, Some(user), &format!("standard.{}", status), kind, Some(&format!("{}@{}", name, version)), &json!({})).map_err(StdError::Db)?;
    let v = get(&tx, kind, name, version)?;
    tx.commit()?;
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn conn() -> (tempfile::TempDir, Connection) {
        let d = tempfile::tempdir().unwrap();
        let mut c = db::open(&d.path().join("s.sqlite3")).unwrap();
        db::migrate(&mut c).unwrap();
        (d, c)
    }

    fn approved(c: &mut Connection, kind: &str, name: &str, content: Value) -> i64 {
        let v = create_draft(c, kind, name, &content, "author", None).unwrap();
        let ver = v["version"].as_i64().unwrap();
        submit(c, kind, name, ver, "author").unwrap();
        approve(c, kind, name, ver, "approver").unwrap();
        ver
    }

    #[test]
    fn lifecycle_four_eyes_and_immutability() {
        let (_d, mut c) = conn();
        let v = create_draft(&mut c, "analysis_method", "HID", &json!({"threshold": 50}), "author", None).unwrap();
        assert_eq!(v["status"], "draft");
        update_draft(&mut c, "analysis_method", "HID", 1, &json!({"threshold": 60}), "author").unwrap();
        submit(&mut c, "analysis_method", "HID", 1, "author").unwrap();
        assert!(matches!(update_draft(&mut c, "analysis_method", "HID", 1, &json!({"threshold": 70}), "author"), Err(StdError::Conflict(_))));
        assert!(matches!(approve(&mut c, "analysis_method", "HID", 1, "author"), Err(StdError::Forbidden(_))));
        let a = approve(&mut c, "analysis_method", "HID", 1, "approver").unwrap();
        assert_eq!(a["status"], "approved");
        assert_eq!(a["content"]["threshold"], 60);
        assert_eq!(a["content_sha256"], content_hash(&json!({"threshold": 60})));
        // New version does not touch v1.
        let v2 = create_draft(&mut c, "analysis_method", "HID", &json!({"threshold": 80}), "author", None).unwrap();
        assert_eq!(v2["version"], 2);
        assert_eq!(get(&c, "analysis_method", "HID", 1).unwrap()["content"]["threshold"], 60);
        set_retired(&mut c, "analysis_method", "HID", 1, "admin", "deprecated").unwrap();
        assert!(matches!(set_retired(&mut c, "analysis_method", "HID", 2, "admin", "deprecated"), Err(StdError::Conflict(_))));
    }

    #[test]
    fn profile_pins_approved_member_hashes() {
        let (_d, mut c) = conn();
        approved(&mut c, "analysis_method", "M", json!({"a": 1}));
        approved(&mut c, "kit", "K", json!({"markers": []}));
        approved(&mut c, "bin_set", "B", json!({"bins": []}));
        create_draft(&mut c, "size_standard", "S", &json!({"sizes": [75, 100]}), "author", None).unwrap();
        let members = json!({"members": [
            {"kind": "analysis_method", "name": "M", "version": 1}, {"kind": "kit", "name": "K", "version": 1},
            {"kind": "bin_set", "name": "B", "version": 1}, {"kind": "size_standard", "name": "S", "version": 1}]});
        create_draft(&mut c, "lab_profile", "Lab HID", &members, "author", None).unwrap();
        // Size standard still a draft -> cannot submit the profile.
        assert!(matches!(submit(&mut c, "lab_profile", "Lab HID", 1, "author"), Err(StdError::Conflict(_))));
        submit(&mut c, "size_standard", "S", 1, "author").unwrap();
        approve(&mut c, "size_standard", "S", 1, "approver").unwrap();
        let p = submit(&mut c, "lab_profile", "Lab HID", 1, "author").unwrap();
        assert_eq!(p["content"]["members"][3]["content_sha256"], content_hash(&json!({"sizes": [75, 100]})));
        let p = approve(&mut c, "lab_profile", "Lab HID", 1, "approver").unwrap();
        assert_eq!(p["status"], "approved");
        let dup = json!({"members": [{"kind": "kit", "name": "K", "version": 1}, {"kind": "kit", "name": "K", "version": 1}]});
        assert!(matches!(create_draft(&mut c, "lab_profile", "Bad", &dup, "author", None), Err(StdError::Invalid(_))));
    }

    #[test]
    fn canonical_hash_ignores_key_order() {
        assert_eq!(content_hash(&json!({"b": 1, "a": {"y": 2, "x": 3}})), content_hash(&json!({"a": {"x": 3, "y": 2}, "b": 1})));
    }
}
