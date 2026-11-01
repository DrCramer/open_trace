//! Backup / restore of a server data directory: consistent SQLite snapshot (online backup API, safe while the server
//! runs), project library, security store (users, audit trail) and the original FSA files, with a manifest of SHA-256
//! hashes that `verify` and `restore` check before anything is written.
use crate::config::ServerConfig;
use crate::db::{hex, now_ms};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const MANIFEST: &str = "manifest.json";
const FORMAT: &str = "opentrace-server-backup/1";

fn file_sha(p: &Path) -> Result<(String, u64), String> {
    let mut f = std::fs::File::open(p).map_err(|e| format!("{}: {}", p.display(), e))?;
    let mut h = Sha256::new();
    let n = std::io::copy(&mut f, &mut h).map_err(|e| format!("{}: {}", p.display(), e))?;
    Ok((hex(&h.finalize()), n))
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let rd = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("{}: {}", dir.display(), e)),
    };
    for e in rd {
        let e = e.map_err(|e| e.to_string())?;
        let p = e.path();
        let ft = e.file_type().map_err(|e| e.to_string())?;
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            walk(root, &p, out)?;
        } else {
            out.push(p.strip_prefix(root).map_err(|e| e.to_string())?.to_path_buf());
        }
    }
    Ok(())
}

fn rel_str(p: &Path) -> String {
    p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

/// Creates `<out_dir>/opentrace-backup-<timestamp>/` and returns its path.
pub fn backup(cfg: &ServerConfig, out_dir: &Path) -> Result<PathBuf, String> {
    let stamp = opentrace_core::localtime::now().iso().replace([':', '-'], "");
    let dest = out_dir.join(format!("opentrace-backup-{}", stamp));
    if dest.exists() {
        return Err(format!("{} already exists", dest.display()));
    }
    std::fs::create_dir_all(&dest).map_err(|e| format!("{}: {}", dest.display(), e))?;
    let mut files: Vec<Value> = Vec::new();
    // 1. Database snapshot.
    let src = crate::db::open(&cfg.db_path())?;
    let schema = crate::db::schema_version(&src)?;
    let db_dest = dest.join("opentrace.sqlite3");
    src.backup(rusqlite::MAIN_DB, &db_dest, None::<fn(rusqlite::backup::Progress)>).map_err(|e| format!("sqlite backup: {}", e))?;
    let (h, n) = file_sha(&db_dest)?;
    files.push(json!({"path": "opentrace.sqlite3", "sha256": h, "size": n}));
    // 2. Directories (FSA blobs are immutable; projects/security are rewritten atomically by the server).
    for sub in ["projects", "security", "fsa"] {
        let root = cfg.data_dir.join(sub);
        let mut rels = Vec::new();
        walk(&root, &root, &mut rels)?;
        rels.sort();
        for rel in rels {
            let name = rel.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            if name.contains(".tmp") || name.ends_with(".part") || name == "server.lock" {
                continue;
            }
            let from = root.join(&rel);
            let to = dest.join(sub).join(&rel);
            std::fs::create_dir_all(to.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::copy(&from, &to).map_err(|e| format!("{}: {}", from.display(), e))?;
            let (h, n) = file_sha(&to)?;
            files.push(json!({"path": format!("{}/{}", sub, rel_str(&rel)), "sha256": h, "size": n}));
        }
    }
    let manifest = json!({"format": FORMAT, "created_ms": now_ms(), "server_version": env!("CARGO_PKG_VERSION"), "schema_version": schema, "files": files});
    std::fs::write(dest.join(MANIFEST), serde_json::to_vec_pretty(&manifest).unwrap()).map_err(|e| e.to_string())?;
    Ok(dest)
}

/// Checks every file of a backup against its manifest. Returns the number of files.
pub fn verify(backup_dir: &Path) -> Result<usize, String> {
    let m: Value = serde_json::from_slice(&std::fs::read(backup_dir.join(MANIFEST)).map_err(|e| format!("{}: {}", backup_dir.join(MANIFEST).display(), e))?)
        .map_err(|e| format!("manifest: {}", e))?;
    if m["format"] != FORMAT {
        return Err("not an opentrace-server backup".into());
    }
    let files = m["files"].as_array().ok_or("manifest without files")?;
    for f in files {
        let rel = f["path"].as_str().ok_or("bad manifest entry")?;
        if rel.split('/').any(|c| c == ".." || c.is_empty()) || rel.contains('\\') || rel.contains(':') {
            return Err(format!("unsafe path in manifest: {}", rel));
        }
        let (h, n) = file_sha(&backup_dir.join(rel))?;
        if h != f["sha256"].as_str().unwrap_or("") || n != f["size"].as_u64().unwrap_or(u64::MAX) {
            return Err(format!("{}: content does not match the manifest", rel));
        }
    }
    Ok(files.len())
}

/// Restores into an empty data directory (refuses to overwrite an existing installation).
pub fn restore(backup_dir: &Path, cfg: &ServerConfig) -> Result<usize, String> {
    let n = verify(backup_dir)?;
    if cfg.db_path().exists() {
        return Err(format!("{} already has a database; restore only into an empty data directory", cfg.data_dir.display()));
    }
    let m: Value = serde_json::from_slice(&std::fs::read(backup_dir.join(MANIFEST)).unwrap()).unwrap();
    for f in m["files"].as_array().unwrap() {
        let rel = f["path"].as_str().unwrap();
        let to = cfg.data_dir.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        std::fs::create_dir_all(to.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::copy(backup_dir.join(rel), &to).map_err(|e| format!("{}: {}", to.display(), e))?;
    }
    let mut c = crate::db::open(&cfg.db_path())?;
    crate::db::migrate(&mut c)?;
    Ok(n)
}
