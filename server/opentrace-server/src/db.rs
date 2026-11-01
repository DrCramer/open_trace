//! SQLite database of the server (only the server process opens it; clients go through the API).
//!
//! Schema changes are append-only numbered migrations recorded in `schema_migrations`; each runs in its own
//! transaction. Never edit an applied migration — add a new one.
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

pub const MIGRATIONS: &[(i64, &str, &str)] = &[
    (
        1,
        "sessions, project state and leases, FSA store, analyses provenance, audit",
        r#"
CREATE TABLE sessions (
    token_sha256 TEXT PRIMARY KEY,
    session_id   TEXT NOT NULL UNIQUE,
    user_name    TEXT NOT NULL,
    client       TEXT,
    created_ms   INTEGER NOT NULL,
    expires_ms   INTEGER NOT NULL,
    revoked      INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX sessions_user ON sessions(user_name);

-- One row per project name of the project library. `revision` increases with every accepted mutation (API v1 or the
-- legacy web endpoint); `fencing_seq` is the last fencing token handed out and never decreases.
CREATE TABLE project_state (
    name        TEXT PRIMARY KEY,
    revision    INTEGER NOT NULL DEFAULT 0,
    fencing_seq INTEGER NOT NULL DEFAULT 0,
    updated_ms  INTEGER,
    updated_by  TEXT
);

CREATE TABLE project_leases (
    name          TEXT PRIMARY KEY REFERENCES project_state(name),
    lease_id      TEXT NOT NULL UNIQUE,
    user_name     TEXT NOT NULL,
    session_id    TEXT NOT NULL,
    client        TEXT,
    fencing_token INTEGER NOT NULL,
    acquired_ms   INTEGER NOT NULL,
    expires_ms    INTEGER NOT NULL
);

CREATE TABLE fsa_blobs (
    sha256    TEXT PRIMARY KEY,
    size      INTEGER NOT NULL,
    stored_ms INTEGER NOT NULL,
    stored_by TEXT NOT NULL
);

CREATE TABLE project_fsa (
    project  TEXT NOT NULL,
    sha256   TEXT NOT NULL REFERENCES fsa_blobs(sha256),
    filename TEXT NOT NULL,
    added_ms INTEGER NOT NULL,
    added_by TEXT NOT NULL,
    PRIMARY KEY (project, sha256, filename)
);

CREATE TABLE analyses (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    project             TEXT NOT NULL,
    sample_id           TEXT,
    fsa_sha256          TEXT NOT NULL,
    analysis_version    TEXT NOT NULL,
    settings_json       TEXT NOT NULL,
    settings_sha256     TEXT NOT NULL,
    lab_profile         TEXT,
    lab_profile_version INTEGER,
    lab_profile_sha256  TEXT,
    result_sha256       TEXT,
    project_revision    INTEGER NOT NULL,
    created_ms          INTEGER NOT NULL,
    created_by          TEXT NOT NULL
);
CREATE INDEX analyses_project ON analyses(project);

CREATE TABLE audit (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    at_ms       INTEGER NOT NULL,
    user_name   TEXT,
    action      TEXT NOT NULL,
    object_type TEXT NOT NULL,
    object_id   TEXT,
    detail      TEXT
);
"#,
    ),
    (
        2,
        "versioned laboratory standards and analysis profiles",
        r#"
CREATE TABLE standard_versions (
    kind           TEXT NOT NULL,
    name           TEXT NOT NULL,
    version        INTEGER NOT NULL,
    status         TEXT NOT NULL CHECK (status IN ('draft', 'pending_review', 'approved', 'deprecated', 'archived')),
    content        TEXT NOT NULL,
    content_sha256 TEXT NOT NULL,
    created_by     TEXT NOT NULL,
    created_ms     INTEGER NOT NULL,
    submitted_by   TEXT,
    submitted_ms   INTEGER,
    approved_by    TEXT,
    approved_ms    INTEGER,
    note           TEXT,
    PRIMARY KEY (kind, name, version)
);
-- Content of anything that left the draft state can never change (defence in depth below the API checks).
CREATE TRIGGER standard_versions_immutable
BEFORE UPDATE OF content, content_sha256, kind, name, version ON standard_versions
WHEN OLD.status <> 'draft'
BEGIN
    SELECT RAISE(ABORT, 'standard version is immutable once submitted');
END;
CREATE TRIGGER standard_versions_no_delete
BEFORE DELETE ON standard_versions
WHEN OLD.status <> 'draft'
BEGIN
    SELECT RAISE(ABORT, 'submitted standard versions cannot be deleted');
END;
"#,
    ),
];

pub fn open(path: &Path) -> Result<Connection, String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
    }
    let conn = Connection::open(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    conn.busy_timeout(std::time::Duration::from_secs(10)).map_err(|e| e.to_string())?;
    conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL; PRAGMA foreign_keys = ON;")
        .map_err(|e| e.to_string())?;
    Ok(conn)
}

