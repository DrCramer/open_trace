//! Server configuration: TOML file + `OPENTRACE_SERVER_*` environment overrides.
use serde::Deserialize;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

/// How clients reach the server. `http` on a non-loopback address needs `allow_insecure_http = true`.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    /// TLS terminates at a reverse proxy (Caddy, nginx, IIS ARR) in front of a loopback listener. Recommended.
    Proxy,
    /// Plain HTTP. Passwords, session tokens, genotypes and FSA files travel unencrypted.
    Http,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    /// `host:port` to listen on.
    pub listen: String,
    /// Root of all server state (database, FSA store, projects, security, logs).
    pub data_dir: PathBuf,
    /// Default target directory of `opentrace-server backup`.
    pub backup_dir: Option<PathBuf>,
    /// Host names (without port) the legacy web UI may be opened under (DNS-rebinding guard of the web UI).
    pub allowed_hosts: Vec<String>,
    pub transport: Transport,
    /// Must be set explicitly to serve plain HTTP on a non-loopback address.
    pub allow_insecure_http: bool,
    pub session_ttl_hours: u64,
    pub lease_ttl_seconds: u64,
    pub max_fsa_mib: u64,
    /// Body limit of the legacy `/api/*` endpoints (project save still carries base64 FSA there).
    pub legacy_max_body_mib: u64,
    /// Serve the existing web UI (`/idx/`, `/api/*`) next to API v1.
    pub legacy_web: bool,
    /// Location of `web/` and `data/` (defaults to the usual search: env, next to the binary, source tree).
    pub resources: Option<PathBuf>,
    /// Login attempts per client IP per minute (in addition to the account lockout policy of the security store).
    pub login_attempts_per_minute: u32,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            listen: "127.0.0.1:8770".into(),
            data_dir: default_data_dir(),
            backup_dir: None,
            allowed_hosts: vec!["localhost".into(), "127.0.0.1".into()],
            transport: Transport::Proxy,
            allow_insecure_http: false,
            session_ttl_hours: 12,
            lease_ttl_seconds: 90,
            max_fsa_mib: 64,
            legacy_max_body_mib: 1024,
            legacy_web: true,
            resources: None,
            login_attempts_per_minute: 10,
        }
    }
}

/// `%ProgramData%\OpenTrace\server` on Windows, `$XDG_DATA_HOME/opentrace-server` (or `~/.local/share/...`) elsewhere.
pub fn default_data_dir() -> PathBuf {
    if cfg!(windows) {
        let base = std::env::var_os("ProgramData").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
        return base.join("OpenTrace").join("server");
    }
    if let Some(x) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(x).join("opentrace-server");
    }
    opentrace_core::paths::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".local").join("share").join("opentrace-server")
}

