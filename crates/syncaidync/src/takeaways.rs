//! Read transcripts, suggest takeaways, and write them to destination files.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Serialize;
use syncaidync_core::{
    build_takeaways_prompt, condense_claude, condense_terminal, merge_takeaways,
    normalize_takeaway, parse_takeaways_reply, takeaway_destination, AgentId, DestCtx, Session,
    Takeaway, WorkspaceKind, TRANSCRIPT_BUDGET,
};

use crate::git;
use crate::http::error::ApiError;
use crate::oneshot;
use crate::store;

const MAX_TRANSCRIPT_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct Destinations {
    pub everywhere: Option<String>,
    pub project: Option<String>,
    pub note: Option<String>,
}

pub async fn destinations_for(session: &Session, sync_home: &Path) -> Destinations {
    let ctx = dest_ctx(session, sync_home).await;
    Destinations {
        everywhere: takeaway_destination(session.agent, "everywhere", &ctx),
        project: takeaway_destination(session.agent, "project", &ctx),
        note: takeaway_destination(session.agent, "note", &ctx),
    }
}

async fn dest_ctx(session: &Session, sync_home: &Path) -> DestCtx {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"));
    let repo = resolve_repo_toplevel(session).await;
    DestCtx {
        home,
        syncaidync_home: sync_home.to_path_buf(),
        repo,
        claude_config_dir: std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from),
        codex_home: std::env::var_os("CODEX_HOME").map(PathBuf::from),
        pi_coding_agent_dir: std::env::var_os("PI_CODING_AGENT_DIR").map(PathBuf::from),
    }
}

