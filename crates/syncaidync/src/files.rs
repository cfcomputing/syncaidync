//! Workspace file access with strict containment.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_MEDIA_BYTES: u64 = 2 * 1024 * 1024;
const MAX_LIST: usize = 2000;
const MAX_FIND: usize = 200;
const MAX_WALK_FILES: usize = 10_000;
const MAX_WALK_DEPTH: usize = 32;

const SKIP_NAMES: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    ".venv",
    "__pycache__",
    ".next",
    ".turbo",
];

#[derive(Debug)]
pub enum FileError {
    BadRequest(String),
    Forbidden(String),
    NotFound(String),
    Stale(String),
    Unprocessable(String),
    Internal(String),
}

impl FileError {
    pub fn into_api(self) -> crate::http::error::ApiError {
        use crate::http::error::ApiError;
        use hyper::StatusCode;
        match self {
            FileError::BadRequest(m) => ApiError::bad_request(m),
            FileError::Forbidden(m) => ApiError::new(StatusCode::FORBIDDEN, "forbidden", m),
            FileError::NotFound(m) => ApiError::not_found(m),
            FileError::Stale(m) => ApiError::new(StatusCode::CONFLICT, "stale", m),
            FileError::Unprocessable(m) => ApiError::unprocessable(m),
            FileError::Internal(m) => {
                ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", m)
            }
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DirEntry {
    pub name: String,
    pub kind: &'static str, // dir | file | link
    pub size: u64,
    pub ignored: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ListResult {
    pub path: String,
    pub entries: Vec<DirEntry>,
    pub truncated: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FindResult {
    pub paths: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReadResult {
    pub path: String,
    pub text: String,
    pub revision: String,
    pub size: u64,
}

/// Validate a client relative path (no absolute, no `..`, no `.git`) without touching the disk.
pub fn validate_rel(rel: &str) -> Result<(), FileError> {
    parse_rel(rel).map(|_| ())
}

/// Validate and resolve a client relative path inside `root`.
/// Returns the absolute canonical path of the target (parent canonicalized; last segment joined).
pub fn resolve(root: &Path, rel: &str) -> Result<PathBuf, FileError> {
    let segments = parse_rel(rel)?;
    let root = fs::canonicalize(root)
        .map_err(|_| FileError::NotFound("This session's folder is gone.".into()))?;

    if segments.is_empty() {
        return Ok(root);
    }

    let mut parent = root.clone();
    for seg in &segments[..segments.len() - 1] {
        parent.push(seg);
    }
    let parent = fs::canonicalize(&parent)
        .map_err(|_| FileError::NotFound("That path is not in this workspace.".into()))?;
    if !is_within(&root, &parent) {
        return Err(FileError::Forbidden(
            "That path is outside this session's workspace.".into(),
        ));
    }

    let mut target = parent;
    target.push(&segments[segments.len() - 1]);
    Ok(target)
}

fn parse_rel(rel: &str) -> Result<Vec<String>, FileError> {
    if rel.contains('\0') {
        return Err(FileError::BadRequest(
            "Paths must not contain NUL bytes.".into(),
        ));
    }
    // Do not use Path::components — it collapses mid-path "." on some platforms.
    if rel.starts_with('/') || Path::new(rel).is_absolute() {
        return Err(FileError::BadRequest(
            "Paths must be relative to the workspace.".into(),
        ));
    }
    if rel.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for seg in rel.split('/') {
        if seg.is_empty() {
            return Err(FileError::BadRequest(
                "Paths must not have empty segments.".into(),
            ));
        }
        if seg == "." {
            return Err(FileError::BadRequest(
                "Paths must not contain '.' segments.".into(),
            ));
        }
        if seg == ".." {
            return Err(FileError::BadRequest(
                "Paths must not contain '..' segments.".into(),
            ));
        }
        if seg.eq_ignore_ascii_case(".git") {
            return Err(FileError::BadRequest("Paths must not touch .git.".into()));
        }
        out.push(seg.to_string());
    }
    Ok(out)
}

fn is_within(root: &Path, candidate: &Path) -> bool {
    candidate
        .strip_prefix(root)
        .is_ok_and(|path| !path.iter().any(|part| part.eq_ignore_ascii_case(".git")))
}

fn is_ignored_name(name: &str) -> bool {
    SKIP_NAMES.contains(&name) || (name.starts_with('.') && name != ".")
}

pub fn list(root: &Path, rel: &str) -> Result<ListResult, FileError> {
    let dir = resolve(root, rel)?;
    let meta = fs::symlink_metadata(&dir)
        .map_err(|_| FileError::NotFound("That path is not in this workspace.".into()))?;
    if !meta.is_dir() {
        return Err(FileError::BadRequest("That path is not a folder.".into()));
    }
    // Ensure dir itself is under root (canonicalize if it's a symlink folder inside)
    let root_c = fs::canonicalize(root)
        .map_err(|_| FileError::NotFound("This session's folder is gone.".into()))?;
    let dir_c = fs::canonicalize(&dir)
        .map_err(|_| FileError::NotFound("That path is not in this workspace.".into()))?;
    if !is_within(&root_c, &dir_c) {
        return Err(FileError::Forbidden(
            "That path is outside this session's workspace.".into(),
        ));
    }

    let mut entries = Vec::new();
    let mut truncated = false;
    let rd = fs::read_dir(&dir_c).map_err(|e| FileError::Internal(e.to_string()))?;
    for item in rd {
        if entries.len() >= MAX_LIST {
            truncated = true;
            break;
        }
        let item = item.map_err(|e| FileError::Internal(e.to_string()))?;
        let name = item.file_name().to_string_lossy().into_owned();
        if name.eq_ignore_ascii_case(".git") {
            continue;
        }
        let ft = item
            .file_type()
            .map_err(|e| FileError::Internal(e.to_string()))?;
        let kind = if ft.is_symlink() {
            "link"
        } else if ft.is_dir() {
            "dir"
        } else {
            "file"
        };
        let size = if kind == "file" {
            item.metadata().map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };
        entries.push(DirEntry {
            ignored: is_ignored_name(&name),
            name,
            kind,
            size,
        });
    }
    entries.sort_by(|a, b| {
        let ak = if a.kind == "dir" { 0 } else { 1 };
        let bk = if b.kind == "dir" { 0 } else { 1 };
        ak.cmp(&bk)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(ListResult {
        path: rel.trim_matches('/').to_string(),
        entries,
        truncated,
    })
}

pub fn find(root: &Path, query: &str) -> Result<FindResult, FileError> {
    let root_c = fs::canonicalize(root)
        .map_err(|_| FileError::NotFound("This session's folder is gone.".into()))?;
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Ok(FindResult {
            paths: Vec::new(),
            truncated: false,
        });
    }

    let mut paths = Vec::new();
    let mut truncated = false;

    if root_c.join(".git").exists() {
        // git ls-files -co --exclude-standard
        let out = std::process::Command::new("git")
            .env_remove("SYNCAIDYNC_KEY")
            .args(["ls-files", "-co", "--exclude-standard"])
            .current_dir(&root_c)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .output();
        if let Ok(out) = out {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout);
                for line in text.lines() {
                    if line.is_empty() || line.split('/').any(|s| s == ".git") {
                        continue;
                    }
                    if line.to_lowercase().contains(&q) {
                        if paths.len() >= MAX_FIND {
                            truncated = true;
                            break;
                        }
                        paths.push(line.to_string());
                    }
                }
                return Ok(FindResult { paths, truncated });
            }
        }
    }

    walk_find(&root_c, &root_c, &q, 0, &mut paths, &mut truncated, &mut 0)?;
    Ok(FindResult { paths, truncated })
}

fn walk_find(
    root: &Path,
    dir: &Path,
    q: &str,
    depth: usize,
    paths: &mut Vec<String>,
    truncated: &mut bool,
    seen: &mut usize,
) -> Result<(), FileError> {
    if depth > MAX_WALK_DEPTH || *truncated || paths.len() >= MAX_FIND {
        return Ok(());
    }
    let rd = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };
    for item in rd.flatten() {
        if *seen >= MAX_WALK_FILES || paths.len() >= MAX_FIND {
            *truncated = true;
            return Ok(());
        }
        *seen += 1;
        let name = item.file_name().to_string_lossy().into_owned();
        if name == ".git" {
            continue;
        }
        let path = item.path();
        let rel = path
            .strip_prefix(root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let ft = item.file_type().ok();
        let is_dir = ft.as_ref().map(|f| f.is_dir()).unwrap_or(false);
        let is_link = ft.as_ref().map(|f| f.is_symlink()).unwrap_or(false);
        if is_dir && !is_link {
            if is_ignored_name(&name) {
                // still searchable if user expands, but find skips ignored folders
                continue;
            }
            walk_find(root, &path, q, depth + 1, paths, truncated, seen)?;
        } else if !is_link && rel.to_lowercase().contains(q) {
            paths.push(rel);
            if paths.len() >= MAX_FIND {
                *truncated = true;
                return Ok(());
            }
        }
    }
    Ok(())
}

pub(crate) fn open_nofollow_read(path: &Path) -> Result<File, FileError> {
    let mut opts = OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    {
        opts.custom_flags(libc::O_NOFOLLOW);
    }
    opts.open(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            FileError::NotFound("That file is not in this workspace.".into())
        } else {
            // Symlink last segment often surfaces as ELOOP / other
            FileError::Forbidden("Symlinked files cannot be opened here.".into())
        }
    })
}

fn revision_of(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

pub fn read(root: &Path, rel: &str) -> Result<ReadResult, FileError> {
    let path = resolve(root, rel)?;
    // Refuse if last segment is a symlink before open — metadata
    let meta = fs::symlink_metadata(&path)
        .map_err(|_| FileError::NotFound("That file is not in this workspace.".into()))?;
    if meta.file_type().is_symlink() {
        return Err(FileError::Forbidden(
            "Symlinked files cannot be opened here.".into(),
        ));
    }
    if !meta.is_file() {
        return Err(FileError::BadRequest("That path is not a file.".into()));
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(FileError::Unprocessable(format!(
            "This file is {} bytes; SyncAiDync opens files up to 1 MiB.",
            meta.len()
        )));
    }
    let mut f = open_nofollow_read(&path)?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)
        .map_err(|e| FileError::Internal(e.to_string()))?;
    let text = String::from_utf8(buf).map_err(|_| {
        FileError::Unprocessable(format!(
            "This file is not UTF-8 text ({} bytes).",
            meta.len()
        ))
    })?;
    let revision = revision_of(text.as_bytes());
    Ok(ReadResult {
        path: rel.trim_matches('/').to_string(),
        size: text.len() as u64,
        text,
        revision,
    })
}

/// Read an image from the workspace for markdown media. Allowed types only; at most 2 MiB.
pub fn read_media(root: &Path, rel: &str) -> Result<(Vec<u8>, &'static str), FileError> {
    let content_type = media_content_type(rel).ok_or_else(|| {
        FileError::Unprocessable(
            "Only png, jpeg, gif, webp and svg images can be shown here.".into(),
        )
    })?;
    let path = resolve(root, rel)?;
    let meta = fs::symlink_metadata(&path)
        .map_err(|_| FileError::NotFound("That file is not in this workspace.".into()))?;
    if meta.file_type().is_symlink() {
        return Err(FileError::Forbidden(
            "Symlinked files cannot be opened here.".into(),
        ));
    }
    if !meta.is_file() {
        return Err(FileError::BadRequest("That path is not a file.".into()));
    }
    if meta.len() > MAX_MEDIA_BYTES {
        return Err(FileError::Unprocessable(format!(
            "This image is {} bytes; SyncAiDync shows images up to 2 MiB.",
            meta.len()
        )));
    }
    let mut f = open_nofollow_read(&path)?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)
        .map_err(|e| FileError::Internal(e.to_string()))?;
    Ok((buf, content_type))
}

fn media_content_type(rel: &str) -> Option<&'static str> {
    media_content_type_for_name(rel.rsplit('/').next().unwrap_or(rel))
}

/// Content-Type for an image file name, or `None` if the extension is not allowed.
pub fn media_content_type_for_name(name: &str) -> Option<&'static str> {
    let ext = name.rsplit('.').next()?.to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "svg" => Some("image/svg+xml"),
        _ => None,
    }
}

