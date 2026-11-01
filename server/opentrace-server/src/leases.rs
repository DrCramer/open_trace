//! Single-editor project locks: leases with heartbeat, expiry and monotonic fencing tokens, persisted in SQLite.
//!
//! Invariants
//! * At most one unexpired lease per project (primary key on `project_leases.name`; every change runs in an
//!   IMMEDIATE transaction, so two server threads — or two processes — cannot both win).
//! * `fencing_token` strictly increases per project across acquisitions, expiries, forced releases and restarts
//!   (`project_state.fencing_seq` is stored, never reset).
//! * An expired lease cannot be renewed: the holder must acquire again, getting a new token, and re-check the
//!   revision. Writes carrying an old lease id or token are refused, so a client that lost its lease can never
//!   overwrite the next editor's data.
//! * Every mutation also states the revision it was based on (`expected_revision`); a mismatch is a conflict, never
//!   last-write-wins.
use crate::db::{audit, now_ms, random_token};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use serde_json::json;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Lease {
    pub project: String,
    pub lease_id: String,
    pub user: String,
    pub session_id: String,
    pub client: Option<String>,
    pub fencing_token: i64,
    pub acquired_ms: i64,
    pub expires_ms: i64,
    pub revision: i64,
}

/// Public view of somebody else's lease (no lease id: that is the holder's write capability).
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Holder {
    pub user: String,
    pub client: Option<String>,
    pub expires_ms: i64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LeaseError {
    /// Another session holds an unexpired lease.
    Held(Holder),
    /// The presented lease is not (or no longer) the current one, or the token is stale.
    Lost(String),
    /// The write was based on another revision.
    Revision { expected: i64, actual: i64 },
    Db(String),
}

impl From<rusqlite::Error> for LeaseError {
    fn from(e: rusqlite::Error) -> Self {
        LeaseError::Db(e.to_string())
    }
}

type LResult<T> = Result<T, LeaseError>;

fn ensure_state(conn: &Connection, project: &str) -> LResult<()> {
    conn.execute("INSERT OR IGNORE INTO project_state (name) VALUES (?1)", params![project])?;
    Ok(())
}

pub fn revision(conn: &Connection, project: &str) -> LResult<i64> {
    Ok(conn
        .query_row("SELECT revision FROM project_state WHERE name = ?1", params![project], |r| r.get(0))
        .optional()?
        .unwrap_or(0))
}

fn current(conn: &Connection, project: &str) -> LResult<Option<Lease>> {
    Ok(conn
        .query_row(
            "SELECT l.lease_id, l.user_name, l.session_id, l.client, l.fencing_token, l.acquired_ms, l.expires_ms, s.revision
             FROM project_leases l JOIN project_state s ON s.name = l.name WHERE l.name = ?1",
            params![project],
            |r| {
                Ok(Lease {
                    project: project.to_string(),
                    lease_id: r.get(0)?,
                    user: r.get(1)?,
                    session_id: r.get(2)?,
                    client: r.get(3)?,
                    fencing_token: r.get(4)?,
                    acquired_ms: r.get(5)?,
                    expires_ms: r.get(6)?,
                    revision: r.get(7)?,
                })
            },
        )
        .optional()?)
}

/// Unexpired lease of a project, if any (for listings and the legacy-web guard).
pub fn active(conn: &Connection, project: &str, now: i64) -> LResult<Option<Holder>> {
    Ok(current(conn, project)?.filter(|l| l.expires_ms > now).map(|l| Holder { user: l.user, client: l.client, expires_ms: l.expires_ms }))
}

/// Acquire (or, for the same session, re-confirm) the edit lease.
pub fn acquire(conn: &mut Connection, project: &str, user: &str, session_id: &str, client: Option<&str>, ttl_ms: i64) -> LResult<Lease> {
    let now = now_ms();
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    ensure_state(&tx, project)?;
    if let Some(l) = current(&tx, project)? {
        if l.expires_ms > now {
            if l.session_id == session_id {
                tx.execute("UPDATE project_leases SET expires_ms = ?1 WHERE name = ?2", params![now + ttl_ms, project])?;
                tx.commit()?;
                return Ok(Lease { expires_ms: now + ttl_ms, ..l });
            }
            return Err(LeaseError::Held(Holder { user: l.user, client: l.client, expires_ms: l.expires_ms }));
        }
        audit(&tx, Some(&l.user), "lease.expired", "project", Some(project), &json!({"fencing_token": l.fencing_token})).map_err(LeaseError::Db)?;
        tx.execute("DELETE FROM project_leases WHERE name = ?1", params![project])?;
    }
    tx.execute("UPDATE project_state SET fencing_seq = fencing_seq + 1 WHERE name = ?1", params![project])?;
    let token: i64 = tx.query_row("SELECT fencing_seq FROM project_state WHERE name = ?1", params![project], |r| r.get(0))?;
    let lease_id = random_token(16);
    tx.execute(
        "INSERT INTO project_leases (name, lease_id, user_name, session_id, client, fencing_token, acquired_ms, expires_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![project, lease_id, user, session_id, client, token, now, now + ttl_ms],
    )?;
    audit(&tx, Some(user), "lease.acquire", "project", Some(project), &json!({"fencing_token": token, "client": client})).map_err(LeaseError::Db)?;
    let lease = current(&tx, project)?.expect("just inserted");
    tx.commit()?;
    Ok(lease)
}

fn check_owner(l: &Option<Lease>, lease_id: &str, session_id: &str, now: i64) -> LResult<Lease> {
    match l {
        Some(l) if l.lease_id == lease_id && l.session_id == session_id && l.expires_ms > now => Ok(l.clone()),
        Some(l) if l.lease_id == lease_id && l.expires_ms <= now => Err(LeaseError::Lost("lease expired; acquire it again".into())),
        Some(l) if l.expires_ms > now && l.lease_id != lease_id => {
            Err(LeaseError::Held(Holder { user: l.user.clone(), client: l.client.clone(), expires_ms: l.expires_ms }))
        }
        _ => Err(LeaseError::Lost("no such lease (released, expired or taken over)".into())),
    }
}

pub fn renew(conn: &mut Connection, project: &str, lease_id: &str, session_id: &str, ttl_ms: i64) -> LResult<Lease> {
    let now = now_ms();
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let l = check_owner(&current(&tx, project)?, lease_id, session_id, now)?;
    tx.execute("UPDATE project_leases SET expires_ms = ?1 WHERE name = ?2 AND lease_id = ?3", params![now + ttl_ms, project, lease_id])?;
    tx.commit()?;
    Ok(Lease { expires_ms: now + ttl_ms, ..l })
}

pub fn release(conn: &mut Connection, project: &str, lease_id: &str, session_id: &str) -> LResult<()> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let l = current(&tx, project)?;
    match &l {
        Some(l) if l.lease_id == lease_id && l.session_id == session_id => {
            tx.execute("DELETE FROM project_leases WHERE name = ?1 AND lease_id = ?2", params![project, lease_id])?;
            audit(&tx, Some(&l.user), "lease.release", "project", Some(project), &json!({"fencing_token": l.fencing_token})).map_err(LeaseError::Db)?;
            tx.commit()?;
            Ok(())
        }
        _ => Err(LeaseError::Lost("not the current lease".into())),
    }
}

