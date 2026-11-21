//! Parse git unified diffs into structured file hunks.

use serde::{Deserialize, Serialize};

use crate::changemap::{self, MAX_LINES_PER_FILE};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineKind {
    Context,
    Add,
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffLine {
    pub kind: LineKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new: Option<u32>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hunk {
    pub id: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub heading: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDiff {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    pub status: FileStatus,
    pub binary: bool,
    pub added: u32,
    pub removed: u32,
    pub group: String,
    pub language: String,
    pub digest: String,
    pub hunks: Vec<Hunk>,
}

/// Parse a `git diff` unified-diff text into file diffs.
pub fn parse(text: &str) -> Vec<FileDiff> {
    let mut files = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0usize;
    while i < lines.len() {
        if !lines[i].starts_with("diff --git ") {
            i += 1;
            continue;
        }
        let (mut old_path, mut new_path) = parse_git_header(lines[i]);
        i += 1;
        let mut status = FileStatus::Modified;
        let mut binary = false;
        let mut mode_only = false;
        let mut saw_plus_minus = false;
        let mut hunks = Vec::new();
        let mut added = 0u32;
        let mut removed = 0u32;
        let mut line_budget = MAX_LINES_PER_FILE;
        let mut truncated = false;

        while i < lines.len() && !lines[i].starts_with("diff --git ") {
            let cur = lines[i];
            if cur.starts_with("new file mode ") {
                status = FileStatus::Added;
                i += 1;
            } else if cur.starts_with("deleted file mode ") {
                status = FileStatus::Deleted;
                i += 1;
            } else if cur.starts_with("rename from ") {
                old_path = decode_git_path(cur.strip_prefix("rename from ").unwrap());
                status = FileStatus::Renamed;
                i += 1;
            } else if cur.starts_with("rename to ") {
                new_path = decode_git_path(cur.strip_prefix("rename to ").unwrap());
                status = FileStatus::Renamed;
                i += 1;
            } else if cur.starts_with("similarity index ")
                || cur.starts_with("dissimilarity index ")
                || cur.starts_with("index ")
                || cur.starts_with("old mode ")
                || cur.starts_with("new mode ")
            {
                if cur.starts_with("old mode ") || cur.starts_with("new mode ") {
                    mode_only = true;
                }
                i += 1;
            } else if cur.starts_with("Binary files ") || cur.starts_with("GIT binary patch") {
                binary = true;
                i += 1;
            } else if cur.starts_with("--- ") {
                saw_plus_minus = true;
                let raw = strip_ab_prefix(cur.strip_prefix("--- ").unwrap_or(cur));
                if raw == "/dev/null" {
                    status = FileStatus::Added;
                } else {
                    old_path = decode_git_path(raw);
                }
                i += 1;
            } else if cur.starts_with("+++ ") {
                saw_plus_minus = true;
                let raw = strip_ab_prefix(cur.strip_prefix("+++ ").unwrap_or(cur));
                if raw == "/dev/null" {
                    status = FileStatus::Deleted;
                } else {
                    new_path = decode_git_path(raw);
                }
                i += 1;
            } else if cur.starts_with("@@ ") {
                if truncated {
                    i += 1;
                    while i < lines.len()
                        && !lines[i].starts_with("diff --git ")
                        && !lines[i].starts_with("@@ ")
                    {
                        i += 1;
                    }
                    continue;
                }
                let Some(header) = parse_hunk_header(cur) else {
                    i += 1;
                    continue;
                };
                i += 1;
                let mut hunk_lines = Vec::new();
                let mut old_no = header.old_start;
                let mut new_no = header.new_start;
                while i < lines.len() {
                    let body = lines[i];
                    if body.starts_with("diff --git ")
                        || body.starts_with("@@ ")
                        || body.starts_with("Binary files ")
                        || body.starts_with("GIT binary patch")
                    {
                        break;
                    }
                    i += 1;
                    if body.starts_with('\\') {
                        continue;
                    }
                    if line_budget == 0 {
                        truncated = true;
                        break;
                    }
                    line_budget -= 1;
                    let (kind, text) = if let Some(t) = body.strip_prefix('+') {
                        added += 1;
                        (LineKind::Add, t.to_string())
                    } else if let Some(t) = body.strip_prefix('-') {
                        removed += 1;
                        (LineKind::Remove, t.to_string())
                    } else if let Some(t) = body.strip_prefix(' ') {
                        (LineKind::Context, t.to_string())
                    } else {
                        (LineKind::Context, body.to_string())
                    };
                    let (old, new) = match kind {
                        LineKind::Context => {
                            let pair = (Some(old_no), Some(new_no));
                            old_no += 1;
                            new_no += 1;
                            pair
                        }
                        LineKind::Add => {
                            let n = Some(new_no);
                            new_no += 1;
                            (None, n)
                        }
                        LineKind::Remove => {
                            let o = Some(old_no);
                            old_no += 1;
                            (o, None)
                        }
                    };
                    hunk_lines.push(DiffLine {
                        kind,
                        old,
                        new,
                        text,
                    });
                }
                hunks.push(Hunk {
                    id: String::new(),
                    old_start: header.old_start,
                    old_lines: header.old_lines,
                    new_start: header.new_start,
                    new_lines: header.new_lines,
                    heading: header.heading,
                    lines: hunk_lines,
                });
            } else {
                i += 1;
            }
        }

        let path = if status == FileStatus::Deleted {
            old_path.clone()
        } else {
            new_path.clone()
        };
        if path.is_empty() {
            continue;
        }
        // Skip empty modified with nothing interesting
        if !saw_plus_minus && !binary && !mode_only && status != FileStatus::Renamed {
            continue;
        }
        let mut file = FileDiff {
            path: path.clone(),
            old_path: if status == FileStatus::Renamed {
                Some(old_path)
            } else {
                None
            },
            status,
            binary,
            added,
            removed,
            group: String::new(),
            language: changemap::language_for(&path).to_string(),
            digest: String::new(),
            hunks,
        };
        assign_hunk_ids(&mut file);
        file.digest = changemap::file_digest(&file.hunks);
        files.push(file);
    }
    files
}

fn assign_hunk_ids(file: &mut FileDiff) {
    let mut used: Vec<String> = Vec::new();
    for hunk in &mut file.hunks {
        let base = changemap::hunk_id(&file.path, &hunk.lines);
        let mut id = base.clone();
        let mut n = 2u32;
        while used.iter().any(|u| u == &id) {
            id = format!("{base}-{n}");
            n += 1;
        }
        used.push(id.clone());
        hunk.id = id;
    }
}

struct HunkHeader {
    old_start: u32,
    old_lines: u32,
    new_start: u32,
    new_lines: u32,
    heading: String,
}

fn parse_hunk_header(line: &str) -> Option<HunkHeader> {
    let rest = line.strip_prefix("@@ ")?;
    let (ranges, heading) = match rest.find(" @@") {
        Some(idx) => (&rest[..idx], rest[idx + 3..].trim().to_string()),
        None => (rest.trim_end_matches('@').trim(), String::new()),
    };
    let mut parts = ranges.split_whitespace();
    let old = parts.next()?;
    let new = parts.next()?;
    let (old_start, old_lines) = parse_range(old, '-')?;
    let (new_start, new_lines) = parse_range(new, '+')?;
    Some(HunkHeader {
        old_start,
        old_lines,
        new_start,
        new_lines,
        heading,
    })
}

fn parse_range(s: &str, prefix: char) -> Option<(u32, u32)> {
    let s = s.strip_prefix(prefix)?;
    if let Some((a, b)) = s.split_once(',') {
        Some((a.parse().ok()?, b.parse().ok()?))
    } else {
        Some((s.parse().ok()?, 1))
    }
}

fn parse_git_header(line: &str) -> (String, String) {
    let rest = line.strip_prefix("diff --git ").unwrap_or(line);
    let parts = split_git_paths(rest);
    if parts.len() >= 2 {
        (
            strip_ab_prefix(&parts[0]).to_string(),
            strip_ab_prefix(&parts[1]).to_string(),
        )
    } else {
        (String::new(), String::new())
    }
}

fn split_git_paths(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = s.chars().peekable();
    while chars.peek().is_some() {
        while matches!(chars.peek(), Some(' ')) {
            chars.next();
        }
        if chars.peek().is_none() {
            break;
        }
        if chars.peek() == Some(&'"') {
            chars.next();
            let mut buf = String::new();
            while let Some(c) = chars.next() {
                if c == '"' {
                    break;
                }
                if c == '\\' {
                    if let Some(n) = chars.next() {
                        buf.push(n);
                    }
                } else {
                    buf.push(c);
                }
            }
            out.push(buf);
        } else {
            let mut buf = String::new();
            while let Some(&c) = chars.peek() {
                if c == ' ' {
                    break;
                }
                buf.push(c);
                chars.next();
            }
            out.push(buf);
        }
    }
    out
}

fn strip_ab_prefix(s: &str) -> &str {
    s.strip_prefix("a/")
        .or_else(|| s.strip_prefix("b/"))
        .unwrap_or(s)
}

/// Decode a git path that may be double-quoted with C-style escapes.
pub fn decode_git_path(raw: &str) -> String {
    let raw = raw.trim();
    let inner = if raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2 {
        &raw[1..raw.len() - 1]
    } else {
        return strip_ab_prefix(raw).to_string();
    };
    let mut out = Vec::new();
    let bytes = inner.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'"' => {
                    out.push(b'"');
                    i += 2;
                }
                b'\\' => {
                    out.push(b'\\');
                    i += 2;
                }
                b't' => {
                    out.push(b'\t');
                    i += 2;
                }
                b'n' => {
                    out.push(b'\n');
                    i += 2;
                }
                b'0'..=b'7' => {
                    let mut val = 0u8;
                    let mut n = 0;
                    while n < 3 && i + 1 + n < bytes.len() {
                        let d = bytes[i + 1 + n];
                        if !(b'0'..=b'7').contains(&d) {
                            break;
                        }
                        val = val * 8 + (d - b'0');
                        n += 1;
                    }
                    out.push(val);
                    i += 1 + n;
                }
                other => {
                    out.push(other);
                    i += 2;
                }
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let s = String::from_utf8_lossy(&out);
    strip_ab_prefix(&s).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_add_modify_delete() {
        let text = r#"diff --git a/new.txt b/new.txt
new file mode 100644
--- /dev/null
+++ b/new.txt
@@ -0,0 +1,2 @@
+hello
+world
diff --git a/old.txt b/old.txt
deleted file mode 100644
--- a/old.txt
+++ /dev/null
@@ -1,1 +0,0 @@
-gone
diff --git a/mod.txt b/mod.txt
--- a/mod.txt
+++ b/mod.txt
@@ -1,3 +1,3 @@
 keep
-old
+new
 keep2
"#;
        let files = parse(text);
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].status, FileStatus::Added);
        assert_eq!(files[0].path, "new.txt");
        assert_eq!(files[0].added, 2);
        assert_eq!(files[0].hunks[0].lines[0].new, Some(1));
        assert_eq!(files[1].status, FileStatus::Deleted);
        assert_eq!(files[1].path, "old.txt");
        assert_eq!(files[1].hunks[0].lines[0].old, Some(1));
        assert_eq!(files[2].status, FileStatus::Modified);
        assert_eq!(files[2].hunks[0].lines[1].kind, LineKind::Remove);
        assert_eq!(files[2].hunks[0].lines[1].old, Some(2));
        assert_eq!(files[2].hunks[0].lines[2].kind, LineKind::Add);
        assert_eq!(files[2].hunks[0].lines[2].new, Some(2));
    }

    #[test]
    fn parse_rename() {
        let text = r#"diff --git a/old name.txt b/new name.txt
similarity index 100%
rename from "old name.txt"
rename to "new name.txt"
"#;
        let files = parse(text);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].status, FileStatus::Renamed);
        assert_eq!(files[0].path, "new name.txt");
        assert_eq!(files[0].old_path.as_deref(), Some("old name.txt"));
    }

    #[test]
    fn identical_hunks_get_suffix() {
        let text = r#"diff --git a/a.txt b/a.txt
--- a/a.txt
+++ b/a.txt
@@ -1,1 +1,1 @@
-x
+y
@@ -10,1 +10,1 @@
-x
+y
"#;
        let files = parse(text);
        assert_eq!(files[0].hunks.len(), 2);
        assert_ne!(files[0].hunks[0].id, files[0].hunks[1].id);
        assert!(files[0].hunks[1].id.ends_with("-2"));
    }

    #[test]
    fn decode_quoted() {
        assert_eq!(decode_git_path(r#""my file.txt""#), "my file.txt");
        assert_eq!(decode_git_path(r#""foo\"bar""#), "foo\"bar");
    }
}
