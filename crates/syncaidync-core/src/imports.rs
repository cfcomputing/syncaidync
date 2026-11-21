//! Best-effort import extraction and path resolution.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Extract raw import/require/mod specifiers from source.
pub fn extract(language: &str, source: &str) -> Vec<String> {
    match language {
        "javascript" | "typescript" => extract_js(source),
        "rust" => extract_rust(source),
        "python" => extract_python(source),
        _ => Vec::new(),
    }
}

/// Resolve a specifier to a path present in `all_paths`, if possible.
pub fn resolve(
    importer_path: &str,
    specifier: &str,
    language: &str,
    all_paths: &[String],
) -> Option<String> {
    let set: HashSet<&str> = all_paths.iter().map(|s| s.as_str()).collect();
    match language {
        "javascript" | "typescript" => resolve_js(importer_path, specifier, &set),
        "rust" => resolve_rust(importer_path, specifier, all_paths, &set),
        "python" => resolve_python(importer_path, specifier, all_paths, &set),
        _ => None,
    }
}

fn skip_noise_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("//") || t.starts_with('#') || t.starts_with('*')
}

fn extract_js(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in source.lines() {
        if skip_noise_line(line) {
            continue;
        }
        // import … from '…' / "…"
        if let Some(s) = find_from_string(line, "from") {
            out.push(s);
            continue;
        }
        // export … from '…'
        if line.contains("export") {
            if let Some(s) = find_from_string(line, "from") {
                out.push(s);
                continue;
            }
        }
        // import '…'
        if let Some(rest) = line.trim_start().strip_prefix("import") {
            let rest = rest.trim_start();
            if rest.starts_with('\'') || rest.starts_with('"') {
                if let Some(s) = quoted_string(rest) {
                    out.push(s);
                    continue;
                }
            }
        }
        // require('…') / import('…')
        for marker in ["require(", "import("] {
            if let Some(i) = line.find(marker) {
                let rest = &line[i + marker.len()..];
                let rest = rest.trim_start();
                if let Some(s) = quoted_string(rest) {
                    out.push(s);
                }
            }
        }
    }
    out
}

fn find_from_string(line: &str, keyword: &str) -> Option<String> {
    let i = line.find(keyword)?;
    let rest = line[i + keyword.len()..].trim_start();
    quoted_string(rest)
}

fn quoted_string(s: &str) -> Option<String> {
    let s = s.trim_start();
    let quote = s.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let mut out = String::new();
    let mut chars = s.chars();
    chars.next();
    for c in chars {
        if c == quote {
            return Some(out);
        }
        if c == '\\' {
            continue;
        }
        out.push(c);
    }
    None
}

fn extract_rust(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in source.lines() {
        if skip_noise_line(line) {
            continue;
        }
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("mod ") {
            let name = rest.trim().trim_end_matches(';').trim();
            if !name.is_empty() && !name.contains(' ') && !name.contains('{') {
                out.push(format!("mod:{name}"));
            }
        }
        if let Some(rest) = t.strip_prefix("use crate::") {
            let path = rest
                .trim()
                .trim_end_matches(';')
                .split(" as ")
                .next()
                .unwrap_or("")
                .trim();
            if !path.is_empty() {
                out.push(format!("crate:{path}"));
            }
        }
    }
    out
}

fn extract_python(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in source.lines() {
        if skip_noise_line(line) {
            continue;
        }
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("from ") {
            let mut parts = rest.split_whitespace();
            let module = parts.next().unwrap_or("");
            if parts.next() == Some("import") && !module.is_empty() {
                out.push(module.to_string());
            }
        } else if let Some(rest) = t.strip_prefix("import ") {
            let module = rest.split(',').next().unwrap_or(rest).trim();
            let module = module.split_whitespace().next().unwrap_or(module);
            if !module.is_empty() {
                out.push(module.to_string());
            }
        }
    }
    out
}

