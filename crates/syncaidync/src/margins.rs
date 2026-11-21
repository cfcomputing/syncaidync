//! Workspace `.syncaidync/margins` sidecar: line-tied context outside shipped source.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::files::{self, FileError};
use crate::ids;

const MAX_MARGINS: usize = 500;
const MAX_TITLE: usize = 120;
const MAX_BODY: usize = 20_000;
const MAX_QUOTE: usize = 2_000;
const MAX_MEDIA_BYTES: u64 = 2 * 1024 * 1024;
const MAX_MEDIA_NAME: usize = 120;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarginIndexEntry {
    pub id: String,
    pub path: String,
    pub start: u32,
    pub end: u32,
    #[serde(default)]
    pub quote: String,
    pub title: String,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Margin {
    pub id: String,
    pub path: String,
    pub start: u32,
    pub end: u32,
    #[serde(default)]
    pub quote: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub media: Vec<String>,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct IndexFile {
    version: u32,
    margins: Vec<MarginIndexEntry>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn dync_root(workspace: &Path) -> PathBuf {
    workspace.join(".syncaidync")
}

fn margins_root(workspace: &Path) -> PathBuf {
    dync_root(workspace).join("margins")
}

fn index_path(workspace: &Path) -> PathBuf {
    margins_root(workspace).join("index.json")
}

fn index_md_path(workspace: &Path) -> PathBuf {
    margins_root(workspace).join("INDEX.md")
}

fn entry_path(workspace: &Path, id: &str) -> PathBuf {
    margins_root(workspace)
        .join("entries")
        .join(format!("{id}.json"))
}

fn media_dir(workspace: &Path, id: &str) -> PathBuf {
    margins_root(workspace).join("media").join(id)
}

fn check_directory(path: &Path, create: bool) -> Result<bool, FileError> {
    if create {
        match fs::create_dir(path) {
            Ok(()) => return Ok(true),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(FileError::Internal(err.to_string())),
        }
    }
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => Ok(true),
        Ok(_) => Err(FileError::Forbidden(
            "The workspace margin folders must be real directories.".into(),
        )),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(FileError::Internal(err.to_string())),
    }
}

/// Create `.syncaidync/` with a default gitignore and the margins folders.
fn ensure_layout(workspace: &Path) -> Result<(), FileError> {
    let root = dync_root(workspace);
    check_directory(&root, true)?;
    let margins = margins_root(workspace);
    check_directory(&margins, true)?;
    check_directory(&margins.join("entries"), true)?;
    check_directory(&margins.join("media"), true)?;
    let gi = root.join(".gitignore");
    if !gi.exists() {
        atomic_write(
            &gi,
            "# SyncAiDync local sidecar — remove lines or use git add -f to share.\n*\n!.gitignore\n".as_bytes(),
        )?;
    }
    if !index_path(workspace).exists() {
        save_index(
            workspace,
            &IndexFile {
                version: 1,
                margins: Vec::new(),
            },
        )?;
    }
    Ok(())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), FileError> {
    let parent = path
        .parent()
        .ok_or_else(|| FileError::Internal("margin path has no parent".into()))?;
    let tmp = parent.join(format!(".dync-m-{}.tmp", ids::random_hex(6)));
    {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|e| FileError::Internal(e.to_string()))?;
        f.write_all(bytes)
            .map_err(|e| FileError::Internal(e.to_string()))?;
        f.sync_all()
            .map_err(|e| FileError::Internal(e.to_string()))?;
    }
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        FileError::Internal(e.to_string())
    })?;
    Ok(())
}

fn read_text(path: &Path) -> Result<String, FileError> {
    let meta = fs::symlink_metadata(path)
        .map_err(|_| FileError::NotFound("That margin file is gone.".into()))?;
    if !meta.is_file() {
        return Err(FileError::Forbidden(
            "Only regular margin files can be opened.".into(),
        ));
    }
    let mut text = String::new();
    files::open_nofollow_read(path)?
        .read_to_string(&mut text)
        .map_err(|err| FileError::Internal(err.to_string()))?;
    Ok(text)
}