pub fn write(
    root: &Path,
    rel: &str,
    text: &str,
    revision: &str,
    force: bool,
) -> Result<String, FileError> {
    let path = resolve(root, rel)?;
    let parent = path.parent().ok_or_else(|| {
        FileError::BadRequest("Cannot write the workspace root as a file.".into())
    })?;
    if !parent.is_dir() {
        return Err(FileError::NotFound(
            "That folder is not in this workspace.".into(),
        ));
    }

    let exists = path.exists();
    if exists {
        let meta = fs::symlink_metadata(&path).map_err(|e| FileError::Internal(e.to_string()))?;
        if meta.file_type().is_symlink() {
            return Err(FileError::Forbidden(
                "Symlinked files cannot be written here.".into(),
            ));
        }
        if !meta.is_file() {
            return Err(FileError::BadRequest("That path is not a file.".into()));
        }
        if !force {
            let mut f = open_nofollow_read(&path)?;
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)
                .map_err(|e| FileError::Internal(e.to_string()))?;
            let current = revision_of(&buf);
            if current != revision {
                return Err(FileError::Stale(
                    "This file changed on disk since you opened it.".into(),
                ));
            }
        }
    } else if !parent.exists() {
        return Err(FileError::NotFound(
            "That folder is not in this workspace.".into(),
        ));
    }

    let mode = if exists {
        #[cfg(unix)]
        {
            fs::metadata(&path).ok().map(|m| m.permissions().mode())
        }
        #[cfg(not(unix))]
        {
            None
        }
    } else {
        None
    };

    let tmp = parent.join(format!(".dync-write-{}.tmp", crate::ids::random_hex(8)));
    {
        let mut f = File::create(&tmp).map_err(|e| FileError::Internal(e.to_string()))?;
        f.write_all(text.as_bytes())
            .map_err(|e| FileError::Internal(e.to_string()))?;
        f.sync_all()
            .map_err(|e| FileError::Internal(e.to_string()))?;
    }
    #[cfg(unix)]
    if let Some(mode) = mode {
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(mode));
    }
    fs::rename(&tmp, &path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        FileError::Internal(e.to_string())
    })?;
    Ok(revision_of(text.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn tmp() -> PathBuf {
        let p = std::env::temp_dir().join(format!("dync-files-{}", crate::ids::random_hex(6)));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn resolve_rejects_bad_paths() {
        let root = tmp();
        assert!(matches!(
            resolve(&root, "/abs"),
            Err(FileError::BadRequest(_))
        ));
        assert!(matches!(
            resolve(&root, "a/../b"),
            Err(FileError::BadRequest(_))
        ));
        assert!(matches!(
            resolve(&root, "a/./b"),
            Err(FileError::BadRequest(_))
        ));
        assert!(matches!(
            resolve(&root, "a/\0b"),
            Err(FileError::BadRequest(_))
        ));
        assert!(matches!(
            resolve(&root, "a/.git/config"),
            Err(FileError::BadRequest(_))
        ));
        assert!(matches!(
            resolve(&root, ".git"),
            Err(FileError::BadRequest(_))
        ));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_symlink_folder_outside() {
        let root = tmp();
        let outside = tmp();
        fs::write(outside.join("secret.txt"), "nope").unwrap();
        symlink(&outside, root.join("leak")).unwrap();
        let err = resolve(&root, "leak/secret.txt").unwrap_err();
        assert!(matches!(
            err,
            FileError::Forbidden(_) | FileError::NotFound(_)
        ));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn resolve_symlink_file_last_segment_open_refused() {
        let root = tmp();
        let outside = tmp();
        fs::write(outside.join("secret.txt"), "nope").unwrap();
        fs::create_dir_all(root.join("in")).unwrap();
        symlink(outside.join("secret.txt"), root.join("in/link.txt")).unwrap();
        // resolve itself may succeed (parent is inside); read must refuse
        let path = resolve(&root, "in/link.txt").unwrap();
        assert!(path.ends_with("link.txt"));
        let err = read(&root, "in/link.txt").unwrap_err();
        assert!(matches!(err, FileError::Forbidden(_)));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn resolve_blocks_git_aliases() {
        let root = tmp();
        fs::create_dir(root.join(".git")).unwrap();
        fs::write(root.join(".git/config"), "git metadata").unwrap();
        symlink(root.join(".git"), root.join("alias")).unwrap();
        assert!(matches!(
            read(&root, "alias/config"),
            Err(FileError::Forbidden(_))
        ));
        assert!(matches!(
            read(&root, ".GIT/config"),
            Err(FileError::BadRequest(_))
        ));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_symlink_folder_inside_allowed() {
        let root = tmp();
        fs::create_dir_all(root.join("real/sub")).unwrap();
        fs::write(root.join("real/sub/a.txt"), "hi").unwrap();
        symlink(root.join("real"), root.join("alias")).unwrap();
        // parent canonicalize follows alias → real, still under root
        let p = resolve(&root, "alias/sub/a.txt").unwrap();
        assert!(p.ends_with("a.txt"));
        let got = read(&root, "alias/sub/a.txt").unwrap();
        assert_eq!(got.text, "hi");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_root_that_is_symlink() {
        let real = tmp();
        fs::write(real.join("f.txt"), "x").unwrap();
        let link =
            std::env::temp_dir().join(format!("dync-rootlink-{}", crate::ids::random_hex(4)));
        symlink(&real, &link).unwrap();
        let p = resolve(&link, "f.txt").unwrap();
        assert!(p.ends_with("f.txt"));
        let _ = fs::remove_dir_all(&real);
        let _ = fs::remove_file(&link);
    }

    #[test]
    fn empty_segments() {
        let root = tmp();
        let p = resolve(&root, "").unwrap();
        assert_eq!(p, fs::canonicalize(&root).unwrap());
        assert!(matches!(
            resolve(&root, "a//b"),
            Err(FileError::BadRequest(_))
        ));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn read_media_happy_and_rejects() {
        let root = tmp();
        fs::write(root.join("ok.png"), b"\x89PNG").unwrap();
        fs::write(root.join("nope.txt"), b"hi").unwrap();
        let (bytes, ct) = read_media(&root, "ok.png").unwrap();
        assert_eq!(ct, "image/png");
        assert_eq!(bytes, b"\x89PNG");
        assert!(matches!(
            read_media(&root, "nope.txt"),
            Err(FileError::Unprocessable(_))
        ));
        assert!(matches!(
            read_media(&root, "a/.git/x.png"),
            Err(FileError::BadRequest(_))
        ));
        let outside = tmp();
        fs::write(outside.join("secret.png"), b"x").unwrap();
        symlink(outside.join("secret.png"), root.join("leak.png")).unwrap();
        assert!(matches!(
            read_media(&root, "leak.png"),
            Err(FileError::Forbidden(_))
        ));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }
}