fn resolve_js(importer: &str, specifier: &str, set: &HashSet<&str>) -> Option<String> {
    if !(specifier.starts_with("./") || specifier.starts_with("../")) {
        return None;
    }
    let base = parent_dir(importer);
    let joined = join_rel(&base, specifier)?;
    let candidates = [
        joined.clone(),
        format!("{joined}.ts"),
        format!("{joined}.tsx"),
        format!("{joined}.js"),
        format!("{joined}.jsx"),
        format!("{joined}.mjs"),
        format!("{joined}.cjs"),
        format!("{joined}.json"),
        format!("{joined}/index.ts"),
        format!("{joined}/index.tsx"),
        format!("{joined}/index.js"),
        format!("{joined}/index.jsx"),
        format!("{joined}/index.mjs"),
        format!("{joined}/index.cjs"),
        format!("{joined}/index.json"),
    ];
    for c in &candidates {
        if set.contains(c.as_str()) {
            return Some(c.clone());
        }
    }
    // .js specifier also tries .ts / .tsx in its place
    if let Some(stem) = joined.strip_suffix(".js") {
        for ext in [".ts", ".tsx"] {
            let c = format!("{stem}{ext}");
            if set.contains(c.as_str()) {
                return Some(c);
            }
        }
    }
    None
}

fn resolve_rust(
    importer: &str,
    specifier: &str,
    all_paths: &[String],
    set: &HashSet<&str>,
) -> Option<String> {
    if let Some(name) = specifier.strip_prefix("mod:") {
        let parent = parent_dir(importer);
        let file_name = Path::new(importer)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let beside = [
            format!("{parent}{name}.rs"),
            format!("{parent}{name}/mod.rs"),
        ];
        let under_stem = {
            let stem = file_name.trim_end_matches(".rs");
            if matches!(stem, "lib" | "main" | "mod") {
                Vec::new()
            } else {
                vec![
                    format!("{parent}{stem}/{name}.rs"),
                    format!("{parent}{stem}/{name}/mod.rs"),
                ]
            }
        };
        for c in beside.iter().chain(under_stem.iter()) {
            let norm = trim_slashes(c);
            if set.contains(norm.as_str()) {
                return Some(norm);
            }
        }
        return None;
    }
    if let Some(path) = specifier.strip_prefix("crate:") {
        let src_root = nearest_cargo_src(importer, all_paths)?;
        let segs: Vec<&str> = path.split("::").filter(|s| !s.is_empty()).collect();
        if segs.is_empty() {
            return None;
        }
        // Longest of a/b.rs, a/b/mod.rs, a.rs, a/mod.rs
        let mut candidates = Vec::new();
        for take in (1..=segs.len()).rev() {
            let sub = segs[..take].join("/");
            candidates.push(format!("{src_root}/{sub}.rs"));
            candidates.push(format!("{src_root}/{sub}/mod.rs"));
        }
        for c in candidates {
            if set.contains(c.as_str()) {
                return Some(c);
            }
        }
    }
    None
}

fn nearest_cargo_src(importer: &str, all_paths: &[String]) -> Option<String> {
    let mut dir = parent_dir(importer);
    loop {
        let cargo = if dir.is_empty() {
            "Cargo.toml".to_string()
        } else {
            format!("{}Cargo.toml", dir)
        };
        if all_paths.iter().any(|p| p == &cargo) {
            return Some(if dir.is_empty() {
                "src".into()
            } else {
                format!("{dir}src")
            });
        }
        if dir.is_empty() {
            return None;
        }
        // pop one segment
        let trimmed = dir.trim_end_matches('/');
        if let Some(i) = trimmed.rfind('/') {
            dir = format!("{}/", &trimmed[..i]);
        } else {
            dir = String::new();
        }
    }
}

fn resolve_python(
    importer: &str,
    specifier: &str,
    all_paths: &[String],
    set: &HashSet<&str>,
) -> Option<String> {
    if specifier.starts_with('.') {
        // relative: from .x / from ..x
        let mut dots = 0usize;
        for c in specifier.chars() {
            if c == '.' {
                dots += 1;
            } else {
                break;
            }
        }
        let rest = &specifier[dots..];
        let mut dir = PathBuf::from(parent_dir(importer).trim_end_matches('/'));
        for _ in 1..dots {
            if !dir.pop() {
                break;
            }
        }
        let base = if rest.is_empty() {
            dir
        } else {
            dir.join(rest.replace('.', "/"))
        };
        return python_module_paths(&base.to_string_lossy(), set);
    }
    // absolute-ish from repo root, nearest pyproject, and its src/
    let roots = python_roots(importer, all_paths);
    let mod_path = specifier.replace('.', "/");
    for root in roots {
        let base = if root.is_empty() {
            mod_path.clone()
        } else {
            format!("{root}/{mod_path}")
        };
        if let Some(p) = python_module_paths(&base, set) {
            return Some(p);
        }
    }
    None
}