pub fn schema_version(conn: &Connection) -> Result<i64, String> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, description TEXT NOT NULL, applied_ms INTEGER NOT NULL);")
        .map_err(|e| e.to_string())?;
    conn.query_row("SELECT COALESCE(MAX(version), 0) FROM schema_migrations", [], |r| r.get(0)).map_err(|e| e.to_string())
}

/// Applies pending migrations; returns the versions applied now.
pub fn migrate(conn: &mut Connection) -> Result<Vec<i64>, String> {
    let current = schema_version(conn)?;
    let newest = MIGRATIONS.last().map(|m| m.0).unwrap_or(0);
    if current > newest {
        return Err(format!(
            "Database schema version {} is newer than this server ({}); use the matching or a newer opentrace-server",
            current, newest
        ));
    }
    let mut applied = Vec::new();
    for (version, description, sql) in MIGRATIONS {
        if *version <= current {
            continue;
        }
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        tx.execute_batch(sql).map_err(|e| format!("migration {}: {}", version, e))?;
        tx.execute("INSERT INTO schema_migrations (version, description, applied_ms) VALUES (?1, ?2, ?3)", params![version, description, now_ms()])
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        applied.push(*version);
    }
    Ok(applied)
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

pub fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::getrandom(&mut buf).expect("OS randomness");
    hex(&buf)
}

pub fn hex(b: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push(H[(x >> 4) as usize] as char);
        s.push(H[(x & 15) as usize] as char);
    }
    s
}

pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    hex(&sha2::Sha256::digest(data))
}

pub fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn audit(conn: &Connection, user: Option<&str>, action: &str, object_type: &str, object_id: Option<&str>, detail: &serde_json::Value) -> Result<(), String> {
    conn.execute(
        "INSERT INTO audit (at_ms, user_name, action, object_type, object_id, detail) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![now_ms(), user, action, object_type, object_id, detail.to_string()],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

pub fn get_opt<T: rusqlite::types::FromSql>(conn: &Connection, sql: &str, p: impl rusqlite::Params) -> Result<Option<T>, String> {
    conn.query_row(sql, p, |r| r.get(0)).optional().map_err(|e| e.to_string())
}

pub fn get_row3<A, B, C>(conn: &Connection, sql: &str, p: impl rusqlite::Params) -> Result<Option<(A, B, C)>, String>
where
    A: rusqlite::types::FromSql,
    B: rusqlite::types::FromSql,
    C: rusqlite::types::FromSql,
{
    conn.query_row(sql, p, |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_once_and_record_version() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = open(&dir.path().join("t.sqlite3")).unwrap();
        assert_eq!(migrate(&mut c).unwrap(), vec![1, 2]);
        assert!(migrate(&mut c).unwrap().is_empty());
        assert_eq!(schema_version(&c).unwrap(), 2);
    }

    #[test]
    fn newer_schema_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = open(&dir.path().join("t.sqlite3")).unwrap();
        migrate(&mut c).unwrap();
        c.execute("INSERT INTO schema_migrations VALUES (99, 'future', 0)", []).unwrap();
        assert!(migrate(&mut c).unwrap_err().contains("newer"));
    }

    #[test]
    fn approved_content_is_immutable_in_sql() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = open(&dir.path().join("t.sqlite3")).unwrap();
        migrate(&mut c).unwrap();
        c.execute("INSERT INTO standard_versions (kind,name,version,status,content,content_sha256,created_by,created_ms) VALUES ('kit','K',1,'approved','{}','x','a',0)", []).unwrap();
        let e = c.execute("UPDATE standard_versions SET content = '{\"x\":1}' WHERE name = 'K'", []).unwrap_err();
        assert!(e.to_string().contains("immutable"));
        assert!(c.execute("DELETE FROM standard_versions WHERE name = 'K'", []).is_err());
        c.execute("UPDATE standard_versions SET status = 'deprecated' WHERE name = 'K'", []).unwrap();
    }
}