fn load_index(workspace: &Path) -> Result<IndexFile, FileError> {
    ensure_layout(workspace)?;
    let path = index_path(workspace);
    let text = read_text(&path)?;
    let mut idx: IndexFile =
        serde_json::from_str(&text).map_err(|e| FileError::Internal(e.to_string()))?;
    if idx.version == 0 {
        idx.version = 1;
    }
    Ok(idx)
}

fn save_index(workspace: &Path, idx: &IndexFile) -> Result<(), FileError> {
    let mut copy = idx.clone();
    copy.version = 1;
    copy.margins.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.start.cmp(&b.start))
            .then(a.id.cmp(&b.id))
    });
    let text =
        serde_json::to_string_pretty(&copy).map_err(|e| FileError::Internal(e.to_string()))?;
    atomic_write(&index_path(workspace), text.as_bytes())?;
    write_index_md(workspace, &copy)?;
    Ok(())
}

fn write_index_md(workspace: &Path, idx: &IndexFile) -> Result<(), FileError> {
    let mut out = String::from(
        "# SyncAiDync margins\n\n\
         Short context attached to lines of code (diagrams, notes, pictures). \
         This index is the overview — open an entry only when you need the detail.\n\n\
         | Id | File | Lines | Title |\n\
         |----|------|-------|-------|\n",
    );
    for m in &idx.margins {
        let lines = if m.start == m.end {
            m.start.to_string()
        } else {
            format!("{}–{}", m.start, m.end)
        };
        out.push_str(&format!(
            "| `{id}` | `{path}` | {lines} | {title} |\n",
            id = m.id,
            path = m.path.replace('|', "\\|"),
            title = m.title.replace('|', "\\|"),
        ));
    }
    if idx.margins.is_empty() {
        out.push_str("| — | — | — | (none yet) |\n");
    }
    out.push_str(
        "\nBodies: `.syncaidync/margins/entries/<id>.json` (field `body`). \
         Images: `.syncaidync/margins/media/<id>/`.\n",
    );
    atomic_write(&index_md_path(workspace), out.as_bytes())
}

fn validate_id(id: &str) -> Result<(), FileError> {
    if id.is_empty() || id.len() > 64 || !id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return Err(FileError::BadRequest("That margin id is not valid.".into()));
    }
    Ok(())
}

fn validate_media_name(name: &str) -> Result<(), FileError> {
    if name.is_empty() || name.len() > MAX_MEDIA_NAME {
        return Err(FileError::BadRequest(
            "That media name is not valid.".into(),
        ));
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") || name.starts_with('.') {
        return Err(FileError::BadRequest(
            "That media name is not valid.".into(),
        ));
    }
    if files::media_content_type_for_name(name).is_none() {
        return Err(FileError::Unprocessable(
            "Only png, jpeg, gif, webp and svg images can be attached.".into(),
        ));
    }
    Ok(())
}

fn validate_margin(m: &Margin) -> Result<(), FileError> {
    validate_id(&m.id)?;
    files::validate_rel(&m.path)?;
    if m.start == 0 || m.end == 0 || m.end < m.start {
        return Err(FileError::BadRequest(
            "Margin line numbers must be 1-based with end ≥ start.".into(),
        ));
    }
    let title = m.title.trim();
    if title.is_empty() || title.len() > MAX_TITLE {
        return Err(FileError::BadRequest(format!(
            "Title must be 1–{MAX_TITLE} characters."
        )));
    }
    if m.body.len() > MAX_BODY {
        return Err(FileError::Unprocessable(format!(
            "Margin body is too large (max {MAX_BODY} characters)."
        )));
    }
    if m.quote.len() > MAX_QUOTE {
        return Err(FileError::BadRequest("Quote is too long.".into()));
    }
    for name in &m.media {
        validate_media_name(name)?;
    }
    Ok(())
}

