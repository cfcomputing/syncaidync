mod common;

use common::{request, start_server, TestServer};
use serde_json::json;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use tokio::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::const_new(());

fn git(cwd: &std::path::Path, args: &[&str]) {
    assert!(Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .status()
        .unwrap()
        .success());
}

fn write_script(path: &std::path::Path, body: &str) {
    fs::write(path, body).unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

async fn folder_session(ts: &TestServer, path: &str) -> String {
    let body = serde_json::to_vec(&json!({
        "agent": "shell",
        "workspace": { "kind": "folder", "path": path },
        "title": "takeaways test"
    }))
    .unwrap();
    let (status, _, resp) = request(
        "POST",
        &format!("{}/api/v1/sessions", ts.base()),
        &ts.host(),
        &[
            ("Authorization", &format!("Bearer {}", ts.key())),
            ("Content-Type", "application/json"),
            ("Origin", &format!("http://127.0.0.1:{}", ts.port())),
        ],
        Some(&body),
    )
    .await;
    assert_eq!(status, 201, "{}", String::from_utf8_lossy(&resp));
    let v: serde_json::Value = serde_json::from_slice(&resp).unwrap();
    v["id"].as_str().unwrap().to_string()
}

fn auth_refs(ts: &TestServer) -> [(String, String); 3] {
    [
        ("Authorization".into(), format!("Bearer {}", ts.key())),
        ("Content-Type".into(), "application/json".into()),
        ("Origin".into(), format!("http://127.0.0.1:{}", ts.port())),
    ]
}

#[tokio::test]
async fn suggest_and_save_with_fake_claude() {
    let _guard = ENV_LOCK.lock().await;
    let ts = start_server().await;
    let repo = ts.home.join("take-repo");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init"]);
    git(&repo, &["checkout", "-b", "main"]);
    fs::write(repo.join("readme.md"), "hi\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "base"]);

    let id = folder_session(&ts, &repo.to_string_lossy()).await;
    let log_dir = ts.home.join("logs");
    fs::create_dir_all(&log_dir).unwrap();
    fs::write(
        log_dir.join(format!("{id}.log")),
        "You: please use cargo test\nAgent: sure, cargo test works\n",
    )
    .unwrap();

    let out_file = ts.home.join("fake-takeaways.json");
    let wrapped = serde_json::json!({
        "result": serde_json::json!({
            "takeaways": [{
                "title": "Prefer cargo test",
                "body": "Run cargo test before claiming done.",
                "scope": "project"
            }, {
                "title": "Keep prompts short",
                "body": "Short prompts land better.",
                "scope": "everywhere"
            }]
        }).to_string()
    });
    fs::write(&out_file, wrapped.to_string()).unwrap();
    let script = ts.home.join("fake-claude");
    write_script(
        &script,
        &format!(
            "#!/bin/sh\nset -eu\ncat > /dev/null\ncat \"{}\"\n",
            out_file.display()
        ),
    );
    std::env::set_var("SYNCAIDYNC_BIN_CLAUDE", &script);

    let auth = auth_refs(&ts);
    let refs = [
        (auth[0].0.as_str(), auth[0].1.as_str()),
        (auth[1].0.as_str(), auth[1].1.as_str()),
        (auth[2].0.as_str(), auth[2].1.as_str()),
    ];

    let (status, _, body) = request(
        "GET",
        &format!("{}/api/v1/sessions/{id}/takeaways/destinations", ts.base()),
        &ts.host(),
        &refs,
        None,
    )
    .await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    let dest: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(dest["project"].as_str().unwrap().ends_with("AGENTS.md"));
    assert!(dest["note"]
        .as_str()
        .unwrap()
        .contains("notes/everywhere.md"));
    assert_eq!(dest["everywhere"], dest["note"]);

    let suggest_body = serde_json::to_vec(&json!({ "agent": "claude" })).unwrap();
    let (status, _, body) = request(
        "POST",
        &format!("{}/api/v1/sessions/{id}/takeaways/suggest", ts.base()),
        &ts.host(),
        &refs,
        Some(&suggest_body),
    )
    .await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    let sug: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(sug["takeaways"].as_array().unwrap().len(), 2);

    let save_body = serde_json::to_vec(&json!({
        "takeaways": sug["takeaways"]
    }))
    .unwrap();
    let (status, _, body) = request(
        "POST",
        &format!("{}/api/v1/sessions/{id}/takeaways/save", ts.base()),
        &ts.host(),
        &refs,
        Some(&save_body),
    )
    .await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    let saved: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(!saved["written"].as_array().unwrap().is_empty());

    let project_path = dest["project"].as_str().unwrap();
    let everywhere_path = dest["everywhere"].as_str().unwrap();
    let project_text = fs::read_to_string(project_path).unwrap();
    assert!(project_text.contains("Prefer cargo test"));
    assert!(project_text.contains("## Takeaways (SyncAiDync)"));
    let everywhere_text = fs::read_to_string(everywhere_path).unwrap();
    assert!(everywhere_text.contains("Keep prompts short"));

    // Saving twice does not duplicate.
    let (status, _, body) = request(
        "POST",
        &format!("{}/api/v1/sessions/{id}/takeaways/save", ts.base()),
        &ts.host(),
        &refs,
        Some(&save_body),
    )
    .await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    let project_text2 = fs::read_to_string(project_path).unwrap();
    assert_eq!(
        project_text2.matches("Prefer cargo test").count(),
        1,
        "{project_text2}"
    );

    std::env::remove_var("SYNCAIDYNC_BIN_CLAUDE");
    ts.shutdown().await;
}

#[tokio::test]
async fn project_scope_without_repo_is_422() {
    let _guard = ENV_LOCK.lock().await;
    let ts = start_server().await;
    let folder = ts.home.join("plain-folder");
    fs::create_dir_all(&folder).unwrap();
    let id = folder_session(&ts, &folder.to_string_lossy()).await;

    let auth = auth_refs(&ts);
    let refs = [
        (auth[0].0.as_str(), auth[0].1.as_str()),
        (auth[1].0.as_str(), auth[1].1.as_str()),
        (auth[2].0.as_str(), auth[2].1.as_str()),
    ];

    let (status, _, body) = request(
        "GET",
        &format!("{}/api/v1/sessions/{id}/takeaways/destinations", ts.base()),
        &ts.host(),
        &refs,
        None,
    )
    .await;
    assert_eq!(status, 200);
    let dest: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(dest["project"].is_null());

    let save_body = serde_json::to_vec(&json!({
        "takeaways": [{
            "title": "Nope",
            "body": "Should fail",
            "scope": "project"
        }]
    }))
    .unwrap();
    let (status, _, body) = request(
        "POST",
        &format!("{}/api/v1/sessions/{id}/takeaways/save", ts.base()),
        &ts.host(),
        &refs,
        Some(&save_body),
    )
    .await;
    assert_eq!(status, 422, "{}", String::from_utf8_lossy(&body));

    ts.shutdown().await;
}
