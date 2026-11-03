//! Native "Save As" for exports.
//!
//! The web UI exports by clicking a temporary `<a download>` with a blob URL; in a browser that is a download. The
//! shim turns such a click into `POST /desktop/api/save-as?name=…` with the bytes, and this module asks where to save
//! (system dialog, parented to the window that exported, starting in the folder used last, file type from the name)
//! and writes the file atomically (temporary file in the target folder, then rename). Cancel writes nothing.
//!
//! Runs on a request worker thread: the dialog itself is created on the UI thread by tauri-plugin-dialog and awaited
//! here, so the UI thread keeps pumping messages (no window is moved under a Tauri lock).
//!
//! Self-test runs: no dialog; the file goes to `OPENTRACE_SELFTEST_SAVE_DIR` or `<data dir>/selftest-exports`.
use crate::state::DesktopState;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

static LAST_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// A file name the user's file system accepts (the UI already replaces most of these; this is the last guard).
pub fn safe_file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let s: String = base.chars().map(|c| if (c as u32) < 0x20 || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
    let s = s.trim().trim_end_matches(['.', ' ']).to_string();
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || ((stem.starts_with("COM") || stem.starts_with("LPT")) && stem.len() == 4 && stem.as_bytes()[3].is_ascii_digit());
    if s.is_empty() || reserved {
        format!("export_{}", s)
    } else {
        s.chars().take(200).collect()
    }
}

/// Writes `data` to `path` via a temporary file in the same folder and a rename.
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<(), String> {
    let dir = path.parent().ok_or("no folder")?;
    let tmp = dir.join(format!(".{}.{}.part", path.file_name().and_then(|n| n.to_str()).unwrap_or("export"), std::process::id()));
    let r = (|| -> std::io::Result<()> {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if r.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    r.map_err(|e| format!("{}: {}", path.display(), e))
}

pub fn save_as(app: &AppHandle, st: &DesktopState, label: &str, name: &str, data: &[u8]) -> Result<Option<PathBuf>, String> {
    let name = safe_file_name(name);
    if st.selftest.is_some() {
        let dir = std::env::var_os("OPENTRACE_SELFTEST_SAVE_DIR").filter(|d| !d.is_empty()).map(PathBuf::from).unwrap_or_else(|| st.data_dir.join("selftest-exports"));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let p = dir.join(&name);
        write_atomic(&p, data)?;
        return Ok(Some(p));
    }
    let start = LAST_DIR.lock().unwrap_or_else(|e| e.into_inner()).clone().or_else(|| app.path().download_dir().ok());
    let mut d = app.dialog().file().set_file_name(&name);
    if let Some(dir) = start.filter(|d| d.is_dir()) {
        d = d.set_directory(dir);
    }
    if let Some(ext) = Path::new(&name).extension().and_then(|e| e.to_str()).filter(|e| !e.is_empty() && e.len() <= 10) {
        d = d.add_filter(ext.to_ascii_uppercase(), &[ext]);
    }
    if let Some(w) = app.get_webview_window(label) {
        d = d.set_parent(&w);
    }
    let Some(chosen) = d.blocking_save_file() else { return Ok(None) };
    let path = chosen.into_path().map_err(|e| e.to_string())?;
    write_atomic(&path, data)?;
    if let Some(dir) = path.parent() {
        *LAST_DIR.lock().unwrap_or_else(|e| e.into_inner()) = Some(dir.to_path_buf());
    }
    crate::dlog!("[desktop] export saved ({} bytes)", data.len());
    Ok(Some(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_atomic_write() {
        assert_eq!(safe_file_name("Kit_Panels.txt"), "Kit_Panels.txt");
        assert_eq!(safe_file_name("a/b\\c:d?.txt"), "c_d_.txt");
        assert_eq!(safe_file_name("CON.txt"), "export_CON.txt");
        assert_eq!(safe_file_name("com1"), "export_com1");
        assert_eq!(safe_file_name("table. "), "table");
        assert_eq!(safe_file_name(""), "export_");
        let dir = std::env::temp_dir().join(format!("ot-saveas-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("x.txt");
        write_atomic(&p, b"one").unwrap();
        write_atomic(&p, b"two").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"two");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1); // no temporary file left
        let _ = std::fs::remove_dir_all(dir);
    }
}
