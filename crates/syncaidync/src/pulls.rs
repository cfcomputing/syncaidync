//! Pull request list: open, fetch, changes, draft review, session.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use syncaidync_core::{
    draft_payload, parse_ref, progress_for, remote_matches, AgentId, Pull, PullProgress, Workspace,
    WorkspaceKind,
};
use tokio::sync::Mutex;

use crate::changes::{self, ChangeSource};
use crate::discovery;
use crate::gh::{self, GhError};
use crate::git;
use crate::http::error::ApiError;
use crate::http::sse::Hub;
use crate::ids::uuid_v4;
use crate::markdown;
use crate::registry::{CreateError, PreparedSession, Registry};
use crate::reviews::Reviews;
use crate::store::{self, Saver};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PullsFile {
    version: u32,
    pulls: Vec<Pull>,
}

pub struct Pulls {
    inner: Arc<Mutex<HashMap<String, Pull>>>,
    saver: Saver<PullsFile>,
    hub: Hub,
    home: PathBuf,
    roots: Vec<PathBuf>,
    prepare: Mutex<()>,
}

impl Pulls {
    pub fn load(home: &Path, hub: Hub, roots: Vec<PathBuf>) -> Self {
        let path = home.join("pulls.json");
        let list = match store::load::<PullsFile>(&path) {
            Ok(Some(f)) => f.pulls,
            _ => Vec::new(),
        };
        let mut map = HashMap::new();
        for p in list {
            map.insert(p.id.clone(), p);
        }
        Self {
            inner: Arc::new(Mutex::new(map)),
            saver: Saver::new(path),
            hub,
            home: home.to_path_buf(),
            roots,
            prepare: Mutex::new(()),
        }
    }

    pub async fn list(&self) -> Vec<Pull> {
        let g = self.inner.lock().await;
        let mut v: Vec<_> = g.values().cloned().collect();
        v.sort_by_key(|p| std::cmp::Reverse(p.added_at));
        v
    }

    pub async fn get(&self, id: &str) -> Option<Pull> {
        self.inner.lock().await.get(id).cloned()
    }

    async fn persist(&self) {
        let g = self.inner.lock().await;
        let mut pulls: Vec<_> = g.values().cloned().collect();
        pulls.sort_by_key(|p| p.added_at);
        let file = PullsFile { version: 1, pulls };
        drop(g);
        self.saver.queue(file).await;
    }

    async fn store_publish(&self, pull: Pull) {
        {
            let mut g = self.inner.lock().await;
            g.insert(pull.id.clone(), pull.clone());
        }
        self.hub.publish("pull", &pull);
        self.persist().await;
    }

    pub async fn open(&self, ref_text: &str) -> Result<Pull, ApiError> {
        let pref = parse_ref(ref_text).map_err(ApiError::bad_request)?;
        let _prepare = self.prepare.lock().await;
        {
            let g = self.inner.lock().await;
            if let Some(existing) = g.values().find(|p| {
                p.owner.eq_ignore_ascii_case(&pref.owner)
                    && p.repo.eq_ignore_ascii_case(&pref.repo)
                    && p.number == pref.number
            }) {
                return Ok(existing.clone());
            }
        }

        let view = pr_view(&pref.owner, &pref.repo, pref.number).await?;
        let clone = match find_clone(&self.roots, &self.home, &pref.owner, &pref.repo) {
            Some(clone) => clone,
            None => {
                self.clone_for_review(&pref.owner, &pref.repo, &view.base_ref)
                    .await?
            }
        };
        let (remote, head_commit, base_commit) = fetch_pull(
            &clone,
            &pref.owner,
            &pref.repo,
            pref.number,
            &view,
            clone.starts_with(self.home.join("pull-clones")),
        )
        .await?;
        let _ = remote;

        let now = now_ms();
        let pull = Pull {
            id: uuid_v4(),
            owner: pref.owner,
            repo: pref.repo,
            number: pref.number,
            url: view.url,
            title: view.title,
            author: view.author,
            body_html: markdown::to_safe_html(&view.body),
            state: view.state,
            draft: view.draft,
            base_ref: view.base_ref,
            head_ref: view.head_ref,
            base_commit,
            head_commit,
            clone: clone.to_string_lossy().into_owned(),
            status: "active".into(),
            session: None,
            added_at: now,
            fetched_at: now,
            progress: PullProgress {
                files: 0,
                reviewed: 0,
            },
        };
        self.store_publish(pull.clone()).await;
        Ok(pull)
    }

