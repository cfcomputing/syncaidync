//! Gather a ChangeSet for a session workspace.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use syncaidync_core::{
    build_map, groups, language_for, parse_diff, stamp, untracked_file, ChangeBase, ChangeHead,
    ChangeSet, FileDiff, MAX_FILES, MAX_IMPORTER_NODES, MAX_UNTRACKED, MAX_UNTRACKED_BYTES,
};

use crate::git;

fn cache() -> &'static Mutex<HashMap<String, (String, ChangeSet)>> {
    static CACHE: OnceLock<Mutex<HashMap<String, (String, ChangeSet)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub enum ChangeSource {
    Folder {
        root: PathBuf,
        base_commit: String,
        base_label: String,
    },
    /// Compare two commits in a repo (pull request review). No working-tree untracked files.
    Commits {
        repo: PathBuf,
        base: String,
        head: String,
        base_label: String,
        head_label: String,
    },
}

#[derive(Debug)]
pub enum ChangesError {
    Unprocessable(String),
    Internal(String),
}

impl ChangesError {
    pub fn into_api(self) -> crate::http::error::ApiError {
        match self {
            ChangesError::Unprocessable(m) => crate::http::error::ApiError::unprocessable(m),
            ChangesError::Internal(_) => crate::http::error::ApiError::internal(),
        }
    }
}

pub fn gather(source: ChangeSource) -> Result<ChangeSet, ChangesError> {
    match source {
        ChangeSource::Folder {
            root,
            base_commit,
            base_label,
        } => gather_folder(root, base_commit, base_label),
        ChangeSource::Commits {
            repo,
            base,
            head,
            base_label,
            head_label,
        } => gather_commits(repo, base, head, base_label, head_label),
    }
}

fn gather_folder(
    root: PathBuf,
    base_commit: String,
    base_label: String,
) -> Result<ChangeSet, ChangesError> {
    let inside = Command::new("git")
        .env_remove("SYNCAIDYNC_KEY")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(&root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim() == "true")
        .unwrap_or(false);
    if !inside {
        return Err(ChangesError::Unprocessable(
            "This folder is not a git repository, so there is nothing to compare against.".into(),
        ));
    }

    let raw_diff = git_diff(&root, &base_commit, None)?;
    let untracked = git_untracked(&root)?;
    let untracked_paths: Vec<String> = untracked.iter().map(|(p, _)| p.clone()).collect();
    let stamp_key = stamp(&raw_diff, &untracked_paths);
    let cache_key = format!("{}|{}|{}", root.display(), base_commit, stamp_key);

    if let Ok(guard) = cache().lock() {
        if let Some((s, cs)) = guard.get(&cache_key) {
            if s == &stamp_key {
                return Ok(cs.clone());
            }
        }
    }

    let all_paths = git_all_paths(&root)?;
    let mut files = parse_diff(&raw_diff);
    let mut truncated = false;

    for (path, text) in &untracked {
        if files.iter().any(|f| &f.path == path) {
            continue;
        }
        if files.len() >= MAX_FILES {
            truncated = true;
            break;
        }
        files.push(untracked_file(path, text));
    }
    if files.len() > MAX_FILES {
        files.truncate(MAX_FILES);
        truncated = true;
    }

    let group_list = groups(&mut files, &all_paths);
    let changed_paths: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
    let edges = find_import_edges(&root, None, &files, &all_paths);
    let map = build_map(&changed_paths, &edges, MAX_IMPORTER_NODES);

    let head_commit = git_stdout(&root, &["rev-parse", "HEAD"]).unwrap_or_default();
    let head_label = git_stdout(&root, &["branch", "--show-current"]).unwrap_or_else(|_| {
        if head_commit.len() >= 7 {
            head_commit[..7].to_string()
        } else {
            head_commit.clone()
        }
    });
    let dirty = !raw_diff.is_empty() || !untracked_paths.is_empty();

    let cs = ChangeSet {
        stamp: stamp_key.clone(),
        base: ChangeBase {
            label: base_label,
            commit: base_commit,
        },
        head: ChangeHead {
            label: head_label,
            commit: head_commit,
            dirty,
        },
        truncated,
        files,
        groups: group_list,
        map,
    };
    store_cache(cache_key, stamp_key, cs.clone());
    Ok(cs)
}