impl ServerConfig {
    pub fn load(path: Option<&Path>) -> Result<ServerConfig, String> {
        let mut cfg = match path {
            Some(p) => {
                let text = std::fs::read_to_string(p).map_err(|e| format!("{}: {}", p.display(), e))?;
                toml::from_str::<ServerConfig>(&text).map_err(|e| format!("{}: {}", p.display(), e))?
            }
            None => ServerConfig::default(),
        };
        cfg.apply_env(|k| std::env::var(k).ok())?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn apply_env(&mut self, get: impl Fn(&str) -> Option<String>) -> Result<(), String> {
        let num = |k: &str, v: String| v.trim().parse::<u64>().map_err(|_| format!("{}: not a number: {}", k, v));
        let flag = |v: &str| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on");
        if let Some(v) = get("OPENTRACE_SERVER_LISTEN") {
            self.listen = v;
        }
        if let Some(v) = get("OPENTRACE_SERVER_DATA_DIR") {
            self.data_dir = PathBuf::from(v);
        }
        if let Some(v) = get("OPENTRACE_SERVER_BACKUP_DIR") {
            self.backup_dir = Some(PathBuf::from(v));
        }
        if let Some(v) = get("OPENTRACE_ALLOWED_HOSTS") {
            self.allowed_hosts = v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        }
        if let Some(v) = get("OPENTRACE_SERVER_TRANSPORT") {
            self.transport = match v.trim() {
                "proxy" => Transport::Proxy,
                "http" => Transport::Http,
                other => return Err(format!("OPENTRACE_SERVER_TRANSPORT: proxy or http, got {}", other)),
            };
        }
        if let Some(v) = get("OPENTRACE_SERVER_ALLOW_INSECURE_HTTP") {
            self.allow_insecure_http = flag(&v);
        }
        if let Some(v) = get("OPENTRACE_SERVER_LEASE_TTL_SECONDS") {
            self.lease_ttl_seconds = num("OPENTRACE_SERVER_LEASE_TTL_SECONDS", v)?;
        }
        if let Some(v) = get("OPENTRACE_SERVER_SESSION_TTL_HOURS") {
            self.session_ttl_hours = num("OPENTRACE_SERVER_SESSION_TTL_HOURS", v)?;
        }
        if let Some(v) = get("OPENTRACE_RESOURCES") {
            if !v.is_empty() {
                self.resources = Some(PathBuf::from(v));
            }
        }
        Ok(())
    }

    pub fn listen_addr(&self) -> Result<SocketAddr, String> {
        self.listen.parse::<SocketAddr>().map_err(|e| format!("listen = {:?}: {}", self.listen, e))
    }

    pub fn validate(&self) -> Result<(), String> {
        let addr = self.listen_addr()?;
        if !(10..=3600).contains(&self.lease_ttl_seconds) {
            return Err("lease_ttl_seconds must be within 10..=3600".into());
        }
        if self.session_ttl_hours == 0 || self.session_ttl_hours > 24 * 30 {
            return Err("session_ttl_hours must be within 1..=720".into());
        }
        if self.transport == Transport::Http && !addr.ip().is_loopback() && !self.allow_insecure_http {
            return Err(format!(
                "Refusing plain HTTP on {}: passwords, sessions, genetic data and FSA files would be unencrypted. \
                 Put a TLS reverse proxy in front of a loopback listener (transport = \"proxy\"), or set \
                 allow_insecure_http = true for an isolated, trusted network segment.",
                addr
            ));
        }
        if self.transport == Transport::Proxy && !addr.ip().is_loopback() {
            return Err(format!(
                "transport = \"proxy\" expects a loopback listener (127.0.0.1), the TLS proxy is the public endpoint; got {}",
                addr
            ));
        }
        Ok(())
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("opentrace.sqlite3")
    }
    pub fn fsa_dir(&self) -> PathBuf {
        self.data_dir.join("fsa")
    }
    pub fn projects_dir(&self) -> PathBuf {
        self.data_dir.join("projects")
    }
    pub fn security_dir(&self) -> PathBuf {
        self.data_dir.join("security")
    }
    pub fn tmp_dir(&self) -> PathBuf {
        self.data_dir.join("tmp")
    }
    pub fn max_fsa_bytes(&self) -> u64 {
        self.max_fsa_mib * 1024 * 1024
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_http_on_lan_needs_explicit_opt_in() {
        let mut c = ServerConfig { listen: "0.0.0.0:8770".into(), transport: Transport::Http, ..Default::default() };
        assert!(c.validate().unwrap_err().contains("Refusing plain HTTP"));
        c.allow_insecure_http = true;
        c.validate().unwrap();
    }

    #[test]
    fn proxy_mode_requires_loopback() {
        let c = ServerConfig { listen: "0.0.0.0:8770".into(), ..Default::default() };
        assert!(c.validate().is_err());
        ServerConfig::default().validate().unwrap();
    }

    #[test]
    fn env_overrides() {
        let mut c = ServerConfig::default();
        c.apply_env(|k| match k {
            "OPENTRACE_SERVER_LISTEN" => Some("127.0.0.1:9000".into()),
            "OPENTRACE_ALLOWED_HOSTS" => Some("a, b,,c".into()),
            "OPENTRACE_SERVER_TRANSPORT" => Some("http".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(c.listen, "127.0.0.1:9000");
        assert_eq!(c.allowed_hosts, vec!["a", "b", "c"]);
        assert_eq!(c.transport, Transport::Http);
        c.validate().unwrap();
    }

    #[test]
    fn toml_file() {
        let c: ServerConfig = toml::from_str("listen = \"127.0.0.1:1234\"\nlease_ttl_seconds = 30\ntransport = \"http\"\n").unwrap();
        assert_eq!(c.lease_ttl_seconds, 30);
        assert!(toml::from_str::<ServerConfig>("bogus = 1").is_err());
    }
}
