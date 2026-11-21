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
        "title": "plan test"
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

#[tokio::test]
async fn plan_with_fake_claude() {
    let _guard = ENV_LOCK.lock().await;
    let ts = start_server().await;
    let repo = ts.home.join("plan-repo");
    fs::create_dir_all(repo.join("src")).unwrap();
    git(&repo, &["init"]);
    git(&repo, &["checkout", "-b", "main"]);
    fs::write(repo.join("src/a.rs"), "pub fn a() {}\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "base"]);
    fs::write(repo.join("src/a.rs"), "pub fn a() {}\npub fn b() {}\n").unwrap();

    let id = folder_session(&ts, &repo.to_string_lossy()).await;
    let auth = [
        ("Authorization", format!("Bearer {}", ts.key())),
        ("Content-Type", "application/json".into()),
        ("Origin", format!("http://127.0.0.1:{}", ts.port())),
    ];
    let refs = [
        (auth[0].0, auth[0].1.as_str()),
        (auth[1].0, auth[1].1.as_str()),
        (auth[2].0, auth[2].1.as_str()),
    ];

    let (status, _, body) = request(
        "GET",
        &format!("{}/api/v1/sessions/{id}/changes", ts.base()),
        &ts.host(),
        &refs,
        None,
    )
    .await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    let cs: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let hunk_id = cs["files"][0]["hunks"][0]["id"].as_str().unwrap();

    let marker = ts.home.join("oneshot-ran");
    let script = ts.home.join("fake-claude");
    let out_file = ts.home.join("fake-claude-out.json");
    let result_json = serde_json::json!({
        "summary": "Adds b",
        "checkpoints": [{
            "title": "New helper",
            "why": "behaviour",
            "look_for": ["b"],
            "hunks": [hunk_id]
        }]
    });
    let wrapped = serde_json::json!({ "result": result_json.to_string() });
    fs::write(&out_file, wrapped.to_string()).unwrap();
    write_script(
        &script,
        &format!(
            "#!/bin/sh\nset -eu\nif [ -n \"${{SYNCAIDYNC_SESSION:-}}\" ]; then echo leaked >&2; exit 2; fi\npwd > \"{}.cwd\"\ncat > /dev/null\ncat \"{}\"\ntouch \"{}\"\n",
            marker.display(),
            out_file.display(),
            marker.display()
        ),
    );
    std::env::set_var("SYNCAIDYNC_BIN_CLAUDE", &script);

    let plan_body = serde_json::to_vec(&json!({ "agent": "claude", "model": "sonnet" })).unwrap();
    let (status, _, body) = request(
        "POST",
        &format!("{}/api/v1/sessions/{id}/changes/plan", ts.base()),
        &ts.host(),
        &refs,
        Some(&plan_body),
    )
    .await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    let plan: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(plan["summary"], "Adds b");
    assert_eq!(plan["checkpoints"][0]["id"], "c1");
    assert!(marker.is_file());

    let (status, _, body) = request(
        "GET",
        &format!("{}/api/v1/sessions/{id}/review", ts.base()),
        &ts.host(),
        &refs,
        None,
    )
    .await;
    assert_eq!(status, 200);
    let review: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(review["plan"]["summary"], "Adds b");

    std::env::remove_var("SYNCAIDYNC_BIN_CLAUDE");
    ts.shutdown().await;
}

#[tokio::test]
async fn plan_timeout_is_503() {
    let _guard = ENV_LOCK.lock().await;
    let ts = start_server().await;
    let repo = ts.home.join("timeout-repo");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init"]);
    git(&repo, &["checkout", "-b", "main"]);
    fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "base"]);
    fs::write(repo.join("a.txt"), "b\n").unwrap();

    let script = ts.home.join("slow-claude");
    write_script(
        &script,
        "#!/bin/sh\nsleep 30\necho '{\"result\":\"{\\\"summary\\\":\\\"x\\\",\\\"checkpoints\\\":[]}\"}'\n",
    );
    std::env::set_var("SYNCAIDYNC_BIN_CLAUDE", &script);
    std::env::set_var("SYNCAIDYNC_ONESHOT_TIMEOUT_MS", "200");

    let id = folder_session(&ts, &repo.to_string_lossy()).await;
    let auth = [
        ("Authorization", format!("Bearer {}", ts.key())),
        ("Content-Type", "application/json".into()),
        ("Origin", format!("http://127.0.0.1:{}", ts.port())),
    ];
    let refs = [
        (auth[0].0, auth[0].1.as_str()),
        (auth[1].0, auth[1].1.as_str()),
        (auth[2].0, auth[2].1.as_str()),
    ];
    let plan_body = serde_json::to_vec(&json!({ "agent": "claude" })).unwrap();
    let (status, _, body) = request(
        "POST",
        &format!("{}/api/v1/sessions/{id}/changes/plan", ts.base()),
        &ts.host(),
        &refs,
        Some(&plan_body),
    )
    .await;
    assert_eq!(status, 503, "{}", String::from_utf8_lossy(&body));

    std::env::remove_var("SYNCAIDYNC_BIN_CLAUDE");
    std::env::remove_var("SYNCAIDYNC_ONESHOT_TIMEOUT_MS");
    ts.shutdown().await;
}

#[tokio::test]
async fn plan_nonsense_is_503() {
    let _guard = ENV_LOCK.lock().await;
    let ts = start_server().await;
    let repo = ts.home.join("junk-repo");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init"]);
    git(&repo, &["checkout", "-b", "main"]);
    fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "base"]);
    fs::write(repo.join("a.txt"), "b\n").unwrap();

    let script = ts.home.join("junk-claude");
    write_script(
        &script,
        "#!/bin/sh\necho '{\"result\":\"not-json-plan\"}'\n",
    );
    std::env::set_var("SYNCAIDYNC_BIN_CLAUDE", &script);

    let id = folder_session(&ts, &repo.to_string_lossy()).await;
    let auth = [
        ("Authorization", format!("Bearer {}", ts.key())),
        ("Content-Type", "application/json".into()),
        ("Origin", format!("http://127.0.0.1:{}", ts.port())),
    ];
    let refs = [
        (auth[0].0, auth[0].1.as_str()),
        (auth[1].0, auth[1].1.as_str()),
        (auth[2].0, auth[2].1.as_str()),
    ];
    let plan_body = serde_json::to_vec(&json!({ "agent": "claude" })).unwrap();
    let (status, _, body) = request(
        "POST",
        &format!("{}/api/v1/sessions/{id}/changes/plan", ts.base()),
        &ts.host(),
        &refs,
        Some(&plan_body),
    )
    .await;
    assert_eq!(status, 503, "{}", String::from_utf8_lossy(&body));

    std::env::remove_var("SYNCAIDYNC_BIN_CLAUDE");
    ts.shutdown().await;
}