fn gather_commits(
    repo: PathBuf,
    base: String,
    head: String,
    base_label: String,
    head_label: String,
) -> Result<ChangeSet, ChangesError> {
    let cache_key = format!("commits|{}|{}|{}", repo.display(), base, head);
    if let Ok(guard) = cache().lock() {
        if let Some((_, cs)) = guard.get(&cache_key) {
            if cs.head.commit == head {
                return Ok(cs.clone());
            }
        }
    }

    let raw_diff = git_diff(&repo, &base, Some(&head))?;
    let stamp_key = stamp(&raw_diff, &[]);
    let all_paths = git_ls_tree(&repo, &head)?;
    let mut files = parse_diff(&raw_diff);
    let mut truncated = false;
    if files.len() > MAX_FILES {
        files.truncate(MAX_FILES);
        truncated = true;
    }
    let group_list = groups(&mut files, &all_paths);
    let changed_paths: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
    let edges = find_import_edges(&repo, Some(&head), &files, &all_paths);
    let map = build_map(&changed_paths, &edges, MAX_IMPORTER_NODES);

    let cs = ChangeSet {
        stamp: stamp_key.clone(),
        base: ChangeBase {
            label: base_label,
            commit: base,
        },
        head: ChangeHead {
            label: head_label,
            commit: head.clone(),
            dirty: false,
        },
        truncated,
        files,
        groups: group_list,
        map,
    };
    store_cache(cache_key, stamp_key, cs.clone());
    Ok(cs)
}

fn store_cache(cache_key: String, stamp_key: String, cs: ChangeSet) {
    if let Ok(mut guard) = cache().lock() {
        guard.insert(cache_key, (stamp_key, cs));
        if guard.len() > 32 {
            if let Some(k) = guard.keys().next().cloned() {
                guard.remove(&k);
            }
        }
    }
}

