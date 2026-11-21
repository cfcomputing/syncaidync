//! Thin git helpers: list args only, no shell.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;
use tokio::time::timeout;

#[derive(Debug)]
pub struct GitError {
    pub message: String,
    pub stderr: String,
}

impl GitError {
    pub fn new(message: impl Into<String>, stderr: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            stderr: stderr.into(),
        }
    }
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for GitError {}

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);

pub async fn run(cwd: &Path, args: &[&str]) -> Result<String, GitError> {
    run_timeout(cwd, args, DEFAULT_TIMEOUT).await
}

pub async fn run_timeout(cwd: &Path, args: &[&str], limit: Duration) -> Result<String, GitError> {
    let output = run_bytes(cwd, args, limit).await?;
    Ok(String::from_utf8_lossy(&output).trim().to_string())
}

pub async fn run_paths(cwd: &Path, args: &[&str]) -> Result<Vec<String>, GitError> {
    let output = run_bytes(cwd, args, DEFAULT_TIMEOUT).await?;
    String::from_utf8(output)
        .map(|paths| paths.split_terminator('\0').map(str::to_owned).collect())
        .map_err(|_| GitError::new("Git reported a path that is not valid UTF-8.", ""))
}

async fn run_bytes(cwd: &Path, args: &[&str], limit: Duration) -> Result<Vec<u8>, GitError> {
    let mut cmd = Command::new("git");
    cmd.env_remove("SYNCAIDYNC_KEY");
    cmd.args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .kill_on_drop(true);

    let child = cmd
        .spawn()
        .map_err(|e| GitError::new(format!("Could not run git: {e}"), String::new()))?;

    let output = timeout(limit, child.wait_with_output())
        .await
        .map_err(|_| GitError::new("git timed out.", String::new()))?
        .map_err(|e| GitError::new(format!("git failed: {e}"), String::new()))?;

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !output.status.success() {
        let msg = if stderr.is_empty() {
            format!("git {} failed.", args.first().unwrap_or(&"command"))
        } else {
            stderr.lines().next().unwrap_or("git failed.").to_string()
        };
        return Err(GitError::new(msg, stderr));
    }
    Ok(output.stdout)
}

pub async fn is_repo(path: &Path) -> bool {
    run(path, &["rev-parse", "--is-inside-work-tree"])
        .await
        .map(|s| s == "true")
        .unwrap_or(false)
}

pub async fn toplevel(path: &Path) -> Result<PathBuf, GitError> {
    let out = run(path, &["rev-parse", "--show-toplevel"]).await?;
    Ok(PathBuf::from(out))
}

pub async fn head_commit(path: &Path) -> Result<String, GitError> {
    run(path, &["rev-parse", "HEAD"]).await
}

pub async fn current_branch(path: &Path) -> Result<String, GitError> {
    run(path, &["branch", "--show-current"]).await
}

pub async fn rev_parse(path: &Path, rev: &str) -> Result<String, GitError> {
    run(path, &["rev-parse", "--verify", rev]).await
}

pub async fn default_branch(path: &Path) -> Result<String, GitError> {
    if let Ok(sym) = run(path, &["symbolic-ref", "refs/remotes/origin/HEAD"]).await {
        if let Some(name) = sym.rsplit('/').next() {
            if !name.is_empty() {
                return Ok(name.to_string());
            }
        }
    }
    for candidate in ["main", "master"] {
        if rev_parse(path, candidate).await.is_ok() {
            return Ok(candidate.to_string());
        }
    }
    current_branch(path).await
}

pub async fn branches(path: &Path) -> Result<Vec<String>, GitError> {
    let out = run(
        path,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname:short)",
            "refs/heads",
        ],
    )
    .await?;
    let mut list: Vec<String> = out
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .take(200)
        .collect();
    // Newest first already from --sort=-committerdate.
    if list.is_empty() {
        if let Ok(cur) = current_branch(path).await {
            if !cur.is_empty() {
                list.push(cur);
            }
        }
    }
    Ok(list)
}

pub async fn branch_exists(path: &Path, branch: &str) -> Result<bool, GitError> {
    Ok(rev_parse(path, &format!("refs/heads/{branch}"))
        .await
        .is_ok())
}

pub async fn check_branch_name(branch: &str) -> Result<(), GitError> {
    // check-ref-format does not need a repo cwd; use /tmp or current dir.
    let cwd = std::env::temp_dir();
    run(&cwd, &["check-ref-format", "--branch", branch])
        .await
        .map(|_| ())
        .map_err(|_| {
            GitError::new(
                "That branch name is not valid. Use letters, numbers, /, -, and _.",
                String::new(),
            )
        })
}

pub async fn is_dirty(path: &Path) -> Result<bool, GitError> {
    let out = run(path, &["status", "--porcelain"]).await?;
    Ok(!out.is_empty())
}

pub async fn unpushed_count(path: &Path) -> Result<u64, GitError> {
    // Count commits on HEAD not in upstream; if no upstream, compare to base if set later.
    match run(path, &["rev-list", "--count", "@{upstream}..HEAD"]).await {
        Ok(s) => Ok(s.parse().unwrap_or(0)),
        Err(_) => Ok(0),
    }
}

pub async fn commits_ahead_of(path: &Path, base_commit: &str) -> Result<u64, GitError> {
    let out = run(
        path,
        &["rev-list", "--count", &format!("{base_commit}..HEAD")],
    )
    .await?;
    Ok(out.parse().unwrap_or(0))
}

pub async fn worktree_add(
    repo: &Path,
    branch: &str,
    folder: &Path,
    base: &str,
) -> Result<(), GitError> {
    run(
        repo,
        &[
            "worktree",
            "add",
            "-b",
            branch,
            &folder.to_string_lossy(),
            base,
        ],
    )
    .await
    .map(|_| ())
}

pub async fn worktree_remove(repo: &Path, folder: &Path) -> Result<(), GitError> {
    run(
        repo,
        &["worktree", "remove", "--force", &folder.to_string_lossy()],
    )
    .await
    .map(|_| ())
}

pub async fn delete_branch(repo: &Path, branch: &str) -> Result<(), GitError> {
    run(repo, &["branch", "-D", branch]).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as StdCommand;

    fn temp_repo() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("syncaidync-git-{}", uuid_like()));
        std::fs::create_dir_all(&dir).unwrap();
        StdCommand::new("git")
            .args(["init", "-b", "main"])
            .current_dir(&dir)
            .output()
            .unwrap();
        StdCommand::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&dir)
            .output()
            .unwrap();
        StdCommand::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&dir)
            .output()
            .unwrap();
        std::fs::write(dir.join("README"), "hi").unwrap();
        StdCommand::new("git")
            .args(["add", "."])
            .current_dir(&dir)
            .output()
            .unwrap();
        StdCommand::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(&dir)
            .output()
            .unwrap();
        dir
    }

    fn uuid_like() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("{n:x}")
    }

    #[tokio::test]
    async fn repo_basics() {
        let dir = temp_repo();
        assert!(is_repo(&dir).await);
        assert_eq!(current_branch(&dir).await.unwrap(), "main");
        assert_eq!(default_branch(&dir).await.unwrap(), "main");
        let head = head_commit(&dir).await.unwrap();
        assert_eq!(head.len(), 40);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
