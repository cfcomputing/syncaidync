//! Pull request references and draft-review payloads (pure).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::diff::{FileDiff, LineKind};
use crate::review::Review;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRef {
    pub owner: String,
    pub repo: String,
    pub number: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullProgress {
    pub files: u32,
    pub reviewed: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pull {
    pub id: String,
    pub owner: String,
    pub repo: String,
    pub number: u64,
    pub url: String,
    pub title: String,
    pub author: String,
    pub body_html: String,
    pub state: String,
    pub draft: bool,
    pub base_ref: String,
    pub head_ref: String,
    pub base_commit: String,
    pub head_commit: String,
    pub clone: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    pub added_at: u64,
    pub fetched_at: u64,
    pub progress: PullProgress,
}

const ACCEPT: &str =
    "Paste a GitHub pull request URL (github.com/owner/repo/pull/123) or owner/repo#123.";

/// Parse a PR URL or `owner/repo#n`.
pub fn parse_ref(text: &str) -> Result<PullRef, String> {
    let raw = text.trim();
    if raw.is_empty() {
        return Err(ACCEPT.into());
    }

    if !raw.contains("://") {
        if let Some((owner_repo, num)) = raw.split_once('#') {
            let (owner, repo) = split_owner_repo(owner_repo.trim())?;
            let number = parse_number(num.trim())?;
            if num.trim().contains('#') {
                return Err(ACCEPT.into());
            }
            return Ok(PullRef {
                owner,
                repo,
                number,
            });
        }
        return Err(ACCEPT.into());
    }

    let (scheme, after) = raw.split_once("://").ok_or_else(|| ACCEPT.to_string())?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return Err(ACCEPT.into());
    }
    let after = if after.len() >= 4 && after[..4].eq_ignore_ascii_case("www.") {
        &after[4..]
    } else {
        after
    };
    let (host, path) = after.split_once('/').ok_or_else(|| ACCEPT.to_string())?;
    if !host.eq_ignore_ascii_case("github.com") {
        return Err(ACCEPT.into());
    }
    let path_only = path
        .split(['?', '#'])
        .next()
        .unwrap_or(path)
        .trim_end_matches('/');
    let parts: Vec<&str> = path_only.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() < 4 || !parts[2].eq_ignore_ascii_case("pull") {
        return Err(ACCEPT.into());
    }
    let owner = parts[0];
    let repo = parts[1];
    if !valid_name(owner) || !valid_name(repo) {
        return Err(ACCEPT.into());
    }
    let number = parse_number(parts[3])?;
    Ok(PullRef {
        owner: owner.to_string(),
        repo: repo.to_string(),
        number,
    })
}

fn split_owner_repo(s: &str) -> Result<(String, String), String> {
    let mut parts = s.split('/');
    let owner = parts.next().unwrap_or("");
    let repo = parts.next().unwrap_or("");
    if parts.next().is_some() || owner.is_empty() || repo.is_empty() {
        return Err(ACCEPT.into());
    }
    if !valid_name(owner) || !valid_name(repo) {
        return Err(ACCEPT.into());
    }
    Ok((owner.to_string(), repo.to_string()))
}

fn parse_number(s: &str) -> Result<u64, String> {
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_digit()) {
        return Err(ACCEPT.into());
    }
    s.parse().map_err(|_| ACCEPT.to_string())
}

fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// Whether a git remote URL points at github.com/owner/repo.
pub fn remote_matches(url: &str, owner: &str, repo: &str) -> bool {
    let u = url.trim();
    let lower = u.to_ascii_lowercase();
    let owner_l = owner.to_ascii_lowercase();
    let repo_l = repo.to_ascii_lowercase();
    let repo_git = format!("{repo_l}.git");

    let path = if let Some(rest) = lower.strip_prefix("https://github.com/") {
        rest
    } else if let Some(rest) = lower.strip_prefix("http://github.com/") {
        rest
    } else if let Some(rest) = lower.strip_prefix("git@github.com:") {
        rest
    } else if let Some(rest) = lower.strip_prefix("ssh://git@github.com/") {
        rest
    } else {
        return false;
    };
    let path = path
        .trim_end_matches('/')
        .split(['?', '#'])
        .next()
        .unwrap_or(path);
    let mut parts = path.split('/');
    let o = parts.next().unwrap_or("");
    let r = parts.next().unwrap_or("");
    if parts.next().is_some() {
        return false;
    }
    o == owner_l && (r == repo_l || r == repo_git)
}

