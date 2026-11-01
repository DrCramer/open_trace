//! Native windows for the existing multi-window UI.
//!
//! The web UI opens Samples/Genotypes Plot, Panel Manager, Report Manager and print preview with `window.open` and
//! talks to them through `window.opener.OT` (synchronous object access). Each `window.open` becomes a real native
//! window created by `on_new_window` + `window_features`, which hands the new webview to the engine as the popup of
//! the opener (same WebView2 environment / related WebKitGTK view), so `window.opener` keeps working. Geometry is
//! saved per window kind ("plot", "panels", ...) by the window-state plugin, so a plot window placed on the second
//! monitor reopens there.
use crate::state::DesktopState;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::webview::{DownloadEvent, NewWindowFeatures, NewWindowResponse};
use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder, Wry};

/// `OPENTRACE_DESKTOP_TRACE=1`: log page loads, popups and scheme requests (paths only, no bodies) to stderr.
pub fn trace() -> bool {
    static T: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *T.get_or_init(|| std::env::var_os("OPENTRACE_DESKTOP_TRACE").is_some_and(|v| !v.is_empty()))
}

/// Origin of the scheme as the platform webview exposes it.
pub fn base_url() -> Url {
    if cfg!(windows) {
        Url::parse("http://opentrace.localhost/").unwrap()
    } else {
        Url::parse("opentrace://localhost/").unwrap()
    }
}

/// Only our own scheme (and the blank/blob documents the UI creates itself) may be loaded in a window.
pub fn allowed(url: &Url) -> bool {
    match url.scheme() {
        "opentrace" => url.host_str() == Some("localhost"),
        "http" | "https" => url.host_str() == Some("opentrace.localhost"),
        "about" => url.as_str() == "about:blank",
        "blob" => url.path().starts_with("http://opentrace.localhost/") || url.path().starts_with("opentrace://localhost/"),
        _ => false,
    }
}

/// Window kind for labels and saved geometry: `/idx/plot.html` -> "plot".
pub fn kind_of(url: &Url) -> String {
    let stem = url.path().rsplit('/').next().unwrap_or("").trim_end_matches(".html");
    let k: String = stem.chars().filter(|c| c.is_ascii_alphanumeric()).take(20).collect();
    if k.is_empty() {
        "window".into()
    } else {
        k
    }
}

/// `plot-12` -> `plot` (window-state key).
pub fn state_key(label: &str) -> &str {
    match label.rfind('-') {
        Some(i) if label[i + 1..].chars().all(|c| c.is_ascii_digit()) => &label[..i],
        _ => label,
    }
}

fn decorate<'a>(app: &AppHandle, st: &Arc<DesktopState>, b: WebviewWindowBuilder<'a, Wry, AppHandle>) -> WebviewWindowBuilder<'a, Wry, AppHandle> {
    let app_nw = app.clone();
    let app_dl = app.clone();
    let _ = st;
    // Desktop scripts are injected into the HTML by bridge::inject_scripts (works in every window on every platform).
    b
        // HTML5 drag & drop of .fsa files into the Add Samples dialog (the native handler would swallow the events).
        .disable_drag_drop_handler()
        .on_navigation(|u| {
            let ok = allowed(u);
            if !ok {
                eprintln!("[desktop] blocked navigation to {}", u);
            }
            ok
        })
        .on_new_window(move |url, features| popup(&app_nw, url, features))
        .on_page_load(|w, p| {
            if trace() {
                eprintln!("[trace] page {} {:?} {}", w.label(), p.event(), p.url());
            }
        })
        .on_document_title_changed(|w, title| {
            let _ = w.set_title(if title.trim().is_empty() { "OpenTrace" } else { &title });
        })
        .on_download(move |webview, event| match event {
            DownloadEvent::Requested { url, destination } => {
                // Exports go to the user's Downloads folder unless the engine already chose an absolute path.
                if !destination.is_absolute() || destination.as_os_str().is_empty() {
                    let name = destination.file_name().map(|n| n.to_string_lossy().into_owned()).filter(|n| !n.is_empty()).unwrap_or_else(|| "export".into());
                    if let Ok(dir) = app_dl.path().download_dir() {
                        *destination = unique_path(&dir, &name);
                    }
                }
                eprintln!("[desktop] download {} -> {}", url.scheme(), destination.display());
                true
            }
            DownloadEvent::Finished { path, success, .. } => {
                let p = path.map(|p| p.display().to_string()).unwrap_or_default();
                let js = format!("window.__otDesktopDownloaded && window.__otDesktopDownloaded({}, {})", serde_json::to_string(&p).unwrap(), success);
                let _ = webview.eval(&js);
                true
            }
            _ => true,
        })
}

