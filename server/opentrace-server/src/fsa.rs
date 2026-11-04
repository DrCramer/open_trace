//! Content-addressed store of original FSA files: `<data>/fsa/ab/cd/<sha256>.fsa`, written once, never modified.
//!
//! Uploads stream to a temporary file while hashing; the file is moved into place only when the SHA-256 of the
//! received bytes equals the name the client asked for. A repeated upload of the same content is idempotent and
//! does not create a second copy.
use crate::db::is_sha256_hex;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

pub fn blob_path(root: &Path, sha: &str) -> Option<PathBuf> {
    if !is_sha256_hex(sha) {
        return None;
    }
    Some(root.join(&sha[0..2]).join(&sha[2..4]).join(format!("{}.fsa", sha)))
}

/// Replaces `copy` (a file with the same bytes as `blob`) by a hard link to `blob`: stored once on disk. The link is
/// made under a temporary name and renamed over the copy, so the copy is never missing. Nothing changes when the
/// sizes differ or the file system has no hard links (then the copy simply stays).
pub fn link_copy(blob: &Path, copy: &Path) -> std::io::Result<bool> {
    let (b, c) = (std::fs::metadata(blob)?, std::fs::metadata(copy)?);
    if b.len() != c.len() {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if b.ino() == c.ino() && b.dev() == c.dev() {
            return Ok(true);
        }
    }
    if std::fs::read(blob)? != std::fs::read(copy)? {
        return Ok(false);
    }
    let tmp = copy.with_extension(format!("fsa.link{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    std::fs::hard_link(blob, &tmp)?;
    if let Err(e) = std::fs::rename(&tmp, copy) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(true)
}

#[derive(Debug)]
pub enum PutError {
    BadName,
    TooLarge(u64),
    HashMismatch { expected: String, actual: String },
    Empty,
    NotAbif,
    Io(String),
    Client(String),
}

#[derive(Debug, PartialEq, Eq)]
pub enum PutOutcome {
    Created(u64),
    Exists(u64),
}

/// Streams `body` into the store under `sha`. The temporary file lives in `tmp_dir` (same volume as `root`, so the
/// final rename is atomic) and is removed on every failure path.
pub async fn put_stream<S, E>(root: &Path, tmp_dir: &Path, sha: &str, max_bytes: u64, mut body: S) -> Result<PutOutcome, PutError>
where
    S: futures_util::Stream<Item = Result<bytes::Bytes, E>> + Unpin,
    E: std::fmt::Display,
{
    let dest = blob_path(root, sha).ok_or(PutError::BadName)?;
    if let Ok(m) = tokio::fs::metadata(&dest).await {
        return Ok(PutOutcome::Exists(m.len()));
    }
    tokio::fs::create_dir_all(tmp_dir).await.map_err(|e| PutError::Io(e.to_string()))?;
    let tmp = tmp_dir.join(format!("upload-{}-{}.part", sha, crate::db::random_token(6)));
    let result = async {
        let mut file = tokio::fs::File::create(&tmp).await.map_err(|e| PutError::Io(e.to_string()))?;
        let mut hasher = Sha256::new();
        let mut size: u64 = 0;
        let mut head: Vec<u8> = Vec::with_capacity(4);
        while let Some(chunk) = body.next().await {
            let chunk = chunk.map_err(|e| PutError::Client(e.to_string()))?;
            size += chunk.len() as u64;
            if size > max_bytes {
                return Err(PutError::TooLarge(max_bytes));
            }
            if head.len() < 4 {
                head.extend(chunk.iter().take(4 - head.len()));
            }
            hasher.update(&chunk);
            file.write_all(&chunk).await.map_err(|e| PutError::Io(e.to_string()))?;
        }
        if size == 0 {
            return Err(PutError::Empty);
        }
        if head != b"ABIF" {
            return Err(PutError::NotAbif);
        }
        file.flush().await.map_err(|e| PutError::Io(e.to_string()))?;
        file.sync_all().await.map_err(|e| PutError::Io(e.to_string()))?;
        drop(file);
        let actual = crate::db::hex(&hasher.finalize());
        if actual != sha {
            return Err(PutError::HashMismatch { expected: sha.to_string(), actual });
        }
        let parent = dest.parent().expect("blob path has a parent");
        tokio::fs::create_dir_all(parent).await.map_err(|e| PutError::Io(e.to_string()))?;
        if tokio::fs::metadata(&dest).await.is_ok() {
            // A concurrent upload of the same content finished first.
            return Ok(PutOutcome::Exists(size));
        }
        tokio::fs::rename(&tmp, &dest).await.map_err(|e| PutError::Io(e.to_string()))?;
        Ok(PutOutcome::Created(size))
    }
    .await;
    if !matches!(result, Ok(PutOutcome::Created(_))) {
        let _ = tokio::fs::remove_file(&tmp).await;
    }
    result
}

/// Re-hashes a stored blob (backup verification, `verify` command).
pub fn verify_blob(root: &Path, sha: &str) -> Result<bool, String> {
    let p = blob_path(root, sha).ok_or("bad sha256")?;
    let mut f = std::fs::File::open(&p).map_err(|e| format!("{}: {}", p.display(), e))?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h).map_err(|e| e.to_string())?;
    Ok(crate::db::hex(&h.finalize()) == sha)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(parts: Vec<&'static [u8]>) -> impl futures_util::Stream<Item = Result<bytes::Bytes, String>> + Unpin {
        futures_util::stream::iter(parts.into_iter().map(|p| Ok(bytes::Bytes::from_static(p))))
    }

    #[tokio::test]
    async fn stores_once_and_verifies_hash() {
        let d = tempfile::tempdir().unwrap();
        let (root, tmp) = (d.path().join("fsa"), d.path().join("tmp"));
        let data: &'static [u8] = b"ABIF....synthetic";
        let sha = crate::db::sha256_hex(data);
        assert_eq!(put_stream(&root, &tmp, &sha, 1024, stream(vec![&data[..6], &data[6..]])).await.unwrap(), PutOutcome::Created(data.len() as u64));
        assert_eq!(put_stream(&root, &tmp, &sha, 1024, stream(vec![data])).await.unwrap(), PutOutcome::Exists(data.len() as u64));
        assert!(verify_blob(&root, &sha).unwrap());
        assert_eq!(std::fs::read_dir(&tmp).unwrap().count(), 0, "no temp files left");
    }

    #[tokio::test]
    async fn rejects_mismatch_oversize_and_non_abif_without_leftovers() {
        let d = tempfile::tempdir().unwrap();
        let (root, tmp) = (d.path().join("fsa"), d.path().join("tmp"));
        let wrong = crate::db::sha256_hex(b"something else");
        assert!(matches!(put_stream(&root, &tmp, &wrong, 1024, stream(vec![b"ABIF1234"])).await, Err(PutError::HashMismatch { .. })));
        let sha = crate::db::sha256_hex(b"ABIF12345678");
        assert!(matches!(put_stream(&root, &tmp, &sha, 4, stream(vec![b"ABIF12345678"])).await, Err(PutError::TooLarge(4))));
        let sha = crate::db::sha256_hex(b"<html>");
        assert!(matches!(put_stream(&root, &tmp, &sha, 1024, stream(vec![b"<html>"])).await, Err(PutError::NotAbif)));
        assert!(matches!(put_stream(&root, &tmp, "../../etc/passwd", 1024, stream(vec![b"x"])).await, Err(PutError::BadName)));
        assert_eq!(std::fs::read_dir(&tmp).map(|r| r.count()).unwrap_or(0), 0);
        assert!(!root.exists() || std::fs::read_dir(&root).unwrap().count() == 0);
    }
}