    async fn clone_for_review(
        &self,
        owner: &str,
        repo: &str,
        base: &str,
    ) -> Result<PathBuf, ApiError> {
        if [owner, repo].iter().any(|name| matches!(*name, "." | "..")) {
            return Err(ApiError::bad_request("Enter a GitHub pull request URL."));
        }
        let root = self
            .home
            .join("pull-clones")
            .join(format!("owner-{}", owner.to_ascii_lowercase()));
        let clone = root.join(format!("repo-{}", repo.to_ascii_lowercase()));
        if matching_remote(&clone, owner, repo).is_some() {
            return Ok(clone);
        }
        tokio::fs::create_dir_all(&root)
            .await
            .map_err(|_| ApiError::internal())?;
        gh::run(
            &[
                "repo",
                "clone",
                &format!("https://github.com/{owner}/{repo}"),
                &clone.to_string_lossy(),
                "--",
                "--no-checkout",
                "--no-tags",
                "--depth=1",
                "--branch",
                base,
            ],
            None,
        )
        .await
        .map_err(GhError::into_api)?;
        Ok(clone)
    }

    pub async fn refresh(&self, id: &str) -> Result<Pull, ApiError> {
        let _prepare = self.prepare.lock().await;
        let mut pull = self
            .get(id)
            .await
            .ok_or_else(|| ApiError::not_found("No pull request with that id."))?;
        let view = pr_view(&pull.owner, &pull.repo, pull.number).await?;
        let clone = PathBuf::from(&pull.clone);
        let (_remote, head_commit, base_commit) = fetch_pull(
            &clone,
            &pull.owner,
            &pull.repo,
            pull.number,
            &view,
            clone.starts_with(self.home.join("pull-clones")),
        )
        .await?;
        pull.url = view.url;
        pull.title = view.title;
        pull.author = view.author;
        pull.body_html = markdown::to_safe_html(&view.body);
        pull.state = view.state;
        pull.draft = view.draft;
        pull.base_ref = view.base_ref;
        pull.head_ref = view.head_ref;
        pull.head_commit = head_commit;
        pull.base_commit = base_commit;
        pull.fetched_at = now_ms();
        self.store_publish(pull.clone()).await;
        Ok(pull)
    }

    pub async fn changes(
        &self,
        id: &str,
        refresh: bool,
        reviews: &Reviews,
    ) -> Result<syncaidync_core::ChangeSet, ApiError> {
        let pull = if refresh {
            self.refresh(id).await?
        } else {
            self.get(id)
                .await
                .ok_or_else(|| ApiError::not_found("No pull request with that id."))?
        };
        let cs = changes::gather_async(ChangeSource::Commits {
            repo: PathBuf::from(&pull.clone),
            base: pull.base_commit.clone(),
            head: pull.head_commit.clone(),
            base_label: pull.base_ref.clone(),
            head_label: pull.head_ref.clone(),
        })
        .await
        .map_err(changes::ChangesError::into_api)?;

        let review = reviews.get(&Reviews::pull_key(id)).await;
        let progress = progress_for(&review, &cs.files);
        if progress != pull.progress {
            let mut updated = pull;
            updated.progress = progress;
            self.store_publish(updated).await;
        }
        Ok(cs)
    }

    pub async fn update_progress(&self, id: &str, reviews: &Reviews) -> Result<Pull, ApiError> {
        let pull = self
            .get(id)
            .await
            .ok_or_else(|| ApiError::not_found("No pull request with that id."))?;
        let cs = changes::gather_async(ChangeSource::Commits {
            repo: PathBuf::from(&pull.clone),
            base: pull.base_commit.clone(),
            head: pull.head_commit.clone(),
            base_label: pull.base_ref.clone(),
            head_label: pull.head_ref.clone(),
        })
        .await
        .map_err(changes::ChangesError::into_api)?;
        let review = reviews.get(&Reviews::pull_key(id)).await;
        let mut updated = pull;
        updated.progress = progress_for(&review, &cs.files);
        self.store_publish(updated.clone()).await;
        Ok(updated)
    }

