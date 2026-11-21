mod common;

use std::process::Command;

use common::{request, start_server, start_server_with_roots};
use serde_json::json;

fn temp_git_repo() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "syncaidync-ws-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(&dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(&dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(&dir)
        .output()
        .unwrap();
    std::fs::write(dir.join("README"), "hi").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(&dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(&dir)
        .output()
        .unwrap();
    dir
}

async fn auth_headers(ts: &common::TestServer) -> Vec<(String, String)> {
    vec![
        ("Authorization".into(), format!("Bearer {}", ts.key())),
        ("Content-Type".into(), "application/json".into()),
        ("Origin".into(), format!("http://127.0.0.1:{}", ts.port())),
    ]
}

#[tokio::test]
async fn worktree_create_conflict_and_remove() {
    let repo = temp_git_repo();
    let ts = start_server().await;
    let headers = auth_headers(&ts).await;
    let header_refs: Vec<(&str, &str)> = headers
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    let body = serde_json::to_vec(&json!({
        "agent": "shell",
        "title": "Fix Login",
        "workspace": {
            "kind": "worktree",
            "repo": repo.to_string_lossy(),
            "base": "main",
            "branch": "dync/fix-login"
        }
    }))
    .unwrap();
    let (status, _, resp) = request(
        "POST",
        &format!("{}/api/v1/sessions", ts.base()),
        &ts.host(),
        &header_refs,
        Some(&body),
    )
    .await;
    assert_eq!(status, 201, "{}", String::from_utf8_lossy(&resp));
    let session: serde_json::Value = serde_json::from_slice(&resp).unwrap();
    let path = session["workspace"]["path"].as_str().unwrap();
    assert!(std::path::Path::new(path).is_dir());
    assert_eq!(session["workspace"]["branch"], "dync/fix-login");
    assert!(session["workspace"]["base_commit"].as_str().unwrap().len() == 40);

    // Same branch again → 409
    let (status, _, resp) = request(
        "POST",
        &format!("{}/api/v1/sessions", ts.base()),
        &ts.host(),
        &header_refs,
        Some(&body),
    )
    .await;
    assert_eq!(status, 409, "{}", String::from_utf8_lossy(&resp));

    // Second title gets a different folder
    let body2 = serde_json::to_vec(&json!({
        "agent": "shell",
        "title": "Fix Login",
        "workspace": {
            "kind": "worktree",
            "repo": repo.to_string_lossy(),
            "base": "main",
            "branch": "dync/fix-login-2"
        }
    }))
    .unwrap();
    let (status, _, resp) = request(
        "POST",
        &format!("{}/api/v1/sessions", ts.base()),
        &ts.host(),
        &header_refs,
        Some(&body2),
    )
    .await;
    assert_eq!(status, 201, "{}", String::from_utf8_lossy(&resp));
    let session2: serde_json::Value = serde_json::from_slice(&resp).unwrap();
    assert_ne!(session2["workspace"]["path"], session["workspace"]["path"]);

    // Bad branch name
    let bad = serde_json::to_vec(&json!({
        "agent": "shell",
        "workspace": {
            "kind": "worktree",
            "repo": repo.to_string_lossy(),
            "base": "main",
            "branch": "bad branch.."
        }
    }))
    .unwrap();
    let (status, _, resp) = request(
        "POST",
        &format!("{}/api/v1/sessions", ts.base()),
        &ts.host(),
        &header_refs,
        Some(&bad),
    )
    .await;
    assert_eq!(status, 400, "{}", String::from_utf8_lossy(&resp));

    // removal info + delete with remove
    let id = session["id"].as_str().unwrap();
    let (status, _, resp) = request(
        "GET",
        &format!("{}/api/v1/sessions/{id}/removal", ts.base()),
        &ts.host(),
        &header_refs,
        None,
    )
    .await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&resp));

    let (status, _, _) = request(
        "DELETE",
        &format!("{}/api/v1/sessions/{id}?workspace=remove", ts.base()),
        &ts.host(),
        &header_refs,
        None,
    )
    .await;
    assert_eq!(status, 204);
    assert!(!std::path::Path::new(path).exists());

    // Folder session never deleted
    let folder_body = serde_json::to_vec(&json!({
        "agent": "shell",
        "workspace": { "kind": "folder", "path": repo.to_string_lossy() }
    }))
    .unwrap();
    let (status, _, resp) = request(
        "POST",
        &format!("{}/api/v1/sessions", ts.base()),
        &ts.host(),
        &header_refs,
        Some(&folder_body),
    )
    .await;
    assert_eq!(status, 201, "{}", String::from_utf8_lossy(&resp));
    let folder_session: serde_json::Value = serde_json::from_slice(&resp).unwrap();
    let fid = folder_session["id"].as_str().unwrap();
    let (status, _, _) = request(
        "DELETE",
        &format!("{}/api/v1/sessions/{fid}?workspace=remove", ts.base()),
        &ts.host(),
        &header_refs,
        None,
    )
    .await;
    assert_eq!(status, 204);
    assert!(repo.is_dir());

    let _ = std::fs::remove_dir_all(&repo);
    ts.shutdown().await;
}

