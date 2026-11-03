//! Remembered size and position of popup windows (plot, panels, reports …), one entry per window kind.
//!
//! Why not `tauri-plugin-window-state` for popups: the plugin restores a window from its `on_window_ready` hook, which
//! Tauri runs while holding the plugin-store lock. A popup created from `window.open` is built inside WebView2's
//! NewWindowRequested callback (dispatched by wry from a window procedure, i.e. outside the event-loop handler), so the
//! plugin's `SetWindowPos` delivers `WM_MOVE`/`WM_SIZE` synchronously, the event-loop handler asks for the same
//! plugin-store lock and the UI thread waits for itself forever (white windows, "not responding"). Observed on
//! Windows 11 with a hang dump: the second opening of the Panel Manager after it had been moved.
//!
//! Here the geometry is only recorded from window events (no window calls) and applied to the window *builder*, so the
//! popup is created at its place and nothing moves a window while a Tauri lock is held. The plugin keeps handling the
//! main window only.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

/// Physical pixels: outer position, inner size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

pub struct PopupGeometry {
    path: Option<PathBuf>,
    kinds: Mutex<HashMap<String, Geometry>>,
}

/// Minimised windows on Windows report (-32000, -32000) and a zero size; such values are never remembered.
const MIN_SIDE: u32 = 120;
const OFF_SCREEN: i32 = -30000;

impl PopupGeometry {
    /// `path = None`: in memory only (self-test).
    pub fn load(path: Option<PathBuf>) -> Self {
        let kinds = path
            .as_ref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice::<HashMap<String, Geometry>>(&b).ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|(_, g)| g.width >= MIN_SIDE && g.height >= MIN_SIDE && g.x > OFF_SCREEN && g.y > OFF_SCREEN)
            .collect();
        PopupGeometry { path, kinds: Mutex::new(kinds) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Geometry>> {
        self.kinds.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn get(&self, kind: &str) -> Option<Geometry> {
        self.lock().get(kind).copied().filter(|g| g.width > 0 && g.height > 0)
    }

    pub fn moved(&self, kind: &str, x: i32, y: i32) {
        if x <= OFF_SCREEN || y <= OFF_SCREEN {
            return;
        }
        let mut g = self.lock();
        let e = g.entry(kind.to_string()).or_default();
        e.x = x;
        e.y = y;
    }

    pub fn resized(&self, kind: &str, width: u32, height: u32) {
        if width < MIN_SIDE || height < MIN_SIDE {
            return;
        }
        let mut g = self.lock();
        let e = g.entry(kind.to_string()).or_default();
        e.width = width;
        e.height = height;
    }

    /// Written when a popup closes and on exit (small file next to `desktop.json`; tmp + rename).
    pub fn save(&self) {
        let Some(p) = &self.path else { return };
        let json = {
            let g = self.lock();
            let keep: HashMap<&String, &Geometry> = g.iter().filter(|(_, v)| v.width > 0 && v.height > 0).collect();
            serde_json::to_vec_pretty(&keep).unwrap_or_default()
        };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = p.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, p);
        }
    }
}

/// True when the rectangle (physical) overlaps a monitor by at least 50×50 px, so a remembered place on a monitor that
/// is no longer connected is not used.
pub fn on_some_monitor(g: &Geometry, monitors: &[(i32, i32, u32, u32)]) -> bool {
    monitors.iter().any(|&(mx, my, mw, mh)| {
        let x0 = g.x.max(mx) as i64;
        let y0 = g.y.max(my) as i64;
        let x1 = (g.x as i64 + g.width as i64).min(mx as i64 + mw as i64);
        let y1 = (g.y as i64 + g.height as i64).min(my as i64 + mh as i64);
        x1 - x0 >= 50 && y1 - y0 >= 50
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimised_and_tiny_values_are_not_remembered_and_file_round_trips() {
        let dir = std::env::temp_dir().join(format!("ot-geom-{}", std::process::id()));
        let path = dir.join("popup-geometry.json");
        let g = PopupGeometry::load(Some(path.clone()));
        assert_eq!(g.get("plot"), None);
        g.moved("plot", 100, 50);
        g.resized("plot", 900, 600);
        g.moved("plot", -32000, -32000); // minimised
        g.resized("plot", 0, 0);
        assert_eq!(g.get("plot"), Some(Geometry { x: 100, y: 50, width: 900, height: 600 }));
        g.moved("panels", 10, 10); // position only: not usable yet
        assert_eq!(g.get("panels"), None);
        g.save();
        let h = PopupGeometry::load(Some(path));
        assert_eq!(h.get("plot"), Some(Geometry { x: 100, y: 50, width: 900, height: 600 }));
        assert_eq!(h.get("panels"), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn monitor_check() {
        let mons = [(0, 0, 1920, 1080), (1920, 0, 2560, 1440)];
        assert!(on_some_monitor(&Geometry { x: 100, y: 100, width: 800, height: 600 }, &mons));
        assert!(on_some_monitor(&Geometry { x: 3000, y: 200, width: 800, height: 600 }, &mons));
        assert!(!on_some_monitor(&Geometry { x: 5000, y: 100, width: 800, height: 600 }, &mons));
        assert!(!on_some_monitor(&Geometry { x: -900, y: 100, width: 920, height: 600 }, &mons)); // 20 px visible
    }
}