fn python_module_paths(base: &str, set: &HashSet<&str>) -> Option<String> {
    let base = base.trim_start_matches('/');
    let candidates = [format!("{base}.py"), format!("{base}/__init__.py")];
    candidates.into_iter().find(|c| set.contains(c.as_str()))
}

fn python_roots(importer: &str, all_paths: &[String]) -> Vec<String> {
    let mut roots = vec![String::new()];
    let mut dir = parent_dir(importer);
    loop {
        let py = if dir.is_empty() {
            "pyproject.toml".to_string()
        } else {
            format!("{}pyproject.toml", dir)
        };
        if all_paths.iter().any(|p| p == &py) {
            let folder = dir.trim_end_matches('/').to_string();
            roots.push(folder.clone());
            roots.push(if folder.is_empty() {
                "src".into()
            } else {
                format!("{folder}/src")
            });
            break;
        }
        if dir.is_empty() {
            break;
        }
        let trimmed = dir.trim_end_matches('/');
        if let Some(i) = trimmed.rfind('/') {
            dir = format!("{}/", &trimmed[..i]);
        } else {
            dir = String::new();
        }
    }
    roots
}

fn parent_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(i) => path[..=i].to_string(),
        None => String::new(),
    }
}

fn join_rel(base: &str, spec: &str) -> Option<String> {
    let mut parts: Vec<&str> = if base.is_empty() {
        Vec::new()
    } else {
        base.trim_end_matches('/').split('/').collect()
    };
    for seg in spec.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if parts.is_empty() {
                    return None;
                }
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

fn trim_slashes(s: &str) -> String {
    s.trim_start_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_extract_and_resolve() {
        let src = r#"
import { a } from './util';
import "./side";
const x = require('../lib/x');
// import { fake } from './nope';
const s = "import './not-real'";
"#;
        let specs = extract("javascript", src);
        assert!(specs.contains(&"./util".into()));
        assert!(specs.contains(&"./side".into()));
        assert!(specs.contains(&"../lib/x".into()));
        assert!(!specs.iter().any(|s| s.contains("nope")));
        assert!(!specs.iter().any(|s| s.contains("not-real")));

        let all = vec![
            "src/app.ts".into(),
            "src/util.ts".into(),
            "src/side.js".into(),
            "lib/x.ts".into(),
        ];
        assert_eq!(
            resolve("src/app.ts", "./util", "typescript", &all).as_deref(),
            Some("src/util.ts")
        );
        assert_eq!(
            resolve("src/app.ts", "./side", "javascript", &all).as_deref(),
            Some("src/side.js")
        );
        assert_eq!(
            resolve("src/app.ts", "../lib/x", "typescript", &all).as_deref(),
            Some("lib/x.ts")
        );
        assert_eq!(resolve("src/app.ts", "lodash", "javascript", &all), None);
    }

    #[test]
    fn rust_mod_and_crate() {
        let src = "mod foo;\nuse crate::bar::baz;\n";
        let specs = extract("rust", src);
        assert!(specs.contains(&"mod:foo".into()));
        assert!(specs.contains(&"crate:bar::baz".into()));
        let all = vec![
            "Cargo.toml".into(),
            "src/lib.rs".into(),
            "src/foo.rs".into(),
            "src/bar/mod.rs".into(),
            "src/bar/baz.rs".into(),
        ];
        assert_eq!(
            resolve("src/lib.rs", "mod:foo", "rust", &all).as_deref(),
            Some("src/foo.rs")
        );
        assert_eq!(
            resolve("src/lib.rs", "crate:bar::baz", "rust", &all).as_deref(),
            Some("src/bar/baz.rs")
        );
    }

    #[test]
    fn python_relative() {
        let src = "from .util import x\nimport pkg.mod\n";
        let specs = extract("python", src);
        assert!(specs.contains(&".util".into()));
        assert!(specs.contains(&"pkg.mod".into()));
        let all = vec![
            "pkg/__init__.py".into(),
            "pkg/app.py".into(),
            "pkg/util.py".into(),
            "pkg/mod.py".into(),
        ];
        assert_eq!(
            resolve("pkg/app.py", ".util", "python", &all).as_deref(),
            Some("pkg/util.py")
        );
        assert_eq!(
            resolve("pkg/app.py", "pkg.mod", "python", &all).as_deref(),
            Some("pkg/mod.py")
        );
    }
}