fn read_lossy(path: &Path) -> Result<String, ApiError> {
    let bytes = std::fs::read(path).map_err(|_| ApiError::internal())?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn read_source(session: &Session, sync_home: &Path) -> Result<(String, bool), ApiError> {
    if let Some(path) = session.transcript.as_ref() {
        let p = PathBuf::from(path);
        if p.is_file() {
            let meta = std::fs::metadata(&p).map_err(|_| ApiError::internal())?;
            if meta.len() <= MAX_TRANSCRIPT_BYTES {
                // Transcript paths are JSONL (Claude). Path presence is the signal, not agent id.
                return Ok((read_lossy(&p)?, true));
            }
        }
    }
    let log = sync_home.join("logs").join(format!("{}.log", session.id));
    if log.is_file() {
        let meta = std::fs::metadata(&log).map_err(|_| ApiError::internal())?;
        if meta.len() > MAX_TRANSCRIPT_BYTES {
            return Err(ApiError::unprocessable(
                "The session log is too large to suggest takeaways.",
            ));
        }
        return Ok((read_lossy(&log)?, false));
    }
    Err(ApiError::unprocessable(
        "There is no transcript or terminal log for this session yet.",
    ))
}

/// The session's transcript (or terminal log) condensed to `budget` bytes; empty when there is
/// none.
pub fn condensed(session: &Session, sync_home: &Path, budget: usize) -> String {
    match read_source(session, sync_home) {
        Ok((raw, true)) => condense_claude(&raw, budget),
        Ok((raw, false)) => condense_terminal(&raw, budget),
        Err(_) => String::new(),
    }
}

pub async fn suggest(
    session: &Session,
    sync_home: &Path,
    agent: AgentId,
    model: Option<&str>,
) -> Result<Vec<Takeaway>, ApiError> {
    if matches!(agent, AgentId::Shell) {
        return Err(ApiError::bad_request(
            "Pick Claude, Codex, Pi, or Cursor Agent to suggest takeaways.",
        ));
    }
    let (raw, is_jsonl) = read_source(session, sync_home)?;
    let condensed = if is_jsonl {
        condense_claude(&raw, TRANSCRIPT_BUDGET)
    } else {
        condense_terminal(&raw, TRANSCRIPT_BUDGET)
    };
    if condensed.trim().is_empty() {
        return Err(ApiError::unprocessable(
            "There is not enough session material to suggest takeaways.",
        ));
    }
    let label = match agent {
        AgentId::Claude => "Claude Code",
        AgentId::Codex => "Codex",
        AgentId::Pi => "Pi",
        AgentId::Cursor => "Cursor Agent",
        AgentId::Shell => "Shell",
    };
    let mut prompt = build_takeaways_prompt(&condensed, label);
    if let Some(summary) = crate::goals::takeaways_summary(sync_home, session).await {
        prompt.push_str(
            "\nThis session worked toward a goal with checks that SyncAiDync ran after every \
             turn. Prefer lessons about what took several rounds to get right. Summary:\n",
        );
        prompt.push_str(&summary);
    }
    let reply = oneshot::run(agent, model, &prompt)
        .await
        .map_err(oneshot::OneshotError::into_api)?;
    Ok(parse_takeaways_reply(&reply))
}

#[derive(Debug, Clone, Serialize)]
pub struct Written {
    pub scope: String,
    pub path: String,
    pub count: u32,
}

pub async fn save(
    session: &Session,
    sync_home: &Path,
    takeaways: &[Takeaway],
) -> Result<Vec<Written>, ApiError> {
    if takeaways.is_empty() {
        return Err(ApiError::bad_request("There are no takeaways to save."));
    }
    let cleaned: Vec<Takeaway> = takeaways.iter().filter_map(normalize_takeaway).collect();
    if cleaned.is_empty() {
        return Err(ApiError::bad_request(
            "Each takeaway needs a title and a body.",
        ));
    }
    let ctx = dest_ctx(session, sync_home).await;
    let allowed: BTreeSet<String> = ["everywhere", "project", "note"]
        .iter()
        .filter_map(|s| takeaway_destination(session.agent, s, &ctx))
        .collect();

    let mut by_path: BTreeMap<String, Vec<Takeaway>> = BTreeMap::new();
    for t in cleaned {
        let path = takeaway_destination(session.agent, &t.scope, &ctx).ok_or_else(|| {
            if t.scope == "project" {
                ApiError::unprocessable(
                    "That takeaway has scope project, but this session is not in a git repository.",
                )
            } else {
                ApiError::bad_request(format!("Unknown takeaway scope: {}", t.scope))
            }
        })?;
        if !allowed.contains(&path) {
            return Err(ApiError::bad_request(
                "Refusing to write a takeaway outside the allowed destinations.",
            ));
        }
        by_path.entry(path).or_default().push(t);
    }

    static SAVE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _save = SAVE.lock().await;
    let date = local_ymd().await?;
    let mut written = Vec::new();
    for (path, group) in by_path {
        let existing = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => {
                return Err(ApiError::unprocessable(format!(
                    "Could not read {path}: {e}"
                )))
            }
        };
        let merged = merge_takeaways(&existing, &group, &date);
        store::atomic_write(Path::new(&path), merged.as_bytes())
            .map_err(ApiError::unprocessable)?;
        written.push(Written {
            scope: group[0].scope.clone(),
            path,
            count: group.len() as u32,
        });
    }
    Ok(written)
}

/// Resolve the main repo path for a worktree (so project takeaways outlive the worktree).
pub async fn resolve_repo_toplevel(session: &Session) -> Option<PathBuf> {
    if let Some(repo) = &session.workspace.repo {
        let p = PathBuf::from(repo);
        if git::is_repo(&p).await {
            return git::toplevel(&p).await.ok().or(Some(p));
        }
    }
    if session.workspace.kind == WorkspaceKind::Folder
        || session.workspace.kind == WorkspaceKind::Worktree
    {
        let path = PathBuf::from(&session.workspace.path);
        if git::is_repo(&path).await {
            return git::toplevel(&path).await.ok();
        }
    }
    None
}

async fn local_ymd() -> Result<String, ApiError> {
    let output = tokio::process::Command::new("/bin/date")
        .arg("+%Y-%m-%d")
        .output()
        .await
        .map_err(|_| ApiError::internal())?;
    if !output.status.success() {
        return Err(ApiError::internal());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
