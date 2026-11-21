//! Create and remove session workspaces.

use std::path::{Path, PathBuf};

use syncaidync_core::{Workspace, WorkspaceKind};

use crate::git::{self, GitError};

#[derive(Debug)]
pub enum WorkspaceError {
    BadRequest(String),
    Conflict(String),
    Unprocessable(String),
    Internal(String),
}

impl WorkspaceError {
    pub fn bad(m: impl Into<String>) -> Self {
        Self::BadRequest(m.into())
    }
    pub fn conflict(m: impl Into<String>) -> Self {
        Self::Conflict(m.into())
    }
    pub fn unprocessable(m: impl Into<String>) -> Self {
        Self::Unprocessable(m.into())
    }
    pub fn internal(m: impl Into<String>) -> Self {
        Self::Internal(m.into())
    }
}

impl From<GitError> for WorkspaceError {
    fn from(e: GitError) -> Self {
        Self::Unprocessable(e.message)
    }
}

pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    if path == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home);
        }
    }
    PathBuf::from(path)
}

/// `dync/<slug>` from a title (or "session").
pub fn suggest_branch(title: &str) -> String {
    let slug = slugify(title.trim());
    let slug = if slug.is_empty() {
        "session".to_string()
    } else {
        slug
    };
    format!("dync/{slug}")
}

fn slugify(input: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in input.chars() {
        let c = ch.to_ascii_lowercase();
        let ok = c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-';
        if ok {
            out.push(c);
            prev_dash = c == '-';
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.len() > 60 {
        out.truncate(60);
        while out.ends_with('-') {
            out.pop();
        }
    }
    out
}

/// Folder name under worktrees/ from a branch like `dync/fix-login`.
pub fn worktree_slug(branch: &str) -> String {
    let raw = branch.strip_prefix("dync/").unwrap_or(branch);
    let s = slugify(raw);
    if s.is_empty() {
        "session".into()
    } else {
        s
    }
}

pub fn unique_worktree_path(home: &Path, repo: &Path, branch: &str) -> PathBuf {
    let repo_name = repo.file_name().and_then(|s| s.to_str()).unwrap_or("repo");
    let base_slug = worktree_slug(branch);
    let root = home.join("worktrees").join(repo_name);
    let mut path = root.join(&base_slug);
    if !path.exists() {
        return path;
    }
    for n in 2..1000 {
        path = root.join(format!("{base_slug}-{n}"));
        if !path.exists() {
            return path;
        }
    }
    root.join(format!("{base_slug}-x"))
}

pub async fn create_worktree(
    home: &Path,
    repo: &str,
    base: &str,
    branch: &str,
) -> Result<Workspace, WorkspaceError> {
    let repo_path = expand_tilde(repo);
    if !repo_path.is_dir() {
        return Err(WorkspaceError::bad(
            "That repository folder does not exist.",
        ));
    }
    if !git::is_repo(&repo_path).await {
        return Err(WorkspaceError::bad("That folder is not a git repository."));
    }
    let toplevel = git::toplevel(&repo_path).await?;
    git::check_branch_name(branch)
        .await
        .map_err(|e| WorkspaceError::bad(e.message))?;
    if git::branch_exists(&toplevel, branch).await? {
        return Err(WorkspaceError::conflict(
            "That branch already exists. Pick another name.",
        ));
    }
    let _ = git::rev_parse(&toplevel, base)
        .await
        .map_err(|_| WorkspaceError::bad(format!("Could not resolve base branch '{base}'.")))?;
    let base_commit = git::rev_parse(&toplevel, base).await?;

    let folder = unique_worktree_path(home, &toplevel, branch);
    if let Some(parent) = folder.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            WorkspaceError::internal(format!("Could not create worktrees folder: {e}"))
        })?;
    }
    git::worktree_add(&toplevel, branch, &folder, base).await?;

    Ok(Workspace {
        kind: WorkspaceKind::Worktree,
        path: folder.to_string_lossy().into_owned(),
        repo: Some(toplevel.to_string_lossy().into_owned()),
        branch: Some(branch.to_string()),
        base: Some(base.to_string()),
        base_commit: Some(base_commit),
        owned: true,
    })
}