/// Body for `POST .../pulls/{n}/reviews` with no `event` (pending).
pub fn draft_payload(
    head_commit: &str,
    review: &Review,
    change_set: &crate::changemap::ChangeSet,
) -> Result<Value, String> {
    let mut body_parts: Vec<String> = Vec::new();
    if !review.overall.trim().is_empty() {
        body_parts.push(review.overall.trim().to_string());
    }

    let mut comments = Vec::new();
    let mut any = !body_parts.is_empty();

    for note in &review.notes {
        if note.sent {
            continue;
        }
        any = true;
        let file = change_set.files.iter().find(|f| f.path == note.path);
        let line_ok = match (note.start, note.end, file) {
            (Some(start), Some(end), Some(file)) if start > 0 && end >= start => {
                (start..=end).all(|line| line_in_file(file, &note.side, line))
            }
            (Some(line), None, Some(file)) | (None, Some(line), Some(file)) => {
                line_in_file(file, &note.side, line)
            }
            _ => false,
        };

        if line_ok {
            let (start, end) = match (note.start, note.end) {
                (Some(a), Some(b)) => (a.min(b), a.max(b)),
                (Some(a), None) | (None, Some(a)) => (a, a),
                _ => unreachable!(),
            };
            let gh_side = if note.side == "old" { "LEFT" } else { "RIGHT" };
            let mut c = json!({
                "path": note.path,
                "line": end,
                "side": gh_side,
                "body": note.text,
            });
            if start != end {
                c["start_line"] = json!(start);
                c["start_side"] = json!(gh_side);
            }
            comments.push(c);
        } else {
            let label = if note.path.is_empty() {
                "Note".to_string()
            } else {
                format!("### {}", note.path)
            };
            body_parts.push(format!("{label}\n\n{}", note.text.trim()));
        }
    }

    if !any {
        return Err("There is nothing to send as a draft review.".into());
    }

    Ok(json!({
        "commit_id": head_commit,
        "body": body_parts.join("\n\n"),
        "comments": comments,
    }))
}

fn line_in_file(file: &FileDiff, side: &str, line: u32) -> bool {
    let want_old = side == "old";
    for hunk in &file.hunks {
        for dl in &hunk.lines {
            let n = if want_old { dl.old } else { dl.new };
            // Context lines exist on both sides; remove only old; add only new.
            match dl.kind {
                LineKind::Context if n == Some(line) => return true,
                LineKind::Remove if want_old && dl.old == Some(line) => return true,
                LineKind::Add if !want_old && dl.new == Some(line) => return true,
                _ => {}
            }
        }
    }
    false
}

