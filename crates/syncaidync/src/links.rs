//! Links derived from the session's GitHub origin.

use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::Path,
    time::{Duration, Instant},
};
use syncaidync_core::Session;

#[derive(Clone, Default, Serialize)]
pub struct Links {
    repo_url: Option<String>,
    branch_url: Option<String>,
    pull: Option<PullLink>,
}

#[derive(Clone, Deserialize, Serialize)]
struct PullLink {
    number: u64,
    url: String,
    state: String,
}

#[derive(Default)]
pub struct LinkCache {
    entries: tokio::sync::Mutex<HashMap<String, (Instant, Links)>>,
}

impl LinkCache {
    pub async fn get(&self, session: &Session) -> Links {
        {
            let mut entries = self.entries.lock().await;
            entries.retain(|_, (time, _)| time.elapsed().as_secs() < 60);
            if let Some((_, links)) = entries.get(&session.id) {
                return links.clone();
            }
        }
        let links = resolve(session).await.unwrap_or_default();
        self.entries
            .lock()
            .await
            .insert(session.id.clone(), (Instant::now(), links.clone()));
        links
    }
}

fn repo_url(remote: &str) -> Option<String> {
    let remote = remote.trim();
    let path = [
        "https://github.com/",
        "http://github.com/",
        "git@github.com:",
        "ssh://git@github.com/",
    ]
    .iter()
    .find_map(|prefix| remote.strip_prefix(prefix))?;
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, repo) = path.split_once('/')?;
    let valid = |s: &str| {
        !s.is_empty()
            && s != "."
            && s != ".."
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    };
    (valid(owner) && valid(repo)).then(|| format!("https://github.com/{owner}/{repo}"))
}

async fn resolve(session: &Session) -> Option<Links> {
    let path = Path::new(&session.workspace.path);
    let remote = crate::git::run(path, &["remote", "get-url", "origin"])
        .await
        .ok()?;
    let repo_url = repo_url(&remote)?;
    let mut links = Links {
        repo_url: Some(repo_url.clone()),
        ..Links::default()
    };
    let branch = match &session.workspace.branch {
        Some(branch) => branch.clone(),
        None => crate::git::current_branch(path).await.unwrap_or_default(),
    };
    if branch.is_empty() {
        return Some(links);
    }
    let encoded: String = branch
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                char::from(b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    links.branch_url = Some(format!("{repo_url}/tree/{encoded}"));
    let args = [
        "pr",
        "list",
        "--head",
        &branch,
        "--state",
        "all",
        "--json",
        "number,url,state",
        "--limit",
        "1",
    ];
    if let Ok(Ok(output)) = tokio::time::timeout(
        Duration::from_secs(5),
        crate::gh::run_in(Some(path), &args, None),
    )
    .await
    {
        links.pull = serde_json::from_str::<Vec<PullLink>>(&output)
            .ok()
            .and_then(|list| list.into_iter().next())
            .filter(|pull| pull.url == format!("{repo_url}/pull/{}", pull.number));
    }
    Some(links)
}

#[cfg(test)]
mod tests {
    use super::repo_url;

    #[test]
    fn github_origin_forms() {
        for remote in [
            "https://github.com/owner/repo.git",
            "git@github.com:owner/repo.git",
            "ssh://git@github.com/owner/repo",
        ] {
            assert_eq!(
                repo_url(remote).as_deref(),
                Some("https://github.com/owner/repo")
            );
        }
        assert_eq!(repo_url("https://github.com.example/owner/repo"), None);
        assert_eq!(repo_url("https://github.com/owner/.."), None);
    }
}