pub fn list(
    workspace: &Path,
    path_filter: Option<&str>,
) -> Result<Vec<MarginIndexEntry>, FileError> {
    let idx = load_index(workspace)?;
    let mut out = idx.margins;
    if let Some(p) = path_filter.map(str::trim).filter(|s| !s.is_empty()) {
        files::validate_rel(p)?;
        out.retain(|m| m.path == p);
    }
    Ok(out)
}

pub fn get(workspace: &Path, id: &str) -> Result<Margin, FileError> {
    validate_id(id)?;
    ensure_layout(workspace)?;
    let path = entry_path(workspace, id);
    let text = read_text(&path)?;
    serde_json::from_str(&text).map_err(|e| FileError::Internal(e.to_string()))
}

pub fn put(workspace: &Path, margin: Margin) -> Result<Margin, FileError> {
    validate_margin(&margin)?;
    ensure_layout(workspace)?;
    let mut idx = load_index(workspace)?;
    let existing = idx.margins.iter().any(|e| e.id == margin.id);
    if !existing && idx.margins.len() >= MAX_MARGINS {
        return Err(FileError::Unprocessable(format!(
            "This workspace already has {MAX_MARGINS} margins."
        )));
    }
    let mut stored = margin;
    stored.title = stored.title.trim().to_string();
    stored.updated_at = now_ms();
    // Keep only media files that still exist on disk.
    let mdir = media_dir(workspace, &stored.id);
    if check_directory(&mdir, false)? {
        stored
            .media
            .retain(|name| fs::symlink_metadata(mdir.join(name)).is_ok_and(|meta| meta.is_file()));
    } else {
        stored.media.clear();
    }

    let text =
        serde_json::to_string_pretty(&stored).map_err(|e| FileError::Internal(e.to_string()))?;
    atomic_write(&entry_path(workspace, &stored.id), text.as_bytes())?;

    let entry = MarginIndexEntry {
        id: stored.id.clone(),
        path: stored.path.clone(),
        start: stored.start,
        end: stored.end,
        quote: stored.quote.clone(),
        title: stored.title.clone(),
        updated_at: stored.updated_at,
    };
    if let Some(slot) = idx.margins.iter_mut().find(|e| e.id == stored.id) {
        *slot = entry;
    } else {
        idx.margins.push(entry);
    }
    save_index(workspace, &idx)?;
    Ok(stored)
}

pub fn delete(workspace: &Path, id: &str) -> Result<(), FileError> {
    validate_id(id)?;
    ensure_layout(workspace)?;
    let mut idx = load_index(workspace)?;
    let before = idx.margins.len();
    idx.margins.retain(|e| e.id != id);
    if idx.margins.len() == before {
        return Err(FileError::NotFound("No margin with that id.".into()));
    }
    let dir = media_dir(workspace, id);
    let has_media = check_directory(&dir, false)?;
    let _ = fs::remove_file(entry_path(workspace, id));
    if has_media {
        let _ = fs::remove_dir_all(dir);
    }
    save_index(workspace, &idx)?;
    Ok(())
}

pub fn put_media(workspace: &Path, id: &str, name: &str, bytes: &[u8]) -> Result<(), FileError> {
    validate_id(id)?;
    validate_media_name(name)?;
    if bytes.len() as u64 > MAX_MEDIA_BYTES {
        return Err(FileError::Unprocessable(format!(
            "This image is {} bytes; SyncAiDync stores images up to 2 MiB.",
            bytes.len()
        )));
    }
    let mut margin = get(workspace, id)?;
    let dir = media_dir(workspace, id);
    check_directory(&dir, true)?;
    atomic_write(&dir.join(name), bytes)?;
    if !margin.media.iter().any(|n| n == name) {
        margin.media.push(name.to_string());
    }
    put(workspace, margin)?;
    Ok(())
}

