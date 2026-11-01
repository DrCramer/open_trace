//! Portable per-user locations. `HOME` is honoured first (it is what the Unix code always used and what the tests
//! and Docker image set); Windows falls back to `USERPROFILE`, then `HOMEDRIVE`+`HOMEPATH`.
use std::path::PathBuf;

fn env_path(name: &str) -> Option<PathBuf> {
    match std::env::var_os(name) {
        Some(v) if !v.is_empty() => Some(PathBuf::from(v)),
        _ => None,
    }
}

/// The current user's home directory, if one can be determined.
pub fn home_dir() -> Option<PathBuf> {
    if let Some(h) = env_path("HOME") {
        return Some(h);
    }
    if cfg!(windows) {
        if let Some(h) = env_path("USERPROFILE") {
            return Some(h);
        }
        if let (Some(d), Some(p)) = (std::env::var_os("HOMEDRIVE"), std::env::var_os("HOMEPATH")) {
            let mut s = d;
            s.push(p);
            return Some(PathBuf::from(s));
        }
    }
    None
}