fn unique_path(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let p = dir.join(name);
    if !p.exists() {
        return p;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    for n in 1..1000 {
        let c = dir.join(format!("{} ({}){}", stem, n, ext));
        if !c.exists() {
            return c;
        }
    }
    p
}

/// WebKitGTK blocks `window.open` that is not directly inside a user gesture; WebView2 allows it. The UI opens plot,
/// report and print windows after asynchronous work (e.g. after a dialog), so align Linux with Windows.
pub fn platform_tweaks(w: &WebviewWindow) {
    #[cfg(target_os = "linux")]
    {
        let _ = w.with_webview(|wv| {
            use webkit2gtk::{SettingsExt, WebViewExt};
            if let Some(s) = WebViewExt::settings(&wv.inner()) {
                s.set_javascript_can_open_windows_automatically(true);
            }
        });
    }
    #[cfg(not(target_os = "linux"))]
    let _ = w;
}

pub fn create_main(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let st = app.state::<Arc<DesktopState>>().inner().clone();
    let url = base_url().join("idx/").unwrap();
    let b = WebviewWindowBuilder::new(app, "main", WebviewUrl::CustomProtocol(url))
        .title("OpenTrace")
        .inner_size(1400.0, 900.0)
        .min_inner_size(900.0, 560.0);
    let w = decorate(app, &st, b).build()?;
    platform_tweaks(&w);
    Ok(w)
}

fn popup(app: &AppHandle, url: Url, features: NewWindowFeatures) -> NewWindowResponse<Wry> {
    if !allowed(&url) {
        eprintln!("[desktop] window.open denied for {}", url);
        return NewWindowResponse::Deny;
    }
    let st = app.state::<Arc<DesktopState>>().inner().clone();
    let n = st.popup_seq.fetch_add(1, Ordering::SeqCst) + 1;
    let label = format!("{}-{}", kind_of(&url), n);
    if trace() {
        eprintln!("[trace] window.open {} -> {} (features: size {:?})", url, label, features.size());
    }
    let b = WebviewWindowBuilder::new(app, &label, WebviewUrl::External("about:blank".parse().unwrap())).window_features(features).title("OpenTrace");
    match decorate(app, &st, b).build() {
        Ok(window) => {
            platform_tweaks(&window);
            NewWindowResponse::Create { window }
        }
        Err(e) => {
            eprintln!("[desktop] cannot create window {}: {}", label, e);
            NewWindowResponse::Deny
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_own_scheme_is_navigable() {
        assert!(allowed(&Url::parse("opentrace://localhost/idx/plot.html").unwrap()));
        assert!(allowed(&Url::parse("http://opentrace.localhost/idx/").unwrap()));
        assert!(allowed(&Url::parse("about:blank").unwrap()));
        assert!(!allowed(&Url::parse("https://example.com/").unwrap()));
        assert!(!allowed(&Url::parse("http://opentrace.localhost.evil.com/").unwrap()));
        assert!(!allowed(&Url::parse("file:///etc/passwd").unwrap()));
        assert!(!allowed(&Url::parse("opentrace://evil/").unwrap()));
    }

    #[test]
    fn labels_and_state_keys() {
        assert_eq!(kind_of(&Url::parse("http://opentrace.localhost/idx/plot.html").unwrap()), "plot");
        assert_eq!(kind_of(&Url::parse("about:blank").unwrap()), "blank");
        assert_eq!(state_key("plot-12"), "plot");
        assert_eq!(state_key("main"), "main");
        assert_eq!(state_key("a-b"), "a-b");
    }
}