pub fn get_media(
    workspace: &Path,
    id: &str,
    name: &str,
) -> Result<(Vec<u8>, &'static str), FileError> {
    validate_id(id)?;
    validate_media_name(name)?;
    ensure_layout(workspace)?;
    if !check_directory(&media_dir(workspace, id), false)? {
        return Err(FileError::NotFound("That media file is gone.".into()));
    }
    files::read_media(workspace, &format!(".syncaidync/margins/media/{id}/{name}"))
}

pub fn delete_media(workspace: &Path, id: &str, name: &str) -> Result<(), FileError> {
    validate_id(id)?;
    validate_media_name(name)?;
    let mut margin = get(workspace, id)?;
    let dir = media_dir(workspace, id);
    if check_directory(&dir, false)? {
        let _ = fs::remove_file(dir.join(name));
    }
    margin.media.retain(|n| n != name);
    put(workspace, margin)?;
    Ok(())
}

/// Rewrite `media/name` image destinations in margin markdown to the owner media URL.
/// `owner` is an API prefix such as `sessions/s1` or `pulls/p1`.
pub fn render_body_html(owner: &str, margin_id: &str, body: &str) -> String {
    crate::markdown::to_safe_html_with_context(
        body,
        Some(&crate::markdown::RenderContext {
            owner,
            base_path: "",
            margin_id: Some(margin_id),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let p = std::env::temp_dir().join(format!("dync-margins-{}", ids::random_hex(6)));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn put_get_list_delete_and_index_md() {
        let root = tmp();
        let m = Margin {
            id: "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee".into(),
            path: "src/a.rs".into(),
            start: 3,
            end: 5,
            quote: "fn a() {}".into(),
            title: "Why a exists".into(),
            body: "See the diagram.\n\n![d](media/d.png)\n".into(),
            media: vec![],
            updated_at: 0,
        };
        let saved = put(&root, m).unwrap();
        assert!(saved.updated_at > 0);
        let got = get(&root, &saved.id).unwrap();
        assert_eq!(got.title, "Why a exists");
        let list = list(&root, Some("src/a.rs")).unwrap();
        assert_eq!(list.len(), 1);
        let md = fs::read_to_string(index_md_path(&root)).unwrap();
        assert!(md.contains("Why a exists"));
        assert!(md.contains(&saved.id));
        assert!(dync_root(&root).join(".gitignore").exists());
        delete(&root, &saved.id).unwrap();
        assert!(get(&root, &saved.id).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_symlinked_sidecar_directories_and_entries() {
        use std::os::unix::fs::symlink;
        let outside = tmp();
        let root = tmp();
        symlink(&outside, dync_root(&root)).unwrap();
        assert!(matches!(list(&root, None), Err(FileError::Forbidden(_))));
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);

        fs::remove_file(dync_root(&root)).unwrap();
        ensure_layout(&root).unwrap();
        let id = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
        symlink(outside.join("note.json"), entry_path(&root, id)).unwrap();
        fs::write(outside.join("note.json"), "{}").unwrap();
        assert!(matches!(get(&root, id), Err(FileError::Forbidden(_))));

        symlink(&outside, media_dir(&root, id)).unwrap();
        assert!(matches!(
            get_media(&root, id, "image.png"),
            Err(FileError::Forbidden(_))
        ));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn rejects_bad_path_and_escape_id() {
        let root = tmp();
        let m = Margin {
            id: "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee".into(),
            path: "../x.rs".into(),
            start: 1,
            end: 1,
            quote: String::new(),
            title: "x".into(),
            body: String::new(),
            media: vec![],
            updated_at: 0,
        };
        assert!(put(&root, m).is_err());
        assert!(validate_id("../x").is_err());
        let _ = fs::remove_dir_all(&root);
    }
}