/// Administrator override; the next acquisition gets a higher fencing token, so the old holder's writes fail.
pub fn force_release(conn: &mut Connection, project: &str, admin: &str, reason: &str) -> LResult<Option<Holder>> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let l = current(&tx, project)?;
    if let Some(l) = &l {
        tx.execute("DELETE FROM project_leases WHERE name = ?1", params![project])?;
        audit(&tx, Some(admin), "lease.force_release", "project", Some(project), &json!({"holder": l.user, "client": l.client, "fencing_token": l.fencing_token, "reason": reason}))
            .map_err(LeaseError::Db)?;
    }
    tx.commit()?;
    Ok(l.map(|l| Holder { user: l.user, client: l.client, expires_ms: l.expires_ms }))
}

/// Releases every lease of a session (logout).
pub fn release_session(conn: &mut Connection, session_id: &str) -> LResult<usize> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let n = tx.execute("DELETE FROM project_leases WHERE session_id = ?1", params![session_id])?;
    tx.commit()?;
    Ok(n)
}

/// Write guard for a project mutation. Runs `write` inside the same IMMEDIATE transaction after checking lease id,
/// session, fencing token and expected revision; on success the revision is incremented and returned.
pub fn guarded_write<T>(
    conn: &mut Connection,
    project: &str,
    lease_id: &str,
    session_id: &str,
    fencing_token: i64,
    expected_revision: i64,
    user: &str,
    write: impl FnOnce(&Connection) -> Result<T, String>,
) -> LResult<(T, i64)> {
    let now = now_ms();
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let l = check_owner(&current(&tx, project)?, lease_id, session_id, now)?;
    if l.fencing_token != fencing_token {
        return Err(LeaseError::Lost(format!("stale fencing token {} (current {})", fencing_token, l.fencing_token)));
    }
    let rev = revision(&tx, project)?;
    if rev != expected_revision {
        return Err(LeaseError::Revision { expected: expected_revision, actual: rev });
    }
    let out = write(&tx).map_err(LeaseError::Db)?;
    tx.execute("UPDATE project_state SET revision = revision + 1, updated_ms = ?1, updated_by = ?2 WHERE name = ?3", params![now, user, project])?;
    let new_rev = revision(&tx, project)?;
    tx.commit()?;
    Ok((out, new_rev))
}