/// Recompute progress from the review's reviewed map and current files.
pub fn progress_for(review: &Review, files: &[FileDiff]) -> PullProgress {
    let mut reviewed = 0u32;
    for f in files {
        if review
            .reviewed
            .get(&f.path)
            .map(|d| d == &f.digest)
            .unwrap_or(false)
        {
            reviewed += 1;
        }
    }
    PullProgress {
        files: files.len() as u32,
        reviewed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::changemap::{ChangeBase, ChangeHead, ChangeMap, ChangeSet};
    use crate::diff::{DiffLine, FileStatus, Hunk};

    fn sample_cs() -> ChangeSet {
        ChangeSet {
            stamp: "s".into(),
            base: ChangeBase {
                label: "main".into(),
                commit: "aaa".into(),
            },
            head: ChangeHead {
                label: "head".into(),
                commit: "bbb".into(),
                dirty: false,
            },
            truncated: false,
            files: vec![FileDiff {
                path: "src/a.rs".into(),
                old_path: None,
                status: FileStatus::Modified,
                binary: false,
                added: 1,
                removed: 1,
                group: String::new(),
                language: "rust".into(),
                digest: "d1".into(),
                hunks: vec![Hunk {
                    id: "h1".into(),
                    old_start: 10,
                    old_lines: 2,
                    new_start: 10,
                    new_lines: 2,
                    heading: String::new(),
                    lines: vec![
                        DiffLine {
                            kind: LineKind::Remove,
                            old: Some(10),
                            new: None,
                            text: "old".into(),
                        },
                        DiffLine {
                            kind: LineKind::Add,
                            old: None,
                            new: Some(10),
                            text: "new".into(),
                        },
                        DiffLine {
                            kind: LineKind::Context,
                            old: Some(11),
                            new: Some(11),
                            text: "ctx".into(),
                        },
                    ],
                }],
            }],
            groups: vec![],
            map: ChangeMap {
                nodes: vec![],
                edges: vec![],
            },
        }
    }

    #[test]
    fn parse_ref_forms() {
        let a = parse_ref("https://github.com/Acme/shop/pull/12").unwrap();
        assert_eq!(a.owner, "Acme");
        assert_eq!(a.repo, "shop");
        assert_eq!(a.number, 12);

        let b = parse_ref("http://www.github.com/o/r/pull/3/files?x=1#frag").unwrap();
        assert_eq!((b.owner.as_str(), b.repo.as_str(), b.number), ("o", "r", 3));

        let c = parse_ref("cfcomputing/syncaidync#99").unwrap();
        assert_eq!(c.number, 99);

        assert!(parse_ref("https://gitlab.com/o/r/pull/1").is_err());
        assert!(parse_ref("o/r").is_err());
        assert!(parse_ref("o/r#").is_err());
        assert!(parse_ref("o/r#1#2").is_err());
        assert!(parse_ref("o/r#abc").is_err());
        assert!(parse_ref("o/r/extra#1").is_err());
        assert!(parse_ref("https://github.com/o/r/issues/1").is_err());
        assert!(parse_ref("o/r#1;rm -rf").is_err());
    }

    #[test]
    fn remote_matches_variants() {
        assert!(remote_matches(
            "https://github.com/Acme/Shop.git",
            "acme",
            "shop"
        ));
        assert!(remote_matches("git@github.com:acme/shop", "Acme", "Shop"));
        assert!(remote_matches(
            "ssh://git@github.com/acme/shop.git",
            "acme",
            "shop"
        ));
        assert!(!remote_matches(
            "https://github.com/acme/other.git",
            "acme",
            "shop"
        ));
        assert!(!remote_matches(
            "https://gitlab.com/acme/shop.git",
            "acme",
            "shop"
        ));
    }

    #[test]
    fn draft_payload_shapes() {
        let cs = sample_cs();
        let mut review = Review::empty(1);
        review.overall = "Overall note".into();
        review.notes.push(crate::review::ReviewNote {
            id: "n1".into(),
            path: "src/a.rs".into(),
            side: "new".into(),
            start: Some(10),
            end: Some(10),
            quote: String::new(),
            text: "line note".into(),
            sent: false,
            created_at: 1,
        });
        review.notes.push(crate::review::ReviewNote {
            id: "n2".into(),
            path: "src/a.rs".into(),
            side: "old".into(),
            start: Some(10),
            end: Some(11),
            quote: String::new(),
            text: "range".into(),
            sent: false,
            created_at: 1,
        });
        review.notes.push(crate::review::ReviewNote {
            id: "n3".into(),
            path: "src/a.rs".into(),
            side: "new".into(),
            start: None,
            end: None,
            quote: String::new(),
            text: "file level".into(),
            sent: false,
            created_at: 1,
        });
        review.notes.push(crate::review::ReviewNote {
            id: "n4".into(),
            path: "src/a.rs".into(),
            side: "new".into(),
            start: Some(99),
            end: Some(99),
            quote: String::new(),
            text: "outdated".into(),
            sent: false,
            created_at: 1,
        });

        let payload = draft_payload("bbb", &review, &cs).unwrap();
        assert!(payload.get("event").is_none());
        assert_eq!(payload["commit_id"], "bbb");
        let body = payload["body"].as_str().unwrap();
        assert!(body.contains("Overall note"));
        assert!(body.contains("### src/a.rs"));
        assert!(body.contains("file level"));
        assert!(body.contains("outdated"));
        let comments = payload["comments"].as_array().unwrap();
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0]["side"], "RIGHT");
        assert_eq!(comments[0]["line"], 10);
        assert!(comments[0].get("start_line").is_none());
        assert_eq!(comments[1]["side"], "LEFT");
        assert_eq!(comments[1]["line"], 11);
        assert_eq!(comments[1]["start_line"], 10);
        assert_eq!(comments[1]["start_side"], "LEFT");

        let empty = Review::empty(1);
        assert!(draft_payload("bbb", &empty, &cs).is_err());
    }
}
