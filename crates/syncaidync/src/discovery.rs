//! Scan the machine for git repositories.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

const CACHE_TTL: Duration = Duration::from_secs(30);
const MAX_REPOS: usize = 500;
const MAX_DEPTH: u32 = 3;

const SKIP_NAMES: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "vendor",
    "Library",
    "Applications",
];

#[derive(Debug, Clone, Serialize)]
pub struct RepoEntry {
    pub path: String,
    pub name: String,
}

struct Cache {
    at: Instant,
    roots_key: String,
    skip_worktrees: PathBuf,
    repos: Vec<RepoEntry>,
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

pub fn list_repos(roots: &[PathBuf], home: &Path) -> Vec<RepoEntry> {
    let skip_worktrees = home.join("worktrees");
    let roots_key = roots
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(":");

    {
        let guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(c) = guard.as_ref() {
            if c.roots_key == roots_key
                && c.skip_worktrees == skip_worktrees
                && c.at.elapsed() < CACHE_TTL
            {
                return c.repos.clone();
            }
        }
    }

    let repos = scan(roots, &skip_worktrees);
    if let Ok(mut guard) = CACHE.lock() {
        *guard = Some(Cache {
            at: Instant::now(),
            roots_key,
            skip_worktrees,
            repos: repos.clone(),
        });
    }
    repos
}

fn scan(roots: &[PathBuf], skip_worktrees: &Path) -> Vec<RepoEntry> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for root in roots {
        if !root.is_dir() {
            continue;
        }
        // Home itself: depth 1 only (architecture).
        let max_depth = if is_user_home(root) { 1 } else { MAX_DEPTH };
        walk(root, 0, max_depth, skip_worktrees, &mut out, &mut seen);
        if out.len() >= MAX_REPOS {
            break;
        }
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out.truncate(MAX_REPOS);
    out
}

fn is_user_home(path: &Path) -> bool {
    std::env::var_os("HOME")
        .map(|h| AsRef::<Path>::as_ref(&h) == path)
        .unwrap_or(false)
}

fn walk(
    dir: &Path,
    depth: u32,
    max_depth: u32,
    skip_worktrees: &Path,
    out: &mut Vec<RepoEntry>,
    seen: &mut std::collections::HashSet<PathBuf>,
) {
    if out.len() >= MAX_REPOS {
        return;
    }
    if let Ok(canon) = dir.canonicalize() {
        if canon.starts_with(skip_worktrees) {
            return;
        }
    }
    if is_git_repo(dir) {
        let canon = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        if seen.insert(canon.clone()) {
            let name = canon
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("repo")
                .to_string();
            out.push(RepoEntry {
                path: canon.to_string_lossy().into_owned(),
                name,
            });
        }
        return; // do not descend into a repo
    }
    if depth >= max_depth {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for ent in entries.flatten() {
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        if SKIP_NAMES.contains(&name.as_ref()) {
            continue;
        }
        let path = ent.path();
        if path.is_dir() {
            walk(&path, depth + 1, max_depth, skip_worktrees, out, seen);
            if out.len() >= MAX_REPOS {
                return;
            }
        }
    }
}

fn is_git_repo(path: &Path) -> bool {
    let git = path.join(".git");
    git.is_dir() || git.is_file()
}

const MAX_FOLDER_ENTRIES: usize = 400;

#[derive(Debug, Clone, Serialize)]
pub struct FolderEntry {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FolderListing {
    /// Absolute path being listed, or empty when showing configured roots.
    pub path: String,
    /// Parent folder to navigate up to, if any.
    pub parent: Option<String>,
    pub entries: Vec<FolderEntry>,
    pub truncated: bool,
}

/// List folders under configured roots. Empty `path` returns the roots themselves.
/// Paths outside every root are refused.
pub fn list_folders(roots: &[PathBuf], path: Option<&str>) -> Result<FolderListing, String> {
    let roots = canonicalize_roots(roots);
    if roots.is_empty() {
        return Err("No folder roots are configured.".into());
    }

    let Some(raw) = path.map(str::trim).filter(|s| !s.is_empty()) else {
        let mut entries: Vec<FolderEntry> = roots
            .iter()
            .map(|r| FolderEntry {
                name: display_name(r),
                path: r.to_string_lossy().into_owned(),
            })
            .collect();
        entries.sort_by_key(|a| a.name.to_lowercase());
        return Ok(FolderListing {
            path: String::new(),
            parent: None,
            entries,
            truncated: false,
        });
    };

    let requested = crate::workspaces::expand_tilde(raw);
    let canon = requested
        .canonicalize()
        .map_err(|_| "That folder does not exist.".to_string())?;
    if !canon.is_dir() {
        return Err("That path is not a folder.".into());
    }
    if !roots.iter().any(|r| is_within(r, &canon)) {
        return Err("That folder is outside the allowed roots.".into());
    }

    // At a configured root, "up" returns to the roots list (parent = null).
    let parent = if roots.iter().any(|r| r == &canon) {
        None
    } else {
        canon
            .parent()
            .map(PathBuf::from)
            .filter(|p| roots.iter().any(|r| is_within(r, p)))
            .map(|p| p.to_string_lossy().into_owned())
    };

    let mut entries = Vec::new();
    let mut truncated = false;
    let read = std::fs::read_dir(&canon).map_err(|e| format!("Could not read that folder: {e}"))?;
    for ent in read.flatten() {
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || SKIP_NAMES.contains(&name.as_ref()) {
            continue;
        }
        let meta = match ent.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !meta.is_dir() {
            continue;
        }
        // Skip symlink dirs that escape roots after resolve.
        let child = ent.path();
        let child_canon = match child.canonicalize() {
            Ok(c) => c,
            Err(_) => continue,
        };
        if !roots.iter().any(|r| is_within(r, &child_canon)) {
            continue;
        }
        entries.push(FolderEntry {
            name: name.into_owned(),
            path: child_canon.to_string_lossy().into_owned(),
        });
    }
    // Sort before cutting so a truncated list shows the first folders A–Z.
    entries.sort_by_key(|a| a.name.to_lowercase());
    if entries.len() > MAX_FOLDER_ENTRIES {
        entries.truncate(MAX_FOLDER_ENTRIES);
        truncated = true;
    }

    Ok(FolderListing {
        path: canon.to_string_lossy().into_owned(),
        parent,
        entries,
        truncated,
    })
}

fn canonicalize_roots(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for r in roots {
        let Ok(c) = r.canonicalize() else {
            continue;
        };
        if !c.is_dir() {
            continue;
        }
        if seen.insert(c.clone()) {
            out.push(c);
        }
    }
    out
}

fn is_within(root: &Path, candidate: &Path) -> bool {
    candidate.starts_with(root)
}

fn display_name(path: &Path) -> String {
    if let Ok(home) = std::env::var("HOME") {
        let home = PathBuf::from(home);
        if path == home {
            return "~".into();
        }
        if let Ok(rest) = path.strip_prefix(&home) {
            return format!("~/{}", rest.display());
        }
    }
    path.file_name()
        .and_then(|s| s.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_at_depth_and_skips() {
        let root = std::env::temp_dir().join(format!(
            "syncaidync-disc-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("a/b/c")).unwrap();
        std::fs::create_dir_all(root.join("a/b/c/d")).unwrap();
        std::fs::create_dir_all(root.join("node_modules/x")).unwrap();
        std::fs::create_dir_all(root.join("a/b/c/.git")).unwrap(); // depth 3 repo
        std::fs::create_dir_all(root.join("a/b/c/d/.git")).unwrap(); // depth 4 — skip
        std::fs::create_dir_all(root.join("node_modules/x/.git")).unwrap();

        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let repos = list_repos(std::slice::from_ref(&root), &home);
        let paths: Vec<_> = repos.iter().map(|r| r.path.clone()).collect();
        assert!(
            paths.iter().any(|p| p.ends_with("a/b/c")),
            "expected depth-3 repo, got {paths:?}"
        );
        assert!(
            !paths.iter().any(|p| p.ends_with("a/b/c/d")),
            "depth-4 should be skipped"
        );
        assert!(
            !paths.iter().any(|p| p.contains("node_modules")),
            "node_modules skipped"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn folder_browse_lists_roots_and_children() {
        let root = std::env::temp_dir().join(format!(
            "syncaidync-folders-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("alpha")).unwrap();
        std::fs::create_dir_all(root.join("beta")).unwrap();
        std::fs::create_dir_all(root.join("node_modules")).unwrap();
        std::fs::write(root.join("readme.txt"), "x").unwrap();

        let roots = list_folders(std::slice::from_ref(&root), None).unwrap();
        assert_eq!(roots.entries.len(), 1);
        assert!(roots.parent.is_none());

        let listing =
            list_folders(std::slice::from_ref(&root), Some(root.to_str().unwrap())).unwrap();
        let names: Vec<_> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"beta"));
        assert!(!names.contains(&"node_modules"));
        assert!(!names.iter().any(|n| n.ends_with(".txt")));
        assert!(listing.parent.is_none());

        let outside = std::env::temp_dir().join(format!(
            "syncaidync-folders-out-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&outside).unwrap();
        assert!(
            list_folders(std::slice::from_ref(&root), Some(outside.to_str().unwrap())).is_err()
        );

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }
}

#[derive(Clone, serde::Serialize)]
pub struct HistoryItem {
    pub agent: syncaidync_core::AgentId,
    pub agent_session: String,
    pub cwd: String,
    pub first_prompt: String,
    pub last_active: u64,
    pub transcript: String,
}

#[derive(Default)]
pub struct History {
    cache: tokio::sync::Mutex<Option<(std::time::Instant, Vec<HistoryItem>)>>,
}

impl History {
    pub async fn items(&self) -> Vec<HistoryItem> {
        let mut cache = self.cache.lock().await;
        if let Some((time, items)) = cache.as_ref() {
            if time.elapsed().as_secs() < 30 {
                return items.clone();
            }
        }
        let items = tokio::task::spawn_blocking(scan_history)
            .await
            .unwrap_or_default();
        *cache = Some((std::time::Instant::now(), items.clone()));
        items
    }
}

fn scan_history() -> Vec<HistoryItem> {
    let config = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| crate::workspaces::expand_tilde("~/.claude"));
    let mut items = Vec::new();
    for folder in std::fs::read_dir(config.join("projects"))
        .into_iter()
        .flatten()
        .flatten()
    {
        if !folder.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        for file in std::fs::read_dir(folder.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            if file.file_type().is_ok_and(|kind| kind.is_file()) {
                if let Some(item) = claude_history(&file.path()) {
                    items.push(item);
                }
            }
        }
    }
    items.sort_unstable_by(|a, b| {
        b.last_active
            .cmp(&a.last_active)
            .then_with(|| a.agent_session.cmp(&b.agent_session))
    });
    items
}

fn claude_history(path: &std::path::Path) -> Option<HistoryItem> {
    use std::io::{BufRead, Read};
    if path.extension()?.to_str()? != "jsonl" {
        return None;
    }
    let agent_session = path.file_stem()?.to_str()?.to_string();
    let file = std::fs::File::open(path).ok()?;
    let last_active = file
        .metadata()
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis() as u64;
    let mut cwd = None;
    let mut first_prompt = None;
    for line in std::io::BufReader::new(file.take(4 * 1024 * 1024))
        .split(b'\n')
        .take(200)
        .map_while(Result::ok)
    {
        let Ok(row) = serde_json::from_slice::<serde_json::Value>(&line) else {
            continue;
        };
        if cwd.is_none() {
            cwd = row["cwd"]
                .as_str()
                .filter(|s| std::path::Path::new(s).is_absolute())
                .map(str::to_owned);
        }
        if first_prompt.is_none() && row["type"] == "user" {
            let content = &row["message"]["content"];
            let text = content.as_str().or_else(|| {
                let blocks = content.as_array()?;
                if blocks.iter().any(|block| block["type"] == "tool_result") {
                    return None;
                }
                blocks.iter().find(|block| block["type"] == "text")?["text"].as_str()
            });
            first_prompt = text
                .map(str::trim)
                .filter(|s| !s.is_empty() && !s.starts_with('<'))
                .map(|s| s.chars().take(1000).collect());
        }
        if cwd.is_some() && first_prompt.is_some() {
            break;
        }
    }
    let cwd = cwd?;
    std::path::Path::new(&cwd).is_dir().then_some(HistoryItem {
        agent: syncaidync_core::AgentId::Claude,
        agent_session,
        cwd,
        first_prompt: first_prompt?,
        last_active,
        transcript: path.to_str()?.to_string(),
    })
}