    pub async fn set_status(&self, id: &str, status: &str) -> Result<Pull, ApiError> {
        if status != "active" && status != "done" {
            return Err(ApiError::bad_request("Status must be active or done."));
        }
        let mut pull = self
            .get(id)
            .await
            .ok_or_else(|| ApiError::not_found("No pull request with that id."))?;
        pull.status = status.into();
        self.store_publish(pull.clone()).await;
        Ok(pull)
    }

    pub async fn delete(&self, id: &str, reviews: &Reviews) -> Result<(), ApiError> {
        let pull = {
            let mut g = self.inner.lock().await;
            g.remove(id)
                .ok_or_else(|| ApiError::not_found("No pull request with that id."))?
        };
        reviews.remove_pull(id).await;
        let clone = PathBuf::from(&pull.clone);
        let _ = git::run(
            &clone,
            &[
                "update-ref",
                "-d",
                &format!("refs/syncaidync/pull/{}/head", pull.number),
            ],
        )
        .await;
        let _ = git::run(
            &clone,
            &[
                "update-ref",
                "-d",
                &format!("refs/syncaidync/pull/{}/base", pull.number),
            ],
        )
        .await;
        self.hub
            .publish("pull_gone", &serde_json::json!({ "id": id }));
        self.persist().await;
        Ok(())
    }

    pub async fn draft(&self, id: &str, reviews: &Reviews) -> Result<String, ApiError> {
        let pull = self
            .get(id)
            .await
            .ok_or_else(|| ApiError::not_found("No pull request with that id."))?;
        let cs = changes::gather_async(ChangeSource::Commits {
            repo: PathBuf::from(&pull.clone),
            base: pull.base_commit.clone(),
            head: pull.head_commit.clone(),
            base_label: pull.base_ref.clone(),
            head_label: pull.head_ref.clone(),
        })
        .await
        .map_err(changes::ChangesError::into_api)?;
        let review = reviews.get(&Reviews::pull_key(id)).await;
        let payload =
            draft_payload(&pull.head_commit, &review, &cs).map_err(ApiError::unprocessable)?;
        let body = serde_json::to_string(&payload).map_err(|_| ApiError::internal())?;
        let path = format!(
            "repos/{}/{}/pulls/{}/reviews",
            pull.owner, pull.repo, pull.number
        );
        let out = match gh::run(
            &["api", "--method", "POST", &path, "--input", "-"],
            Some(&body),
        )
        .await
        {
            Ok(o) => o,
            Err(GhError::Failed(m)) => {
                let lower = m.to_ascii_lowercase();
                if lower.contains("pending review") || lower.contains("already have a pending") {
                    return Err(ApiError::new(
                        hyper::StatusCode::CONFLICT,
                        "conflict",
                        "You already have a pending review on this pull request. Submit or discard it on GitHub, then try again.",
                    ));
                }
                return Err(ApiError::unprocessable(m));
            }
            Err(e) => return Err(e.into_api()),
        };
        let v: serde_json::Value = serde_json::from_str(&out).map_err(|_| {
            ApiError::unprocessable("GitHub did not return a review URL.".to_string())
        })?;
        let url = v
            .get("html_url")
            .and_then(|u| u.as_str())
            .ok_or_else(|| {
                ApiError::unprocessable("GitHub did not return a review URL.".to_string())
            })?
            .to_string();

        let ids: Vec<String> = review
            .notes
            .iter()
            .filter(|n| !n.sent)
            .map(|n| n.id.clone())
            .collect();
        reviews
            .mark_notes_sent(&Reviews::pull_key(id), &ids, now_ms())
            .await;
        Ok(url)
    }

