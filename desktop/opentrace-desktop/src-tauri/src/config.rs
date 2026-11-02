//! Per-user desktop settings (not laboratory standards): server address, transport consent, last user name, cache
//! limit. Stored as JSON in the OS config directory; passwords and tokens are never written here.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct DesktopConfig {
    pub server_url: Option<String>,
    /// The user explicitly accepted unencrypted HTTP for this server (trusted, segmented network only).
    pub allow_insecure_http: bool,
    pub username: Option<String>,
    /// Extra trusted CA certificate (PEM) for a laboratory-internal CA, in addition to the OS trust store.
    pub ca_cert_pem: Option<PathBuf>,
    pub fsa_cache_max_mib: u64,
    /// Upload original FSA files to the server in the background after local analysis.
    pub sync_fsa: bool,
    /// Where the project library lives: "local" (this computer) or "server" (organisation server, with locks).
    pub project_source: String,
}

impl Default for DesktopConfig {
    fn default() -> Self {
        DesktopConfig { server_url: None, allow_insecure_http: false, username: None, ca_cert_pem: None, fsa_cache_max_mib: 4096, sync_fsa: true, project_source: "local".into() }
    }
}

impl DesktopConfig {
    pub fn load(path: &Path) -> DesktopConfig {
        std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self).unwrap()).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }
}

/// Normalises and checks a server URL. Plain `http://` is accepted only with explicit consent; there is no automatic
/// fallback from HTTPS to HTTP on TLS errors.
pub fn normalize_server_url(raw: &str, allow_insecure_http: bool) -> Result<String, String> {
    let u = url::Url::parse(raw.trim()).map_err(|e| format!("Неверный адрес сервера: {}", e))?;
    match u.scheme() {
        "https" => {}
        "http" if allow_insecure_http => {}
        "http" => return Err("HTTP без шифрования: пароли, сеансы, генетические данные и FSA передаются открыто. Используйте https:// или явно разрешите HTTP для доверенной изолированной сети.".into()),
        s => return Err(format!("Схема {} не поддерживается (https:// или http://)", s)),
    }
    if u.host_str().map(|h| h.is_empty()).unwrap_or(true) {
        return Err("В адресе нет имени сервера".into());
    }
    if !u.username().is_empty() || u.password().is_some() || u.query().is_some() || u.fragment().is_some() {
        return Err("Адрес сервера не должен содержать логин, пароль, параметры или #".into());
    }
    Ok(u.as_str().trim_end_matches('/').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_required_unless_explicit_http_consent() {
        assert_eq!(normalize_server_url("https://lab.local:8443/", false).unwrap(), "https://lab.local:8443");
        assert!(normalize_server_url("http://10.0.0.5:8770", false).is_err());
        assert_eq!(normalize_server_url("http://10.0.0.5:8770", true).unwrap(), "http://10.0.0.5:8770");
        assert!(normalize_server_url("ftp://x", true).is_err());
        assert!(normalize_server_url("https://user:pw@x", false).is_err());
        assert!(normalize_server_url("https://x/?a=1", false).is_err());
    }

    #[test]
    fn roundtrip_without_secrets() {
        let d = std::env::temp_dir().join(format!("otd-cfg-{}", std::process::id()));
        let p = d.join("desktop.json");
        let c = DesktopConfig { server_url: Some("https://x".into()), username: Some("u".into()), ..Default::default() };
        c.save(&p).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(!text.contains("token") && !text.contains("password"));
        assert_eq!(DesktopConfig::load(&p).server_url.as_deref(), Some("https://x"));
        let _ = std::fs::remove_dir_all(d);
    }
}