/// Revision bump for a write that came through the legacy web endpoint while no lease was held.
pub fn bump_unleased(conn: &mut Connection, project: &str, user: Option<&str>) -> LResult<i64> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    ensure_state(&tx, project)?;
    tx.execute("UPDATE project_state SET revision = revision + 1, updated_ms = ?1, updated_by = ?2 WHERE name = ?3", params![now_ms(), user, project])?;
    let r = revision(&tx, project)?;
    tx.commit()?;
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn conn() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("l.sqlite3");
        let mut c = db::open(&p).unwrap();
        db::migrate(&mut c).unwrap();
        (dir, p)
    }

    #[test]
    fn second_session_is_refused_until_release() {
        let (_d, p) = conn();
        let mut c = db::open(&p).unwrap();
        let a = acquire(&mut c, "P", "alice", "s1", Some("desktop"), 60_000).unwrap();
        assert_eq!(a.fencing_token, 1);
        match acquire(&mut c, "P", "bob", "s2", None, 60_000) {
            Err(LeaseError::Held(h)) => assert_eq!(h.user, "alice"),
            other => panic!("{:?}", other),
        }
        // Same session re-acquiring keeps the lease and token.
        let again = acquire(&mut c, "P", "alice", "s1", Some("desktop"), 60_000).unwrap();
        assert_eq!(again.lease_id, a.lease_id);
        release(&mut c, "P", &a.lease_id, "s1").unwrap();
        let b = acquire(&mut c, "P", "bob", "s2", None, 60_000).unwrap();
        assert_eq!(b.fencing_token, 2);
    }

    #[test]
    fn expired_lease_cannot_be_renewed_and_its_writes_fail() {
        let (_d, p) = conn();
        let mut c = db::open(&p).unwrap();
        let a = acquire(&mut c, "P", "alice", "s1", None, 60_000).unwrap();
        // Expire it.
        c.execute("UPDATE project_leases SET expires_ms = 0", []).unwrap();
        assert!(matches!(renew(&mut c, "P", &a.lease_id, "s1", 60_000), Err(LeaseError::Lost(_))));
        let b = acquire(&mut c, "P", "bob", "s2", None, 60_000).unwrap();
        assert!(b.fencing_token > a.fencing_token);
        // Alice's delayed write with her old lease/token is refused; Bob's goes through.
        let r = guarded_write(&mut c, "P", &a.lease_id, "s1", a.fencing_token, 0, "alice", |_| Ok(()));
        assert!(matches!(r, Err(LeaseError::Held(_))), "{:?}", r);
        let (_, rev) = guarded_write(&mut c, "P", &b.lease_id, "s2", b.fencing_token, 0, "bob", |_| Ok(())).unwrap();
        assert_eq!(rev, 1);
    }

    #[test]
    fn stale_revision_and_token_are_conflicts() {
        let (_d, p) = conn();
        let mut c = db::open(&p).unwrap();
        let a = acquire(&mut c, "P", "alice", "s1", None, 60_000).unwrap();
        guarded_write(&mut c, "P", &a.lease_id, "s1", a.fencing_token, 0, "alice", |_| Ok(())).unwrap();
        assert_eq!(
            guarded_write(&mut c, "P", &a.lease_id, "s1", a.fencing_token, 0, "alice", |_| Ok(())).unwrap_err(),
            LeaseError::Revision { expected: 0, actual: 1 }
        );
        assert!(matches!(guarded_write(&mut c, "P", &a.lease_id, "s1", a.fencing_token - 1, 1, "alice", |_| Ok(())), Err(LeaseError::Lost(_))));
        // A failing write rolls back: revision unchanged.
        assert!(guarded_write(&mut c, "P", &a.lease_id, "s1", a.fencing_token, 1, "alice", |_| Err::<(), _>("disk full".into())).is_err());
        assert_eq!(revision(&c, "P").unwrap(), 1);
    }

    #[test]
    fn force_release_by_admin_fences_old_holder() {
        let (_d, p) = conn();
        let mut c = db::open(&p).unwrap();
        let a = acquire(&mut c, "P", "alice", "s1", None, 60_000).unwrap();
        let h = force_release(&mut c, "P", "admin", "crashed workstation").unwrap().unwrap();
        assert_eq!(h.user, "alice");
        let b = acquire(&mut c, "P", "bob", "s2", None, 60_000).unwrap();
        assert!(guarded_write(&mut c, "P", &a.lease_id, "s1", a.fencing_token, 0, "alice", |_| Ok(())).is_err());
        assert_eq!(b.fencing_token, 2);
        let n: i64 = c.query_row("SELECT COUNT(*) FROM audit WHERE action = 'lease.force_release'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn state_survives_reopen_like_a_restart() {
        let (_d, p) = conn();
        let a = {
            let mut c = db::open(&p).unwrap();
            acquire(&mut c, "P", "alice", "s1", None, 60_000).unwrap()
        };
        let mut c = db::open(&p).unwrap();
        assert!(matches!(acquire(&mut c, "P", "bob", "s2", None, 60_000), Err(LeaseError::Held(_))));
        renew(&mut c, "P", &a.lease_id, "s1", 60_000).unwrap();
        c.execute("UPDATE project_leases SET expires_ms = 0", []).unwrap();
        drop(c);
        let mut c = db::open(&p).unwrap();
        let b = acquire(&mut c, "P", "bob", "s2", None, 60_000).unwrap();
        assert_eq!(b.fencing_token, 2, "fencing sequence persisted across reopen");
    }

    #[test]
    fn concurrent_acquire_has_exactly_one_winner() {
        let (_d, p) = conn();
        let winners = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let p = p.clone();
                let w = winners.clone();
                std::thread::spawn(move || {
                    // Separate connections, like separate server processes.
                    let mut c = db::open(&p).unwrap();
                    if acquire(&mut c, "RACE", &format!("u{}", i), &format!("s{}", i), None, 60_000).is_ok() {
                        w.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(winners.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
