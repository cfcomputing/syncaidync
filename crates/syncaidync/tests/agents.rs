mod common;

use std::process::Stdio;
use std::time::{Duration, Instant};

use common::{request, start_server, TestServer};
use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

async fn create_shell(ts: &TestServer) -> serde_json::Value {
    let body = serde_json::to_vec(&json!({
        "agent": "shell",
        "workspace": { "kind": "scratch" },
        "title": "agent callback test"
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
    serde_json::from_slice(&resp).unwrap()
}

async fn signal_token(ts: &TestServer, id: &str) -> String {
    ts.server
        .state
        .registry
        .as_ref()
        .expect("registry")
        .signal_token(id)
        .await
        .expect("token")
}

async fn get_session(ts: &TestServer, id: &str) -> serde_json::Value {
    let (status, _, resp) = request(
        "GET",
        &format!("{}/api/v1/sessions", ts.base()),
        &ts.host(),
        &[("Authorization", &format!("Bearer {}", ts.key()))],
        None,
    )
    .await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_slice(&resp).unwrap();
    v["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id)
        .cloned()
        .expect("session")
}

#[tokio::test]
async fn signal_auth_and_apply() {
    let ts = start_server().await;
    let session = create_shell(&ts).await;
    let id = session["id"].as_str().unwrap().to_string();
    let token = signal_token(&ts, &id).await;

    // Right token → 204 and state changes (PromptSubmitted → working)
    let body = serde_json::to_vec(&json!({
        "agent": "claude",
        "payload": { "hook_event_name": "UserPromptSubmit" }
    }))
    .unwrap();
    let (status, _, _) = request(
        "POST",
        &format!("{}/agent/v1/signal?session={id}", ts.base()),
        &ts.host(),
        &[
            ("Authorization", &format!("Bearer {token}")),
            ("Content-Type", "application/json"),
            ("Origin", &format!("http://127.0.0.1:{}", ts.port())),
        ],
        Some(&body),
    )
    .await;
    assert_eq!(status, 204);
    let s = get_session(&ts, &id).await;
    assert_eq!(s["state"], "working");

    // Wrong token → 401
    let (status, _, _) = request(
        "POST",
        &format!("{}/agent/v1/signal?session={id}", ts.base()),
        &ts.host(),
        &[
            (
                "Authorization",
                "Bearer deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
            ),
            ("Content-Type", "application/json"),
            ("Origin", &format!("http://127.0.0.1:{}", ts.port())),
        ],
        Some(&body),
    )
    .await;
    assert_eq!(status, 401);

    // Launch key must not work on /agent/v1
    let (status, _, _) = request(
        "POST",
        &format!("{}/agent/v1/signal?session={id}", ts.base()),
        &ts.host(),
        &[
            ("Authorization", &format!("Bearer {}", ts.key())),
            ("Content-Type", "application/json"),
            ("Origin", &format!("http://127.0.0.1:{}", ts.port())),
        ],
        Some(&body),
    )
    .await;
    assert_eq!(status, 401);

    // Token for another session → 401
    let other = create_shell(&ts).await;
    let other_id = other["id"].as_str().unwrap();
    let other_token = signal_token(&ts, other_id).await;
    let (status, _, _) = request(
        "POST",
        &format!("{}/agent/v1/signal?session={id}", ts.base()),
        &ts.host(),
        &[
            ("Authorization", &format!("Bearer {other_token}")),
            ("Content-Type", "application/json"),
            ("Origin", &format!("http://127.0.0.1:{}", ts.port())),
        ],
        Some(&body),
    )
    .await;
    assert_eq!(status, 401);

    ts.shutdown().await;
}

#[tokio::test]
async fn hookcli_signal_subcommand() {
    let ts = start_server().await;
    let session = create_shell(&ts).await;
    let id = session["id"].as_str().unwrap().to_string();
    let token = signal_token(&ts, &id).await;

    let bin = env!("CARGO_BIN_EXE_syncaidync");
    let payload = json!({ "hook_event_name": "Stop" });
    let mut child = Command::new(bin)
        .args(["signal", "--agent", "claude"])
        .env("SYNCAIDYNC_URL", ts.base())
        .env("SYNCAIDYNC_SESSION", &id)
        .env("SYNCAIDYNC_SIGNAL_TOKEN", &token)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    {
        let mut stdin = child.stdin.take().unwrap();
        stdin
            .write_all(payload.to_string().as_bytes())
            .await
            .unwrap();
    }
    let status = child.wait().await.unwrap();
    assert_eq!(status.code(), Some(0));

    // Give the server a beat to apply.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let s = get_session(&ts, &id).await;
    assert_eq!(s["state"], "ready", "{s}");

    // No environment → exit 0, no output
    let out = Command::new(bin)
        .args(["signal", "--agent", "claude"])
        .env_remove("SYNCAIDYNC_URL")
        .env_remove("SYNCAIDYNC_SESSION")
        .env_remove("SYNCAIDYNC_SIGNAL_TOKEN")
        .stdin(Stdio::null())
        .output()
        .await
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    assert!(out.stderr.is_empty());

    // Closed port → exit 0 within 3 seconds
    let start = Instant::now();
    let mut child = Command::new(bin)
        .args(["signal", "--agent", "claude"])
        .env("SYNCAIDYNC_URL", "http://127.0.0.1:9")
        .env("SYNCAIDYNC_SESSION", "x")
        .env("SYNCAIDYNC_SIGNAL_TOKEN", "y")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(b"{}").await;
    }
    let status = tokio::time::timeout(Duration::from_secs(3), child.wait())
        .await
        .expect("timed out")
        .unwrap();
    assert_eq!(status.code(), Some(0));
    assert!(start.elapsed() < Duration::from_secs(3));

    ts.shutdown().await;
}

#[tokio::test]
async fn startup_marker_via_shell() {
    let ts = start_server().await;
    let body = serde_json::to_vec(&json!({
        "agent": "shell",
        "workspace": { "kind": "scratch" },
        "title": "marker",
        "prompt": "printf 'syncaidync-test-startup-block\\n'"
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
    let session: serde_json::Value = serde_json::from_slice(&resp).unwrap();
    let id = session["id"].as_str().unwrap();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(4);
    loop {
        let s = get_session(&ts, id).await;
        if s["state"] == "blocked"
            && s["blocked"]["kind"] == "startup"
            && s["blocked"]["message"] == "Test startup block"
        {
            break;
        }
        if tokio::time::Instant::now() > deadline {
            panic!("expected startup block; got {s}");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    ts.shutdown().await;
}

#[tokio::test]
async fn meta_lists_installed_agents() {
    let ts = start_server().await;
    let (status, _, resp) = request(
        "GET",
        &format!("{}/api/v1/meta", ts.base()),
        &ts.host(),
        &[("Authorization", &format!("Bearer {}", ts.key()))],
        None,
    )
    .await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_slice(&resp).unwrap();
    let agents = v["agents"].as_array().unwrap();
    let ids: Vec<&str> = agents.iter().filter_map(|a| a["id"].as_str()).collect();
    assert!(ids.contains(&"claude"));
    assert!(ids.contains(&"codex"));
    assert!(ids.contains(&"pi"));
    assert!(ids.contains(&"cursor"));
    assert!(ids.contains(&"shell")); // dev_shell
    ts.shutdown().await;
}
