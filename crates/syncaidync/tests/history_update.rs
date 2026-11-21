mod common;

use common::{request, start_server};
use serde_json::{json, Value};
use std::process::Command;

fn git(dir: &std::path::Path, args: &[&str]) {
    assert!(Command::new("git")
        .current_dir(dir)
        .args(args)
        .status()
        .unwrap()
        .success());
}

#[tokio::test]
async fn fixture_history_import_and_daily_update() {
    let ts = start_server().await;
    let work = ts.home.join("work");
    let config = ts.home.join("claude");
    let projects = config.join("projects/sample");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(&projects).unwrap();
    std::fs::write(projects.join("valid.jsonl"), format!(
        "not json\n{}\n{}\n{}\n",
        json!({"cwd": work, "type":"user", "message":{"content":"<command>skip</command>"}}),
        json!({"type":"user", "message":{"content":[{"type":"tool_result"}]}}),
        json!({"type":"user", "message":{"content":[{"type":"text","text":"Summarize the fixture"}]}})
    )).unwrap();
    std::fs::write(
        projects.join("gone.jsonl"),
        json!({
            "cwd": work.join("gone"), "type":"user", "message":{"content":"Skip missing workspace"}
        })
        .to_string(),
    )
    .unwrap();
    std::env::set_var("CLAUDE_CONFIG_DIR", &config);

    let base = format!("{}/api/v1", ts.base());
    let host = ts.host();
    let authorization = format!("Bearer {}", ts.key());
    let origin = ts.base();
    let headers = [
        ("Authorization", authorization.as_str()),
        ("Origin", origin.as_str()),
    ];

    let (status, _, body) = request("GET", &format!("{base}/history"), &host, &headers, None).await;
    assert_eq!(status, 200);
    let list: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    assert_eq!(list["items"][0]["first_prompt"], "Summarize the fixture");

    let (status, _, body) = request(
        "POST",
        &format!("{base}/history/import"),
        &host,
        &headers,
        Some(br#"{"agent":"claude","agent_session":"valid"}"#),
    )
    .await;
    assert_eq!(status, 201, "{}", String::from_utf8_lossy(&body));
    let session: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(session["state"], "ended");
    assert_eq!(session["workspace"]["owned"], false);
    assert_eq!(session["agent_session"], "valid");
    let (_, _, body) = request("GET", &format!("{base}/history"), &host, &headers, None).await;
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["items"],
        json!([])
    );

    git(&work, &["init", "-b", "main"]);
    git(
        &work,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "base",
        ],
    );
    for tag in ["v0.2.0", "v0.12.0", "v2.0.0-rc.1", "unrelated"] {
        git(&work, &["tag", tag]);
    }
    git(&work, &["clone", "--bare", ".", "../release.git"]);
    std::env::set_var("SYNCAIDYNC_UPDATE_REMOTE", ts.home.join("release.git"));
    std::env::remove_var("SYNCAIDYNC_NO_UPDATE_CHECK");
    let (status, _, body) = request("GET", &format!("{base}/update"), &host, &headers, None).await;
    assert_eq!(status, 200);
    let update: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(update["latest"], "v0.12.0");
    assert_eq!(update["newer"], true);
    let record: Value =
        serde_json::from_slice(&std::fs::read(ts.home.join("config.json")).unwrap()).unwrap();
    assert!(record["update"]["checked_at"].as_u64().unwrap() > 0);

    std::env::set_var("SYNCAIDYNC_UPDATE_REMOTE", ts.home.join("missing.git"));
    let (_, _, body) = request("GET", &format!("{base}/update"), &host, &headers, None).await;
    assert_eq!(serde_json::from_slice::<Value>(&body).unwrap(), update);
    ts.shutdown().await;
}
