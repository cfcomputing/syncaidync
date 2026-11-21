//! Run the GitHub CLI (`gh`) with a timeout.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::timeout;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub enum GhError {
    Unavailable(String),
    Failed(String),
}

impl GhError {
    pub fn into_api(self) -> crate::http::error::ApiError {
        use crate::http::error::ApiError;
        use hyper::StatusCode;
        match self {
            GhError::Unavailable(m) => {
                ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "unavailable", m)
            }
            GhError::Failed(m) => ApiError::unprocessable(m),
        }
    }
}

fn gh_bin() -> PathBuf {
    if let Ok(p) = std::env::var("SYNCAIDYNC_BIN_GH") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    PathBuf::from("gh")
}

/// Ensure `gh` is installed and signed in.
pub async fn ensure_auth() -> Result<(), GhError> {
    let bin = gh_bin();
    let mut cmd = Command::new(&bin);
    cmd.env_remove("SYNCAIDYNC_KEY");
    cmd.args(["auth", "status"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1")
        .kill_on_drop(true);
    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(GhError::Unavailable(
                "Install the GitHub CLI (gh) to use pull requests.".into(),
            ));
        }
        Err(e) => {
            return Err(GhError::Unavailable(format!(
                "Install the GitHub CLI (gh) to use pull requests. ({e})"
            )));
        }
    };
    let output = timeout(DEFAULT_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| GhError::Unavailable("The GitHub CLI did not respond in time.".into()))?
        .map_err(|e| GhError::Unavailable(format!("Could not run gh: {e}")))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let out = String::from_utf8_lossy(&output.stdout);
        let text = format!("{err}\n{out}").to_ascii_lowercase();
        if text.contains("not logged")
            || text.contains("not signed")
            || text.contains("you are not logged")
            || text.contains("authentication")
            || output.status.code() == Some(1)
        {
            return Err(GhError::Unavailable(
                "The GitHub CLI is not signed in. Run: gh auth login".into(),
            ));
        }
        return Err(GhError::Unavailable(
            "Install the GitHub CLI (gh) to use pull requests.".into(),
        ));
    }
    Ok(())
}

/// Run `gh` with the given args; optional stdin body.
pub async fn run(args: &[&str], stdin: Option<&str>) -> Result<String, GhError> {
    run_in(None, args, stdin).await
}

pub async fn run_in(
    cwd: Option<&Path>,
    args: &[&str],
    stdin: Option<&str>,
) -> Result<String, GhError> {
    ensure_auth().await?;
    let bin = gh_bin();
    let mut cmd = Command::new(&bin);
    cmd.env_remove("SYNCAIDYNC_KEY");
    cmd.args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1")
        .kill_on_drop(true);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    if stdin.is_some() {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            GhError::Unavailable("Install the GitHub CLI (gh) to use pull requests.".into())
        } else {
            GhError::Unavailable(format!("Could not run gh: {e}"))
        }
    })?;
    if let Some(body) = stdin {
        if let Some(mut inp) = child.stdin.take() {
            inp.write_all(body.as_bytes())
                .await
                .map_err(|e| GhError::Failed(format!("Could not write to gh: {e}")))?;
        }
    }
    let output = timeout(DEFAULT_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| GhError::Failed("gh timed out.".into()))?
        .map_err(|e| GhError::Failed(format!("gh failed: {e}")))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !output.status.success() {
        let msg = if stderr.is_empty() {
            "The GitHub CLI request failed.".into()
        } else {
            stderr.lines().next().unwrap_or("gh failed.").to_string()
        };
        return Err(GhError::Failed(msg));
    }
    Ok(stdout)
}
