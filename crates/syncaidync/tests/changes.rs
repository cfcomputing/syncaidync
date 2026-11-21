mod common;

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use common::{request, start_server, TestServer};
use serde_json::json;

fn git(cwd: &std::path::Path, args: &[&str]) {
    let st = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

async fn create_folder_session(ts: &TestServer, path: &str) -> serde_json::Value {
    let body = serde_json::to_vec(&json!({
        "agent": "shell",
        "workspace": { "kind": "folder", "path": path },
        "title": "changes test"
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

fn auth_headers(ts: &TestServer) -> [(&'static str, String); 3] {
    [
        ("Authorization", format!("Bearer {}", ts.key())),
        ("Content-Type", "application/json".into()),
        ("Origin", format!("http://127.0.0.1:{}", ts.port())),
    ]
}

#[tokio::test]
async fn changes_and_review_round_trip() {
    let ts = start_server().await;
    let repo = ts.home.join("sample-repo");
    fs::create_dir_all(repo.join("src")).unwrap();
    git(&repo, &["init"]);
    git(&repo, &["checkout", "-b", "main"]);
    fs::write(repo.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
    fs::write(
        repo.join("src/main.rs"),
        "mod lib;\nfn main() { lib::a(); }\n",
    )
    .unwrap();
    fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname=\"s\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "base"]);

    // committed change
    fs::write(repo.join("src/lib.rs"), "pub fn a() {}\npub fn b() {}\n").unwrap();
    git(&repo, &["add", "src/lib.rs"]);
    git(&repo, &["commit", "-m", "add b"]);

    // uncommitted + untracked
    fs::write(
        repo.join("src/lib.rs"),
        "pub fn a() {}\npub fn b() {}\npub fn c() {}\n",
    )
    .unwrap();
    fs::write(repo.join("src/extra.rs"), "pub fn e() {}\n").unwrap();

    let session = create_folder_session(&ts, &repo.to_string_lossy()).await;
    let id = session["id"].as_str().unwrap();
    let base = session["workspace"]["base_commit"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(base.len(), 40);

    // Point base_commit at first commit so both commits + dirty show — folder uses HEAD at create.
    // Re-read changes against current base (HEAD at create = tip with b). Uncommitted c + untracked extra.
    let h = auth_headers(&ts);
    let refs: [_; 3] = [
        (h[0].0, h[0].1.as_str()),
        (h[1].0, h[1].1.as_str()),
        (h[2].0, h[2].1.as_str()),
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
    let paths: Vec<_> = cs["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert!(paths.contains(&"src/lib.rs"), "{paths:?}");
    assert!(paths.contains(&"src/extra.rs"), "{paths:?}");

    // map should include main.rs importing lib if grep finds it
    let nodes = cs["map"]["nodes"].as_array().unwrap();
    assert!(nodes
        .iter()
        .any(|n| n["path"] == "src/lib.rs" && n["changed"] == true));

    // review put/get
    let note = json!({
        "reviewed": { "src/lib.rs": cs["files"].as_array().unwrap().iter().find(|f| f["path"]=="src/lib.rs").unwrap()["digest"] },
        "notes": [{
            "id": "11111111-1111-1111-1111-111111111111",
            "path": "src/lib.rs",
            "side": "new",
            "start": 3,
            "end": 3,
            "quote": "pub fn c() {}",
            "text": "why c?",
            "sent": false,
            "created_at": 1
        }],
        "overall": "looks fine"
    });
    let bytes = serde_json::to_vec(&note).unwrap();
    let (status, _, body) = request(
        "PUT",
        &format!("{}/api/v1/sessions/{id}/review", ts.base()),
        &ts.host(),
        &refs,
        Some(&bytes),
    )
    .await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));

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
    assert_eq!(review["overall"], "looks fine");
    assert_eq!(review["notes"].as_array().unwrap().len(), 1);

    // second changes call (cache)
    let (status, _, body2) = request(
        "GET",
        &format!("{}/api/v1/sessions/{id}/changes", ts.base()),
        &ts.host(),
        &refs,
        None,
    )
    .await;
    assert_eq!(status, 200);
    let cs2: serde_json::Value = serde_json::from_slice(&body2).unwrap();
    assert_eq!(cs2["stamp"], cs["stamp"]);

    let _ = PathBuf::from(&base);
    ts.shutdown().await;
}

#[tokio::test]
async fn changes_non_git_is_422() {
    let ts = start_server().await;
    let session = {
        let body = serde_json::to_vec(&json!({
            "agent": "shell",
            "workspace": { "kind": "scratch" },
            "title": "no git"
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
        assert_eq!(status, 201);
        serde_json::from_slice::<serde_json::Value>(&resp).unwrap()
    };
    let id = session["id"].as_str().unwrap();
    let h = auth_headers(&ts);
    let refs: [_; 3] = [
        (h[0].0, h[0].1.as_str()),
        (h[1].0, h[1].1.as_str()),
        (h[2].0, h[2].1.as_str()),
    ];
    let (status, _, body) = request(
        "GET",
        &format!("{}/api/v1/sessions/{id}/changes", ts.base()),
        &ts.host(),
        &refs,
        None,
    )
    .await;
    assert_eq!(status, 422, "{}", String::from_utf8_lossy(&body));
    ts.shutdown().await;
}