fn git_diff(root: &Path, base: &str, head: Option<&str>) -> Result<String, ChangesError> {
    let mut args = vec![
        "-c",
        "core.quotePath=false",
        "diff",
        "--no-color",
        "--no-ext-diff",
        "-M",
        "-U5",
        base,
    ];
    if let Some(h) = head {
        args.push(h);
    }
    let out = Command::new("git")
        .env_remove("SYNCAIDYNC_KEY")
        .args(&args)
        .current_dir(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .map_err(|e| ChangesError::Internal(e.to_string()))?;
    // git diff returns 1 when differences exist
    if !out.status.success() && out.status.code() != Some(1) {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(ChangesError::Internal(err.trim().to_string()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn git_ls_tree(root: &Path, head: &str) -> Result<Vec<String>, ChangesError> {
    let out = Command::new("git")
        .env_remove("SYNCAIDYNC_KEY")
        .args(["ls-tree", "-r", "--name-only", "-z", head])
        .current_dir(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| ChangesError::Internal(e.to_string()))?;
    if !out.status.success() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    for path in out.stdout.split(|&b| b == 0) {
        if path.is_empty() {
            continue;
        }
        let rel = String::from_utf8_lossy(path).into_owned();
        if rel.split('/').any(|s| s == ".git") {
            continue;
        }
        paths.push(rel);
    }
    Ok(paths)
}

fn git_untracked(root: &Path) -> Result<Vec<(String, String)>, ChangesError> {
    let out = Command::new("git")
        .env_remove("SYNCAIDYNC_KEY")
        .args(["ls-files", "--others", "--exclude-standard", "-z"])
        .current_dir(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| ChangesError::Internal(e.to_string()))?;
    if !out.status.success() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for path in out.stdout.split(|&b| b == 0) {
        if path.is_empty() {
            continue;
        }
        let rel = String::from_utf8_lossy(path).into_owned();
        if rel.split('/').any(|s| s == ".git") {
            continue;
        }
        if result.len() >= MAX_UNTRACKED {
            break;
        }
        let full = root.join(&rel);
        let meta = match std::fs::metadata(&full) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !meta.is_file() || meta.len() > MAX_UNTRACKED_BYTES {
            continue;
        }
        let bytes = match std::fs::read(&full) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        result.push((rel, text));
    }
    Ok(result)
}

fn git_all_paths(root: &Path) -> Result<Vec<String>, ChangesError> {
    let out = Command::new("git")
        .env_remove("SYNCAIDYNC_KEY")
        .args(["ls-files", "-co", "--exclude-standard", "-z"])
        .current_dir(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| ChangesError::Internal(e.to_string()))?;
    if !out.status.success() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    for path in out.stdout.split(|&b| b == 0) {
        if path.is_empty() {
            continue;
        }
        let rel = String::from_utf8_lossy(path).into_owned();
        if rel.split('/').any(|s| s == ".git") {
            continue;
        }
        paths.push(rel);
    }
    Ok(paths)
}

fn git_stdout(root: &Path, args: &[&str]) -> Result<String, ChangesError> {
    let out = Command::new("git")
        .env_remove("SYNCAIDYNC_KEY")
        .args(args)
        .current_dir(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| ChangesError::Internal(e.to_string()))?;
    if !out.status.success() {
        return Err(ChangesError::Internal(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn find_import_edges(
    root: &Path,
    at_commit: Option<&str>,
    files: &[FileDiff],
    all_paths: &[String],
) -> Vec<(String, String)> {
    let changed: HashSet<&str> = files.iter().map(|f| f.path.as_str()).collect();
    let mut stems: Vec<(&str, &str)> = files
        .iter()
        .filter_map(|f| {
            let stem = Path::new(&f.path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            if stem.len() < 3 {
                return None;
            }
            Some((f.path.as_str(), stem))
        })
        .collect();
    stems.sort_by_key(|(_, s)| *s);
    stems.dedup_by_key(|(_, s)| *s);

    let mut candidates: HashSet<String> = HashSet::new();
    for (_, stem) in &stems {
        if candidates.len() >= 400 {
            break;
        }
        let mut args = vec!["grep", "-l", "-F", "-e", stem];
        if let Some(rev) = at_commit {
            args.push(rev);
        }
        let out = Command::new("git")
            .env_remove("SYNCAIDYNC_KEY")
            .args(&args)
            .current_dir(root)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output();
        if let Ok(out) = out {
            if out.status.success() {
                for line in String::from_utf8_lossy(&out.stdout).lines() {
                    if candidates.len() >= 400 {
                        break;
                    }
                    let path = if let Some(rev) = at_commit {
                        line.strip_prefix(&format!("{rev}:")).unwrap_or(line)
                    } else {
                        line
                    };
                    if !path.is_empty() {
                        candidates.insert(path.to_string());
                    }
                }
            }
        }
    }

    let mut edges = Vec::new();
    for cand in candidates {
        let text = match at_commit {
            Some(rev) => match git_show(root, rev, &cand) {
                Ok(t) => t,
                Err(_) => continue,
            },
            None => {
                let full = root.join(&cand);
                let meta = match std::fs::metadata(&full) {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                if !meta.is_file() || meta.len() > MAX_UNTRACKED_BYTES {
                    continue;
                }
                let bytes = match std::fs::read(&full) {
                    Ok(b) => b,
                    Err(_) => continue,
                };
                match String::from_utf8(bytes) {
                    Ok(t) => t,
                    Err(_) => continue,
                }
            }
        };
        let lang = language_for(&cand);
        let specs = syncaidync_core::imports::extract(lang, &text);
        for spec in specs {
            if let Some(resolved) = syncaidync_core::imports::resolve(&cand, &spec, lang, all_paths)
            {
                if changed.contains(resolved.as_str()) {
                    edges.push((cand.clone(), resolved));
                }
            }
        }
    }
    edges
}

fn git_show(root: &Path, rev: &str, path: &str) -> Result<String, ChangesError> {
    let spec = format!("{rev}:{path}");
    let out = Command::new("git")
        .env_remove("SYNCAIDYNC_KEY")
        .args(["show", &spec])
        .current_dir(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| ChangesError::Internal(e.to_string()))?;
    if !out.status.success() {
        return Err(ChangesError::Internal(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ));
    }
    if out.stdout.len() as u64 > MAX_UNTRACKED_BYTES {
        return Err(ChangesError::Internal("file too large".into()));
    }
    String::from_utf8(out.stdout).map_err(|_| ChangesError::Internal("not utf8".into()))
}

/// Blocking wrapper used from async handlers.
pub async fn gather_async(source: ChangeSource) -> Result<ChangeSet, ChangesError> {
    tokio::task::spawn_blocking(move || gather(source))
        .await
        .map_err(|e| ChangesError::Internal(e.to_string()))?
}

pub fn clear_cache() {
    if let Ok(mut g) = cache().lock() {
        g.clear();
    }
}

// Keep git module reachable for callers that prefer async helpers elsewhere.
#[allow(dead_code)]
async fn _ensure_repo(path: &Path) -> bool {
    git::is_repo(path).await
}
