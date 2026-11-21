//! Resolve home, port, launch key, and roots.

use std::fs;
use std::path::PathBuf;

use crate::ids::random_hex;

pub const DEFAULT_PORT: u16 = 7717;

#[derive(Debug, Clone)]
pub struct Config {
    pub home: PathBuf,
    pub port: u16,
    pub key: String,
    pub roots: Vec<PathBuf>,
    pub web_dir: Option<PathBuf>,
    pub no_open: bool,
    pub parent_pid: Option<u32>,
    pub dev_shell: bool,
}

#[derive(Debug)]
pub enum ConfigError {
    Message(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Message(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    pub fn from_env(
        port: Option<u16>,
        no_open: bool,
        parent_pid: Option<u32>,
    ) -> Result<Self, ConfigError> {
        let home = match std::env::var_os("SYNCAIDYNC_HOME") {
            Some(p) => PathBuf::from(p),
            None => dirs_home()?.join(".syncaidync"),
        };
        fs::create_dir_all(&home).map_err(|e| {
            ConfigError::Message(format!(
                "Could not create home folder {}: {e}",
                home.display()
            ))
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&home, fs::Permissions::from_mode(0o700));
        }

        let port = port
            .or_else(|| {
                std::env::var("SYNCAIDYNC_PORT")
                    .ok()
                    .and_then(|s| s.parse().ok())
            })
            .unwrap_or(DEFAULT_PORT);

        let key = match std::env::var("SYNCAIDYNC_KEY") {
            Ok(k) => {
                if k.chars().count() < 32 {
                    return Err(ConfigError::Message(
                        "SYNCAIDYNC_KEY must be at least 32 characters.".into(),
                    ));
                }
                k
            }
            Err(_) => random_hex(32),
        };

        let roots = match std::env::var("SYNCAIDYNC_ROOTS") {
            Ok(s) if !s.is_empty() => s
                .split(':')
                .filter(|p| !p.is_empty())
                .map(PathBuf::from)
                .collect(),
            _ => default_roots(),
        };

        let web_dir = std::env::var_os("SYNCAIDYNC_WEB_DIR").map(PathBuf::from);
        let dev_shell = std::env::var_os("SYNCAIDYNC_DEV_SHELL").is_some_and(|v| v == "1");

        Ok(Self {
            home,
            port,
            key,
            roots,
            web_dir,
            no_open,
            parent_pid,
            dev_shell,
        })
    }

    pub fn cookie_name(&self) -> String {
        // Use the configured port; after binding to 0 the server updates this.
        format!("syncaidync_{}", self.port)
    }
}

fn dirs_home() -> Result<PathBuf, ConfigError> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| ConfigError::Message("HOME is not set.".into()))
}

fn default_roots() -> Vec<PathBuf> {
    let Ok(home) = dirs_home() else {
        return Vec::new();
    };
    let mut roots = Vec::new();
    for name in [
        "code",
        "src",
        "dev",
        "work",
        "projects",
        "repos",
        "git",
        "Developer",
        "Documents/GitHub",
    ] {
        let p = home.join(name);
        if p.is_dir() {
            roots.push(p);
        }
    }
    roots.push(home);
    roots
}