pub async fn create_folder(path: &str) -> Result<Workspace, WorkspaceError> {
    let path = expand_tilde(path);
    let meta =
        std::fs::metadata(&path).map_err(|_| WorkspaceError::bad("That folder does not exist."))?;
    if !meta.is_dir() {
        return Err(WorkspaceError::bad("That path is not a folder."));
    }
    let mut ws = Workspace {
        kind: WorkspaceKind::Folder,
        path: path.to_string_lossy().into_owned(),
        repo: None,
        branch: None,
        base: None,
        base_commit: None,
        owned: false,
    };
    if git::is_repo(&path).await {
        if let Ok(top) = git::toplevel(&path).await {
            ws.repo = Some(top.to_string_lossy().into_owned());
            ws.branch = git::current_branch(&top)
                .await
                .ok()
                .filter(|s| !s.is_empty());
            ws.base = git::default_branch(&top).await.ok();
            ws.base_commit = git::head_commit(&top).await.ok();
        }
    }
    Ok(ws)
}

pub fn create_scratch(home: &Path, session_id: &str) -> Result<Workspace, WorkspaceError> {
    let path = home.join("scratch").join(session_id);
    std::fs::create_dir_all(&path)
        .map_err(|e| WorkspaceError::internal(format!("scratch: {e}")))?;
    Ok(Workspace {
        kind: WorkspaceKind::Scratch,
        path: path.to_string_lossy().into_owned(),
        repo: None,
        branch: None,
        base: None,
        base_commit: None,
        owned: true,
    })
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RemovalInfo {
    pub owned: bool,
    pub dirty: bool,
    pub unpushed: u64,
    pub path: String,
}

pub async fn removal_info(
    session_path: &str,
    owned: bool,
    base_commit: Option<&str>,
) -> RemovalInfo {
    let path = PathBuf::from(session_path);
    let mut dirty = false;
    let mut unpushed = 0u64;
    if path.is_dir() && git::is_repo(&path).await {
        dirty = git::is_dirty(&path).await.unwrap_or(false);
        unpushed = if let Some(base) = base_commit {
            git::commits_ahead_of(&path, base).await.unwrap_or(0)
        } else {
            git::unpushed_count(&path).await.unwrap_or(0)
        };
    }
    RemovalInfo {
        owned,
        dirty,
        unpushed,
        path: session_path.to_string(),
    }
}

pub async fn remove_owned(home: &Path, ws: &Workspace) -> Result<(), WorkspaceError> {
    if !ws.owned {
        return Ok(());
    }
    let home = home
        .canonicalize()
        .map_err(|e| WorkspaceError::internal(format!("home: {e}")))?;
    let path = PathBuf::from(&ws.path)
        .canonicalize()
        .map_err(|_| WorkspaceError::bad("That workspace folder is gone."))?;
    if path == home || !path.starts_with(&home) {
        return Err(WorkspaceError::unprocessable(
            "Refusing to delete a folder outside SyncAiDync's home.",
        ));
    }

    match ws.kind {
        WorkspaceKind::Worktree => {
            if let Some(repo) = &ws.repo {
                let repo = PathBuf::from(repo);
                git::worktree_remove(&repo, &path)
                    .await
                    .map_err(|e| WorkspaceError::internal(format!("Remove worktree: {e}")))?;
                if let (Some(branch), Some(base_commit)) = (&ws.branch, &ws.base_commit) {
                    let ahead = git::run(
                        &repo,
                        &["rev-list", "--count", &format!("{base_commit}..{branch}")],
                    )
                    .await
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1);
                    if ahead == 0 {
                        let _ = git::delete_branch(&repo, branch).await;
                    }
                }
            } else {
                remove_workspace_dir(&path)?;
            }
        }
        WorkspaceKind::Scratch => {
            remove_workspace_dir(&path)?;
        }
        WorkspaceKind::Folder => {}
    }
    Ok(())
}

fn remove_workspace_dir(path: &Path) -> Result<(), WorkspaceError> {
    std::fs::remove_dir_all(path)
        .map_err(|e| WorkspaceError::internal(format!("Remove workspace folder: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggest_and_slug() {
        assert_eq!(suggest_branch("Fix Login!"), "dync/fix-login");
        assert_eq!(worktree_slug("dync/Fix Login"), "fix-login");
        assert_eq!(suggest_branch(""), "dync/session");
        assert_eq!(slugify(&"a".repeat(80)).len(), 60);
    }

    #[test]
    fn tilde() {
        if let Ok(home) = std::env::var("HOME") {
            assert_eq!(expand_tilde("~/code"), PathBuf::from(home).join("code"));
        }
    }
}
