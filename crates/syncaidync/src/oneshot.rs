//! Run an agent once for plans / takeaways (no tools, empty temp cwd).

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use syncaidync_core::{adapter_for, AgentId, OneShotPlan};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::Semaphore;
use tokio::time::timeout;

use crate::agents_discover;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(240);
const MAX_OUTPUT: usize = 2 * 1024 * 1024;
const MAX_ARG_PROMPT: usize = 120_000;

fn semaphore() -> &'static Semaphore {
    static SEM: OnceLock<Semaphore> = OnceLock::new();
    SEM.get_or_init(|| Semaphore::new(2))
}

#[derive(Debug)]
pub enum OneshotError {
    Unavailable(String),
    BadRequest(String),
}

impl OneshotError {
    pub fn into_api(self) -> crate::http::error::ApiError {
        use crate::http::error::ApiError;
        use hyper::StatusCode;
        match self {
            OneshotError::Unavailable(m) => {
                ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "unavailable", m)
            }
            OneshotError::BadRequest(m) => ApiError::bad_request(m),
        }
    }
}

/// Override timeout for tests via `SYNCAIDYNC_ONESHOT_TIMEOUT_MS`.
fn timeout_limit() -> Duration {
    if let Ok(ms) = std::env::var("SYNCAIDYNC_ONESHOT_TIMEOUT_MS") {
        if let Ok(n) = ms.parse::<u64>() {
            return Duration::from_millis(n.max(50));
        }
    }
    DEFAULT_TIMEOUT
}

pub async fn run(
    agent: AgentId,
    model: Option<&str>,
    prompt: &str,
) -> Result<String, OneshotError> {
    // Build the plan without holding `&dyn Adapter` across awaits (not Sync).
    let mut plan = {
        let adapter = adapter_for(agent).ok_or_else(|| {
            OneshotError::BadRequest("That agent cannot run one-shot plans.".into())
        })?;
        adapter.oneshot(model, prompt)
    };
    if plan.stdin.is_none() && prompt.len() > MAX_ARG_PROMPT {
        return Err(OneshotError::BadRequest(
            "That prompt is too large to pass as a command argument.".into(),
        ));
    }

    let bin_name = plan.args.first().cloned().unwrap_or_default();
    let bin = agents_discover::resolve_bin_for_launch(agent, &bin_name)
        .map_err(OneshotError::Unavailable)?;
    if !plan.args.is_empty() {
        plan.args[0] = bin;
    }

    let _permit = semaphore()
        .acquire()
        .await
        .map_err(|_| OneshotError::Unavailable("Could not start a one-shot run.".into()))?;

    let tmp = make_temp_dir().map_err(OneshotError::Unavailable)?;
    let result = run_in_dir(&tmp, &plan).await;
    let _ = tokio::fs::remove_dir_all(&tmp).await;
    let (stdout, stderr, code) = result?;

    if code != Some(0) {
        return Err(OneshotError::Unavailable(format_fail(
            "The agent exited with an error.",
            &stderr,
        )));
    }

    let reply = {
        let adapter = adapter_for(agent).ok_or_else(|| {
            OneshotError::BadRequest("That agent cannot run one-shot plans.".into())
        })?;
        adapter.oneshot_reply(&stdout)
    }
    .filter(|s| !s.trim().is_empty())
    .ok_or_else(|| {
        OneshotError::Unavailable(format_fail("The agent returned an empty reply.", &stderr))
    })?;
    Ok(reply)
}

async fn run_in_dir(
    cwd: &std::path::Path,
    plan: &OneShotPlan,
) -> Result<(String, String, Option<i32>), OneshotError> {
    let mut cmd = Command::new(&plan.args[0]);
    if plan.args.len() > 1 {
        cmd.args(&plan.args[1..]);
    }
    cmd.current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env_remove("SYNCAIDYNC_KEY")
        .env_remove("SYNCAIDYNC_SESSION")
        .env_remove("SYNCAIDYNC_TOKEN")
        .env_remove("SYNCAIDYNC_PORT")
        .env_remove("SYNCAIDYNC_HOOK")
        .env("GIT_TERMINAL_PROMPT", "0");

    let mut child = cmd
        .spawn()
        .map_err(|e| OneshotError::Unavailable(format!("Could not start the agent: {e}")))?;

    if let Some(stdin_data) = &plan.stdin {
        if let Some(mut stdin) = child.stdin.take() {
            let data = stdin_data.clone();
            tokio::spawn(async move {
                let _ = stdin.write_all(data.as_bytes()).await;
                let _ = stdin.shutdown().await;
            });
        }
    } else {
        drop(child.stdin.take());
    }

    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let out_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        if let Some(mut r) = stdout.take() {
            let mut chunk = [0u8; 8192];
            loop {
                match r.read(&mut chunk).await {
                    Ok(0) => break,
                    Ok(n) => {
                        if buf.len() < MAX_OUTPUT {
                            let room = MAX_OUTPUT - buf.len();
                            buf.extend_from_slice(&chunk[..n.min(room)]);
                        }
                    }
                    Err(_) => break,
                }
            }
        }
        buf
    });
    let err_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        if let Some(mut r) = stderr.take() {
            let mut chunk = [0u8; 8192];
            loop {
                match r.read(&mut chunk).await {
                    Ok(0) => break,
                    Ok(n) => {
                        if buf.len() < MAX_OUTPUT {
                            let room = MAX_OUTPUT - buf.len();
                            buf.extend_from_slice(&chunk[..n.min(room)]);
                        }
                    }
                    Err(_) => break,
                }
            }
        }
        buf
    });

    let wait = timeout(timeout_limit(), child.wait()).await;
    match wait {
        Err(_) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            Err(OneshotError::Unavailable(
                "The agent timed out after waiting too long.".into(),
            ))
        }
        Ok(Err(e)) => Err(OneshotError::Unavailable(format!("Agent failed: {e}"))),
        Ok(Ok(status)) => {
            let out = out_task.await.unwrap_or_default();
            let err = err_task.await.unwrap_or_default();
            Ok((
                String::from_utf8_lossy(&out).into_owned(),
                String::from_utf8_lossy(&err).into_owned(),
                status.code(),
            ))
        }
    }
}

fn make_temp_dir() -> Result<PathBuf, String> {
    let mut p = std::env::temp_dir();
    p.push(format!("syncaidync-oneshot-{}", crate::ids::random_hex(8)));
    std::fs::create_dir_all(&p).map_err(|e| format!("Could not make a temp folder: {e}"))?;
    Ok(p)
}

fn format_fail(lead: &str, stderr: &str) -> String {
    let tail = {
        let t = stderr.trim();
        if t.len() <= 400 {
            t.to_string()
        } else {
            t[t.len() - 400..].to_string()
        }
    };
    if tail.is_empty() {
        lead.to_string()
    } else {
        format!("{lead} {tail}")
    }
}

/// Shared for callers that need Arc.
pub type Shared = Arc<()>;