    pub async fn start_session(
        &self,
        id: &str,
        reg: &Arc<Registry>,
        agent: AgentId,
        model: Option<String>,
        autonomy: bool,
    ) -> Result<syncaidync_core::Session, ApiError> {
        let _prepare = self.prepare.lock().await;
        let pull = self
            .get(id)
            .await
            .ok_or_else(|| ApiError::not_found("No pull request with that id."))?;
        if let Some(sid) = &pull.session {
            if let Some(s) = reg.get(sid).await {
                return Ok(s);
            }
        }

        let clone = PathBuf::from(&pull.clone);
        let (branch, folder) = unique_pr_worktree(&self.home, &clone, pull.number);
        if let Some(parent) = folder.parent() {
            std::fs::create_dir_all(parent).map_err(|_| ApiError::internal())?;
        }
        git::worktree_add(&clone, &branch, &folder, &pull.head_commit)
            .await
            .map_err(|e| ApiError::unprocessable(e.message))?;

        let workspace = Workspace {
            kind: WorkspaceKind::Worktree,
            path: folder.to_string_lossy().into_owned(),
            repo: Some(clone.to_string_lossy().into_owned()),
            branch: Some(branch.clone()),
            base: Some(pull.base_ref.clone()),
            base_commit: Some(pull.base_commit.clone()),
            owned: true,
        };

        let created = reg
            .create_prepared(PreparedSession {
                agent,
                model,
                autonomy,
                title: Some(format!("PR #{}: {}", pull.number, pull.title)),
                prompt: None,
                workspace,
                pull: Some(pull.id.clone()),
            })
            .await;
        let session = match created {
            Ok(session) => session,
            Err(err) => {
                if git::worktree_remove(&clone, &folder).await.is_ok() {
                    let _ = git::delete_branch(&clone, &branch).await;
                }
                return Err(CreateError::into_api(err));
            }
        };

        let mut updated = pull;
        updated.session = Some(session.id.clone());
        self.store_publish(updated).await;
        Ok(session)
    }

    pub async fn flush(&self) {
        self.saver.flush().await;
    }
}

fn unique_pr_worktree(home: &Path, repo: &Path, number: u64) -> (String, PathBuf) {
    let repo_name = repo.file_name().and_then(|s| s.to_str()).unwrap_or("repo");
    let root = home.join("worktrees").join(repo_name);
    let base_branch = format!("dync/pr-{number}");
    let base_slug = format!("pr-{number}");
    let mut branch = base_branch.clone();
    let mut folder = root.join(&base_slug);
    if !folder.exists() {
        return (branch, folder);
    }
    for n in 2..1000 {
        branch = format!("{base_branch}-{n}");
        folder = root.join(format!("{base_slug}-{n}"));
        if !folder.exists() {
            return (branch, folder);
        }
    }
    (
        format!("{base_branch}-x"),
        root.join(format!("{base_slug}-x")),
    )
}

#[derive(Debug)]
struct PrView {
    title: String,
    author: String,
    body: String,
    state: String,
    draft: bool,
    base_ref: String,
    head_ref: String,
    url: String,
}

async fn pr_view(owner: &str, repo: &str, number: u64) -> Result<PrView, ApiError> {
    let repo_arg = format!("{owner}/{repo}");
    let num = number.to_string();
    let out = gh::run(
        &[
            "pr",
            "view",
            &num,
            "--repo",
            &repo_arg,
            "--json",
            "number,title,author,body,state,isDraft,baseRefName,headRefName,url",
        ],
        None,
    )
    .await
    .map_err(GhError::into_api)?;
    let v: serde_json::Value = serde_json::from_str(&out).map_err(|_| {
        ApiError::unprocessable("The GitHub CLI returned a response that could not be read.")
    })?;
    let author = match v.get("author") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(obj) => obj
            .get("login")
            .and_then(|l| l.as_str())
            .unwrap_or("unknown")
            .to_string(),
        None => "unknown".into(),
    };
    let state_raw = v
        .get("state")
        .and_then(|s| s.as_str())
        .unwrap_or("OPEN")
        .to_ascii_lowercase();
    let state = match state_raw.as_str() {
        "merged" => "merged",
        "closed" => "closed",
        _ => "open",
    }
    .to_string();
    Ok(PrView {
        title: v
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or("(no title)")
            .to_string(),
        author,
        body: v
            .get("body")
            .and_then(|b| b.as_str())
            .unwrap_or("")
            .to_string(),
        state,
        draft: v.get("isDraft").and_then(|d| d.as_bool()).unwrap_or(false),
        base_ref: v
            .get("baseRefName")
            .and_then(|s| s.as_str())
            .unwrap_or("main")
            .to_string(),
        head_ref: v
            .get("headRefName")
            .and_then(|s| s.as_str())
            .unwrap_or("head")
            .to_string(),
        url: v
            .get("url")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
    })
}

