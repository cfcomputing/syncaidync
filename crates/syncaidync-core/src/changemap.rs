//! Digests, groups, stamps, and import map helpers for ChangeSets.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::diff::{DiffLine, FileDiff, FileStatus, Hunk, LineKind};

pub const MAX_FILES: usize = 200;
pub const MAX_UNTRACKED: usize = 100;
pub const MAX_UNTRACKED_BYTES: u64 = 256 * 1024;
pub const MAX_LINES_PER_FILE: usize = 5_000;
pub const MAX_IMPORTER_NODES: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeGroup {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<String>,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapNode {
    pub path: String,
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapEdge {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeMap {
    pub nodes: Vec<MapNode>,
    pub edges: Vec<MapEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeBase {
    pub label: String,
    pub commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeHead {
    pub label: String,
    pub commit: String,
    pub dirty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeSet {
    pub stamp: String,
    pub base: ChangeBase,
    pub head: ChangeHead,
    pub truncated: bool,
    pub files: Vec<FileDiff>,
    pub groups: Vec<ChangeGroup>,
    pub map: ChangeMap,
}

/// Language id by path extension / special name.
pub fn language_for(path: &str) -> &'static str {
    let base = path.rsplit('/').next().unwrap_or(path);
    let lower = base.to_ascii_lowercase();
    match lower.as_str() {
        "dockerfile" | "makefile" | "gnumakefile" => return "shell",
        _ => {}
    }
    let ext = lower.rsplit('.').next().unwrap_or("");
    match ext {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "py" => "python",
        "go" => "go",
        "json" => "json",
        "toml" => "toml",
        "yml" | "yaml" => "yaml",
        "md" | "markdown" => "markdown",
        "html" | "htm" => "html",
        "css" | "scss" | "less" => "css",
        "sh" | "bash" | "zsh" => "shell",
        "swift" => "swift",
        _ => "text",
    }
}

pub fn hunk_id(path: &str, lines: &[DiffLine]) -> String {
    let mut h = Sha256::new();
    for line in lines {
        // Kind + text only so identical hunks at different line numbers collide and get suffixes.
        h.update(format!("{:?}|{}\n", line.kind, line.text).as_bytes());
    }
    let hex = format!("{:x}", h.finalize());
    format!("{path}#{}", &hex[..8])
}

pub fn file_digest(hunks: &[Hunk]) -> String {
    let mut h = Sha256::new();
    for hunk in hunks {
        h.update(hunk.id.as_bytes());
        h.update(b"\n");
        for line in &hunk.lines {
            h.update(format!("{:?}|{}\n", line.kind, line.text).as_bytes());
        }
    }
    format!("{:x}", h.finalize())
}

pub fn stamp(raw_diff: &str, untracked_paths: &[String]) -> String {
    let mut h = Sha256::new();
    h.update(raw_diff.as_bytes());
    h.update(b"\0");
    for p in untracked_paths {
        h.update(p.as_bytes());
        h.update(b"\n");
    }
    format!("{:x}", h.finalize())
}

/// Build a FileDiff for an untracked UTF-8 file (status added, one add hunk).
pub fn untracked_file(path: &str, text: &str) -> FileDiff {
    let lines: Vec<DiffLine> = text
        .lines()
        .enumerate()
        .map(|(i, t)| DiffLine {
            kind: LineKind::Add,
            old: None,
            new: Some((i + 1) as u32),
            text: t.to_string(),
        })
        .collect();
    let hunk = Hunk {
        id: String::new(),
        old_start: 0,
        old_lines: 0,
        new_start: 1,
        new_lines: lines.len() as u32,
        heading: String::new(),
        lines: lines.clone(),
    };
    let mut file = FileDiff {
        path: path.to_string(),
        old_path: None,
        status: FileStatus::Added,
        binary: false,
        added: lines.len() as u32,
        removed: 0,
        group: String::new(),
        language: language_for(path).to_string(),
        digest: String::new(),
        hunks: vec![hunk],
    };
    if let Some(h) = file.hunks.first_mut() {
        h.id = hunk_id(path, &h.lines);
    }
    file.digest = file_digest(&file.hunks);
    file
}

const MANIFESTS: &[(&str, &str)] = &[
    ("package.json", "package.json"),
    ("Cargo.toml", "Cargo.toml"),
    ("pyproject.toml", "pyproject.toml"),
    ("go.mod", "go.mod"),
];

/// Assign each file a group name and return the groups list.
pub fn groups(files: &mut [FileDiff], all_paths: &[String]) -> Vec<ChangeGroup> {
    let path_set: std::collections::HashSet<&str> = all_paths.iter().map(|s| s.as_str()).collect();
    let mut by_group: std::collections::BTreeMap<String, (Option<String>, Vec<String>)> =
        std::collections::BTreeMap::new();

    for file in files.iter_mut() {
        let (name, manifest) = group_for(&file.path, &path_set);
        file.group = name.clone();
        by_group
            .entry(name)
            .or_insert_with(|| (manifest, Vec::new()))
            .1
            .push(file.path.clone());
    }

    by_group
        .into_iter()
        .map(|(name, (manifest, files))| ChangeGroup {
            name,
            manifest,
            files,
        })
        .collect()
}

fn group_for(path: &str, all: &std::collections::HashSet<&str>) -> (String, Option<String>) {
    let parts: Vec<&str> = path.split('/').collect();
    // Walk ancestors looking for a manifest.
    for i in (0..parts.len().saturating_sub(1)).rev() {
        let folder = parts[..=i].join("/");
        for (manifest, _) in MANIFESTS {
            let candidate = if folder.is_empty() {
                (*manifest).to_string()
            } else {
                format!("{folder}/{manifest}")
            };
            if all.contains(candidate.as_str()) {
                return (folder, Some((*manifest).to_string()));
            }
        }
    }
    // Root-level manifests
    for (manifest, _) in MANIFESTS {
        if all.contains(*manifest) {
            return ("(root)".into(), Some((*manifest).to_string()));
        }
    }
    // No manifest: first path segment or (root)
    if parts.len() > 1 {
        (parts[0].to_string(), None)
    } else {
        ("(root)".into(), None)
    }
}

/// Build the import map. `edges` are (from, to) meaning from imports to.
pub fn build_map(
    changed: &[String],
    edges: &[(String, String)],
    importer_limit: usize,
) -> ChangeMap {
    let changed_set: std::collections::HashSet<&str> = changed.iter().map(|s| s.as_str()).collect();
    let mut edge_set = std::collections::BTreeSet::new();
    for (from, to) in edges {
        if from == to {
            continue;
        }
        if !changed_set.contains(to.as_str()) {
            continue;
        }
        edge_set.insert((from.clone(), to.clone()));
    }

    // Count how many changed files each unchanged importer touches.
    let mut importer_hits: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for (from, to) in &edge_set {
        if !changed_set.contains(from.as_str()) && changed_set.contains(to.as_str()) {
            *importer_hits.entry(from.clone()).or_default() += 1;
        }
    }
    let mut importers: Vec<(String, usize)> = importer_hits.into_iter().collect();
    importers.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    importers.truncate(importer_limit);
    let importer_set: std::collections::HashSet<String> =
        importers.into_iter().map(|(p, _)| p).collect();

    let mut nodes = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for p in changed {
        if seen.insert(p.clone()) {
            nodes.push(MapNode {
                path: p.clone(),
                changed: true,
            });
        }
    }
    for p in &importer_set {
        if seen.insert(p.clone()) {
            nodes.push(MapNode {
                path: p.clone(),
                changed: false,
            });
        }
    }

    let edges_out: Vec<MapEdge> = edge_set
        .into_iter()
        .filter(|(from, to)| seen.contains(from) && seen.contains(to))
        .map(|(from, to)| MapEdge { from, to })
        .collect();

    ChangeMap {
        nodes,
        edges: edges_out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_for_common() {
        assert_eq!(language_for("src/main.rs"), "rust");
        assert_eq!(language_for("a.tsx"), "typescript");
        assert_eq!(language_for("x.py"), "python");
        assert_eq!(language_for("Makefile"), "shell");
        assert_eq!(language_for("nope"), "text");
    }

    #[test]
    fn groups_nested_and_none() {
        let all = vec![
            "packages/web/package.json".into(),
            "packages/web/src/a.ts".into(),
            "README.md".into(),
        ];
        let mut files = vec![
            FileDiff {
                path: "packages/web/src/a.ts".into(),
                old_path: None,
                status: FileStatus::Modified,
                binary: false,
                added: 1,
                removed: 0,
                group: String::new(),
                language: "typescript".into(),
                digest: String::new(),
                hunks: vec![],
            },
            FileDiff {
                path: "README.md".into(),
                old_path: None,
                status: FileStatus::Modified,
                binary: false,
                added: 1,
                removed: 0,
                group: String::new(),
                language: "markdown".into(),
                digest: String::new(),
                hunks: vec![],
            },
        ];
        let g = groups(&mut files, &all);
        assert_eq!(files[0].group, "packages/web");
        assert!(g.iter().any(|x| x.name == "packages/web"));
        assert_eq!(files[1].group, "(root)");
    }

    #[test]
    fn build_map_caps_importers() {
        let changed = vec!["lib.rs".into()];
        let mut edges = Vec::new();
        for i in 0..50 {
            edges.push((format!("imp{i}.rs"), "lib.rs".into()));
        }
        let map = build_map(&changed, &edges, 40);
        let unchanged = map.nodes.iter().filter(|n| !n.changed).count();
        assert_eq!(unchanged, 40);
        assert!(map.nodes.iter().any(|n| n.changed && n.path == "lib.rs"));
    }
}