#[tokio::test]
async fn folders_browse_stays_inside_roots() {
    let root = std::env::temp_dir().join(format!(
        "syncaidync-folder-api-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(root.join("proj")).unwrap();

    let ts = start_server_with_roots(vec![root.clone()]).await;

    let headers = auth_headers(&ts).await;
    let header_refs: Vec<(&str, &str)> = headers
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    let (status, _, resp) = request(
        "GET",
        &format!("{}/api/v1/folders", ts.base()),
        &ts.host(),
        &header_refs,
        None,
    )
    .await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&resp));
    let data: serde_json::Value = serde_json::from_slice(&resp).unwrap();
    assert!(data["entries"].as_array().unwrap().iter().any(|e| {
        e["path"]
            .as_str()
            .map(|p| p.contains(&*root.to_string_lossy()))
            .unwrap_or(false)
    }));

    let (status, _, resp) = request(
        "GET",
        &format!(
            "{}/api/v1/folders?path={}",
            ts.base(),
            urlencoding_path(&root)
        ),
        &ts.host(),
        &header_refs,
        None,
    )
    .await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&resp));
    let data: serde_json::Value = serde_json::from_slice(&resp).unwrap();
    let names: Vec<&str> = data["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["name"].as_str())
        .collect();
    assert!(names.contains(&"proj"), "got {names:?}");
    assert!(data["parent"].is_null());

    let outside = std::env::temp_dir().join(format!(
        "syncaidync-folder-out-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&outside).unwrap();
    let (status, _, resp) = request(
        "GET",
        &format!(
            "{}/api/v1/folders?path={}",
            ts.base(),
            urlencoding_path(&outside)
        ),
        &ts.host(),
        &header_refs,
        None,
    )
    .await;
    assert_eq!(status, 400, "{}", String::from_utf8_lossy(&resp));

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&outside);
    ts.shutdown().await;
}

fn urlencoding_path(path: &std::path::Path) -> String {
    path.to_string_lossy()
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b'/' => "%2F".into(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[tokio::test]
async fn repos_scan_respects_roots_and_depth() {
    let root = std::env::temp_dir().join(format!(
        "syncaidync-roots-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(root.join("a/b/c/.git")).unwrap();
    std::fs::create_dir_all(root.join("a/b/c/d/.git")).unwrap();
    std::fs::create_dir_all(root.join("node_modules/x/.git")).unwrap();

    let ts = start_server_with_roots(vec![root.clone()]).await;

    let headers = auth_headers(&ts).await;
    let header_refs: Vec<(&str, &str)> = headers
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let (status, _, resp) = request(
        "GET",
        &format!("{}/api/v1/repos", ts.base()),
        &ts.host(),
        &header_refs,
        None,
    )
    .await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&resp));
    let data: serde_json::Value = serde_json::from_slice(&resp).unwrap();
    let repos = data["repos"].as_array().unwrap();
    let paths: Vec<&str> = repos.iter().filter_map(|r| r["path"].as_str()).collect();
    assert!(paths.iter().any(|p| p.ends_with("a/b/c")), "got {paths:?}");
    assert!(!paths.iter().any(|p| p.ends_with("a/b/c/d")));
    assert!(!paths.iter().any(|p| p.contains("node_modules")));

    let _ = std::fs::remove_dir_all(&root);
    ts.shutdown().await;
}

#[tokio::test]
async fn remove_refuses_outside_home() {
    let outside = std::env::temp_dir().join(format!(
        "syncaidync-outside-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&outside).unwrap();
    let ts = start_server().await;

    // Tamper sessions.json after creating a scratch session
    let headers = auth_headers(&ts).await;
    let header_refs: Vec<(&str, &str)> = headers
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let body = serde_json::to_vec(&json!({
        "agent": "shell",
        "workspace": { "kind": "scratch" }
    }))
    .unwrap();
    let (status, _, resp) = request(
        "POST",
        &format!("{}/api/v1/sessions", ts.base()),
        &ts.host(),
        &header_refs,
        Some(&body),
    )
    .await;
    assert_eq!(status, 201);
    let session: serde_json::Value = serde_json::from_slice(&resp).unwrap();
    let id = session["id"].as_str().unwrap().to_string();

    // Rewrite persisted session to point owned path outside home
    let home = ts.home.clone();
    // Flush the saver before we rewrite sessions.json on disk.
    ts.shutdown().await;

    let path = home.join("sessions.json");
    let raw = std::fs::read_to_string(&path).expect("sessions.json should exist");
    let mut file: serde_json::Value = serde_json::from_str(&raw).unwrap();
    for s in file["sessions"].as_array_mut().unwrap() {
        if s["id"] == id {
            s["workspace"]["path"] = json!(outside.to_string_lossy());
            s["workspace"]["owned"] = json!(true);
            s["workspace"]["kind"] = json!("scratch");
        }
    }
    std::fs::write(&path, serde_json::to_vec_pretty(&file).unwrap()).unwrap();

    // Start a new server on the same home so the tampered session is loaded.
    let web = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web");
    let config = syncaidync::config::Config {
        home: home.clone(),
        port: 0,
        key: "b".repeat(32),
        roots: Vec::new(),
        web_dir: Some(web),
        no_open: true,
        parent_pid: None,
        dev_shell: true,
    };
    let server = syncaidync::http::serve(config).await.expect("serve");
    let ts2 = common::TestServer { server, home };
    let headers2 = auth_headers(&ts2).await;
    let header_refs2: Vec<(&str, &str)> = headers2
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    let (status, _, resp) = request(
        "DELETE",
        &format!("{}/api/v1/sessions/{id}?workspace=remove", ts2.base()),
        &ts2.host(),
        &header_refs2,
        None,
    )
    .await;
    assert_eq!(status, 422, "{}", String::from_utf8_lossy(&resp));
    assert!(outside.is_dir());

    // The failed removal leaves the ended session available to remove on its own.
    let (status, _, _) = request(
        "DELETE",
        &format!("{}/api/v1/sessions/{id}", ts2.base()),
        &ts2.host(),
        &header_refs2,
        None,
    )
    .await;
    assert_eq!(status, 204);
    assert!(outside.is_dir());

    let _ = std::fs::remove_dir_all(&outside);
    ts2.shutdown().await;
}