fn find_clone(roots: &[PathBuf], home: &Path, owner: &str, repo: &str) -> Option<PathBuf> {
    let repos = discovery::list_repos(roots, home);
    for entry in repos {
        let path = PathBuf::from(&entry.path);
        if let Ok(out) = std::process::Command::new("git")
            .env_remove("SYNCAIDYNC_KEY")
            .args(["config", "--get-regexp", r"^remote\..*\.url$"])
            .current_dir(&path)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
        {
            if !out.status.success() {
                continue;
            }
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                // remote.origin.url https://...
                let url = line.split_whitespace().nth(1).unwrap_or("");
                if remote_matches(url, owner, repo) {
                    return Some(path);
                }
            }
        }
    }
    None
}

async fn fetch_pull(
    clone: &Path,
    owner: &str,
    repo: &str,
    number: u64,
    view: &PrView,
    managed: bool,
) -> Result<(String, String, String), ApiError> {
    let remote = matching_remote(clone, owner, repo).ok_or_else(|| {
        ApiError::unprocessable(format!(
            "No local clone of {owner}/{repo} was found in your code folders. Clone it, then try again."
        ))
    })?;
    let head_ref = format!("refs/syncaidync/pull/{number}/head");
    let base_ref = format!("refs/syncaidync/pull/{number}/base");
    let head_spec = format!("+refs/pull/{number}/head:{head_ref}");
    let base_spec = format!("+refs/heads/{}:{base_ref}", view.base_ref);
    for depth in ["", "--deepen=32", "--deepen=128", "--unshallow"] {
        let mut args = vec!["fetch", "--no-tags", "--no-write-fetch-head"];
        if !depth.is_empty() {
            args.push(depth);
        }
        args.extend([&*remote, &head_spec, &base_spec]);
        git::run_timeout(clone, &args, std::time::Duration::from_secs(60))
            .await
            .map_err(|e| ApiError::unprocessable(e.message))?;

        let head_commit = git::rev_parse(clone, &head_ref)
            .await
            .map_err(|e| ApiError::unprocessable(e.message))?;
        let base_tip = git::rev_parse(clone, &base_ref)
            .await
            .map_err(|e| ApiError::unprocessable(e.message))?;
        match git::run(clone, &["merge-base", &base_tip, &head_commit]).await {
            Ok(base_commit) => return Ok((remote, head_commit, base_commit)),
            Err(err) => {
                let shallow = managed
                    && depth != "--unshallow"
                    && git::run(clone, &["rev-parse", "--is-shallow-repository"])
                        .await
                        .is_ok_and(|value| value == "true");
                if !shallow {
                    return Err(ApiError::unprocessable(format!(
                        "Could not find the pull request's merge base. {}",
                        err.message
                    )));
                }
            }
        }
    }
    Err(ApiError::unprocessable(
        "Could not find the pull request's merge base.",
    ))
}

fn matching_remote(clone: &Path, owner: &str, repo: &str) -> Option<String> {
    let out = std::process::Command::new("git")
        .env_remove("SYNCAIDYNC_KEY")
        .args(["config", "--get-regexp", r"^remote\..*\.url$"])
        .current_dir(clone)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let mut parts = line.split_whitespace();
        let key = parts.next().unwrap_or("");
        let url = parts.next().unwrap_or("");
        if remote_matches(url, owner, repo) {
            // remote.origin.url → origin
            let name = key
                .strip_prefix("remote.")
                .and_then(|s| s.strip_suffix(".url"))
                .unwrap_or("origin");
            return Some(name.to_string());
        }
    }
    None
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
