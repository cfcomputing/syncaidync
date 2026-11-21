mod common;

use common::{request, start_server, TestServer};
use serde_json::json;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio::sync::Mutex as AsyncMutex;

static ENV_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());

fn git(cwd: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .args(args)
            .current_dir(cwd)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .status()
            .unwrap()
            .success(),
        "git {args:?}"
    );
}

fn git_out(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn write_script(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

/// Bare remote + clone with github.com URL rewritten to the bare path.
fn setup_pr_repos(home: &Path) -> (PathBuf, PathBuf, String, String) {
    let bare = home.join("remote.git");
    let clone = home.join("code").join("testrepo");
    fs::create_dir_all(clone.parent().unwrap()).unwrap();
    assert!(Command::new("git")
        .args(["init", "--bare", "-b", "main"])
        .arg(&bare)
        .status()
        .unwrap()
        .success());

    let seed = home.join("seed");
    fs::create_dir_all(&seed).unwrap();
    git(&seed, &["init", "-b", "main"]);
    git(&seed, &["config", "user.email", "t@example.com"]);
    git(&seed, &["config", "user.name", "Test"]);
    fs::write(seed.join("README"), "base\n").unwrap();
    fs::create_dir_all(seed.join("src")).unwrap();
    fs::write(seed.join("src/a.rs"), "fn a() {}\n").unwrap();
    git(&seed, &["add", "."]);
    git(&seed, &["commit", "-m", "base"]);
    git(&seed, &["remote", "add", "origin", &bare.to_string_lossy()]);
    git(&seed, &["push", "-u", "origin", "main"]);

    // Feature branch commit for the PR head.
    git(&seed, &["checkout", "-b", "feat"]);
    fs::write(seed.join("src/a.rs"), "fn a() {}\nfn b() {}\n").unwrap();
    fs::write(seed.join("src/b.rs"), "fn b() {}\n").unwrap();
    git(&seed, &["add", "."]);
    git(&seed, &["commit", "-m", "feat"]);
    let head = git_out(&seed, &["rev-parse", "HEAD"]);
    git(&seed, &["push", "origin", "feat"]);
    // Publish as pull/7/head on the bare remote.
    git(&seed, &["push", "origin", "HEAD:refs/pull/7/head"]);
    git(&seed, &["checkout", "main"]);
    let base = git_out(&seed, &["rev-parse", "HEAD"]);

    // Clone for discovery with a github-looking remote rewritten to bare.
    assert!(Command::new("git")
        .args(["clone", &bare.to_string_lossy(), &clone.to_string_lossy()])
        .status()
        .unwrap()
        .success());
    git(
        &clone,
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/testowner/testrepo.git",
        ],
    );
    git(
        &clone,
        &[
            "config",
            &format!("url.{}.insteadOf", bare.to_string_lossy()),
            "https://github.com/testowner/testrepo.git",
        ],
    );
    // Ensure pull ref exists locally via fetch once.
    git(
        &clone,
        &[
            "fetch",
            "origin",
            "+refs/pull/7/head:refs/pull/7/head",
            "+refs/heads/main:refs/remotes/origin/main",
        ],
    );

    (clone, bare, head, base)
}

fn install_fake_gh(home: &Path, head_oid: &str, record: &Path) -> PathBuf {
    let script = home.join("fake-gh");
    let record_s = record.to_string_lossy();
    write_script(
        &script,
        &format!(
            r#"#!/bin/sh
set -e
if [ "$1" = "auth" ]; then
  exit 0
fi
if [ "$1" = "pr" ] && [ "$2" = "view" ]; then
  cat <<'JSON'
{{"number":7,"title":"Add b","author":{{"login":"octo"}},"body":"hello **world**","state":"OPEN","isDraft":false,"baseRefName":"main","headRefName":"feat","headRefOid":"{head_oid}","url":"https://github.com/testowner/testrepo/pull/7"}}
JSON
  exit 0
fi
if [ "$1" = "repo" ] && [ "$2" = "clone" ]; then
  dest="$4"
  shift 5
  git clone "$@" "file://$(dirname "$0")/remote.git" "$dest"
  git -C "$dest" remote set-url origin https://github.com/testowner/testrepo.git
  git -C "$dest" config "url.file://$(dirname "$0")/remote.git.insteadOf" https://github.com/testowner/testrepo.git
  exit 0
fi
if [ "$1" = "api" ]; then
  cat > "{record_s}"
  echo '{{"html_url":"https://github.com/testowner/testrepo/pull/7#pullrequestreview-1"}}'
  exit 0
fi
echo "unexpected: $*" >&2
exit 2
"#
        ),
    );
    script
}

#[tokio::test]
async fn open_changes_draft_delete_with_fake_gh() {
    let _guard = ENV_LOCK.lock().await;
    let home = {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "syncaidync-pulls-{}-{}",
            std::process::id(),
            syncaidync::ids::random_hex(4)
        ));
        fs::create_dir_all(&p).unwrap();
        p
    };

    let (clone, _bare, head, _base) = setup_pr_repos(&home);
    let record = home.join("draft-payload.json");
    let fake = install_fake_gh(&home, &head, &record);
    std::env::set_var("SYNCAIDYNC_BIN_GH", &fake);
    std::env::set_var(
        "SYNCAIDYNC_ROOTS",
        clone.parent().unwrap().to_string_lossy().as_ref(),
    );

    let ts = start_server_with_home(home.clone()).await;
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

    // Open
    let body = serde_json::to_vec(&json!({ "ref": "testowner/testrepo#7" })).unwrap();
    let (status, _, resp) = request(
        "POST",
        &format!("{}/api/v1/pulls/open", ts.base()),
        &ts.host(),
        &refs,
        Some(&body),
    )
    .await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&resp));
    let pull: serde_json::Value = serde_json::from_slice(&resp).unwrap();
    let pid = pull["id"].as_str().unwrap().to_string();
    assert_eq!(pull["number"], 7);
    assert_eq!(pull["owner"], "testowner");
    assert!(pull["body_html"].as_str().unwrap().contains("<strong>"));

    // Private refs exist
    let head_ref = git_out(&clone, &["rev-parse", "refs/syncaidync/pull/7/head"]);
    assert_eq!(head_ref, head);
    assert!(git_out(&clone, &["rev-parse", "refs/syncaidync/pull/7/base"]).len() >= 7);

    // Changes
    let (status, _, resp) = request(
        "GET",
        &format!("{}/api/v1/pulls/{pid}/changes", ts.base()),
        &ts.host(),
        &refs,
        None,
    )
    .await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&resp));
    let cs: serde_json::Value = serde_json::from_slice(&resp).unwrap();
    let paths: Vec<&str> = cs["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert!(paths.contains(&"src/a.rs"));
    assert!(paths.contains(&"src/b.rs"));

    // Put a note and draft
    let note_body = serde_json::to_vec(&json!({
        "reviewed": {},
        "notes": [{
            "id": "n1",
            "path": "src/b.rs",
            "side": "new",
            "start": 1,
            "end": 1,
            "quote": "",
            "text": "looks good",
            "sent": false,
            "created_at": 1
        }],
        "overall": "Ship it"
    }))
    .unwrap();
    let (status, _, _) = request(
        "PUT",
        &format!("{}/api/v1/pulls/{pid}/review", ts.base()),
        &ts.host(),
        &refs,
        Some(&note_body),
    )
    .await;
    assert_eq!(status, 200);

    let (status, _, resp) = request(
        "POST",
        &format!("{}/api/v1/pulls/{pid}/draft", ts.base()),
        &ts.host(),
        &refs,
        Some(b"{}"),
    )
    .await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&resp));
    let draft: serde_json::Value = serde_json::from_slice(&resp).unwrap();
    assert!(draft["url"].as_str().unwrap().contains("pullrequestreview"));
    let payload: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&record).unwrap()).unwrap();
    assert!(payload.get("event").is_none());
    assert_eq!(payload["commit_id"], head);
    assert!(payload["body"].as_str().unwrap().contains("Ship it"));
    assert!(!payload["comments"].as_array().unwrap().is_empty());

    // Refresh after new commit on pull ref
    git(&clone, &["fetch", "origin", "feat:feat"]);
    // Amend on seed path is gone — push another commit via clone
    git(&clone, &["checkout", "-B", "feat", "refs/pull/7/head"]);
    fs::write(clone.join("src/c.rs"), "fn c() {}\n").unwrap();
    git(&clone, &["add", "."]);
    git(&clone, &["commit", "-m", "more"]);
    let new_head = git_out(&clone, &["rev-parse", "HEAD"]);
    git(&clone, &["push", "origin", "HEAD:refs/pull/7/head"]);
    git(&clone, &["checkout", "main"]);

    // Update fake gh head oid
    install_fake_gh(&home, &new_head, &record);

    let (status, _, resp) = request(
        "GET",
        &format!("{}/api/v1/pulls/{pid}/changes?refresh=1", ts.base()),
        &ts.host(),
        &refs,
        None,
    )
    .await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&resp));
    let cs2: serde_json::Value = serde_json::from_slice(&resp).unwrap();
    assert_eq!(cs2["head"]["commit"], new_head);

    // Delete
    let (status, _, _) = request(
        "DELETE",
        &format!("{}/api/v1/pulls/{pid}", ts.base()),
        &ts.host(),
        &refs,
        None,
    )
    .await;
    assert_eq!(status, 204);
    let gone = Command::new("git")
        .args(["rev-parse", "refs/syncaidync/pull/7/head"])
        .current_dir(&clone)
        .output()
        .unwrap();
    assert!(!gone.status.success());

    std::env::remove_var("SYNCAIDYNC_BIN_GH");
    std::env::remove_var("SYNCAIDYNC_ROOTS");
    ts.shutdown().await;
}

#[tokio::test]
async fn review_session_works_without_a_local_clone() {
    let _guard = ENV_LOCK.lock().await;
    let home = std::env::temp_dir().join(format!(
        "syncaidync-pull-session-{}",
        syncaidync::ids::random_hex(8)
    ));
    let (_, _, head, base) = setup_pr_repos(&home);
    let empty_root = home.join("empty-root");
    fs::create_dir_all(&empty_root).unwrap();
    let fake = install_fake_gh(&home, &head, &home.join("draft-payload.json"));
    std::env::set_var("SYNCAIDYNC_BIN_GH", &fake);
    std::env::set_var("SYNCAIDYNC_ROOTS", empty_root);
    let ts = start_server_with_home(home.clone()).await;
    let authorization = format!("Bearer {}", ts.key());
    let origin = format!("http://127.0.0.1:{}", ts.port());
    let headers = [
        ("Authorization", authorization.as_str()),
        ("Origin", origin.as_str()),
        ("Content-Type", "application/json"),
    ];

    let (status, _, response) = request(
        "POST",
        &format!("{}/api/v1/pulls/open", ts.base()),
        &ts.host(),
        &headers,
        Some(br#"{"ref":"testowner/testrepo#7"}"#),
    )
    .await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&response));
    let pull: serde_json::Value = serde_json::from_slice(&response).unwrap();
    let pull_id = pull["id"].as_str().unwrap();
    let clone = Path::new(pull["clone"].as_str().unwrap());
    assert!(clone.starts_with(home.join("pull-clones")));
    assert_eq!(
        git_out(clone, &["rev-parse", "--is-shallow-repository"]),
        "true"
    );

    let session_url = format!("{}/api/v1/pulls/{pull_id}/session", ts.base());
    let mut session_id = String::new();
    for _ in 0..2 {
        let (status, _, response) = request(
            "POST",
            &session_url,
            &ts.host(),
            &headers,
            Some(br#"{"agent":"shell","autonomy":false}"#),
        )
        .await;
        assert_eq!(status, 201, "{}", String::from_utf8_lossy(&response));
        let session: serde_json::Value = serde_json::from_slice(&response).unwrap();
        let id = session["id"].as_str().unwrap();
        assert!(session_id.is_empty() || id == session_id);
        session_id = id.to_owned();
        assert_eq!(session["pull"], pull_id);
        assert_eq!(session["workspace"]["base_commit"], base);
        let worktree = Path::new(session["workspace"]["path"].as_str().unwrap());
        assert_eq!(git_out(worktree, &["rev-parse", "HEAD"]), head);
    }

    let (status, _, response) = request(
        "GET",
        &format!("{}/api/v1/sessions/{session_id}/changes", ts.base()),
        &ts.host(),
        &headers,
        None,
    )
    .await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&response));
    let changes: serde_json::Value = serde_json::from_slice(&response).unwrap();
    let paths: Vec<_> = changes["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| &file["path"])
        .collect();
    assert_eq!(paths, ["src/a.rs", "src/b.rs"]);

    std::env::remove_var("SYNCAIDYNC_BIN_GH");
    std::env::remove_var("SYNCAIDYNC_ROOTS");
    ts.shutdown().await;
}

#[tokio::test]
async fn pull_margins_live_on_clone() {
    let _guard = ENV_LOCK.lock().await;
    let home = {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "syncaidync-pull-margins-{}-{}",
            std::process::id(),
            syncaidync::ids::random_hex(4)
        ));
        fs::create_dir_all(&p).unwrap();
        p
    };

    let (clone, _bare, head, _base) = setup_pr_repos(&home);
    let record = home.join("draft-payload.json");
    let fake = install_fake_gh(&home, &head, &record);
    std::env::set_var("SYNCAIDYNC_BIN_GH", &fake);
    std::env::set_var(
        "SYNCAIDYNC_ROOTS",
        clone.parent().unwrap().to_string_lossy().as_ref(),
    );

    let ts = start_server_with_home(home.clone()).await;
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

    let body = serde_json::to_vec(&json!({ "ref": "testowner/testrepo#7" })).unwrap();
    let (status, _, resp) = request(
        "POST",
        &format!("{}/api/v1/pulls/open", ts.base()),
        &ts.host(),
        &refs,
        Some(&body),
    )
    .await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&resp));
    let pull: serde_json::Value = serde_json::from_slice(&resp).unwrap();
    let pid = pull["id"].as_str().unwrap();
    let clone_path = PathBuf::from(pull["clone"].as_str().unwrap());

    let mid = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
    let put_body = serde_json::to_vec(&json!({
        "id": mid,
        "path": "src/b.rs",
        "start": 1,
        "end": 1,
        "quote": "fn b() {}",
        "title": "PR note",
        "body": "See ![d](media/x.png)\n",
        "media": []
    }))
    .unwrap();
    let (status, _, resp) = request(
        "PUT",
        &format!("{}/api/v1/pulls/{pid}/margins/{mid}", ts.base()),
        &ts.host(),
        &refs,
        Some(&put_body),
    )
    .await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&resp));
    let saved: serde_json::Value = serde_json::from_slice(&resp).unwrap();
    assert_eq!(saved["title"], "PR note");
    assert!(saved["body_html"]
        .as_str()
        .unwrap()
        .contains(&format!("/api/v1/pulls/{pid}/margins/{mid}/media/x.png")));

    let (status, _, resp) = request(
        "GET",
        &format!("{}/api/v1/pulls/{pid}/margins?path=src/b.rs", ts.base()),
        &ts.host(),
        &refs,
        None,
    )
    .await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&resp));
    let listed: serde_json::Value = serde_json::from_slice(&resp).unwrap();
    assert_eq!(listed["margins"].as_array().unwrap().len(), 1);

    let index_md = fs::read_to_string(clone_path.join(".syncaidync/margins/INDEX.md")).unwrap();
    assert!(index_md.contains("PR note"));
    assert!(clone_path.join(".syncaidync/.gitignore").exists());

    let png = b"\x89PNG\r\n\x1a\n".as_slice();
    let (status, _, resp) = request(
        "PUT",
        &format!("{}/api/v1/pulls/{pid}/margins/{mid}/media/x.png", ts.base()),
        &ts.host(),
        &[
            (auth[0].0, auth[0].1.as_str()),
            ("Content-Type", "image/png"),
            (auth[2].0, auth[2].1.as_str()),
        ],
        Some(png),
    )
    .await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&resp));

    let (status, _, resp) = request(
        "GET",
        &format!("{}/api/v1/pulls/{pid}/margins/{mid}/media/x.png", ts.base()),
        &ts.host(),
        &refs,
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(resp, png);

    std::env::remove_var("SYNCAIDYNC_BIN_GH");
    std::env::remove_var("SYNCAIDYNC_ROOTS");
    ts.shutdown().await;
}

#[tokio::test]
async fn gh_missing_is_503() {
    let _guard = ENV_LOCK.lock().await;
    std::env::set_var("SYNCAIDYNC_BIN_GH", "/nonexistent/gh-binary-syncaidync");
    let ts = start_server().await;
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
    let body = serde_json::to_vec(&json!({ "ref": "o/r#1" })).unwrap();
    let (status, _, resp) = request(
        "POST",
        &format!("{}/api/v1/pulls/open", ts.base()),
        &ts.host(),
        &refs,
        Some(&body),
    )
    .await;
    assert_eq!(status, 503, "{}", String::from_utf8_lossy(&resp));
    let v: serde_json::Value = serde_json::from_slice(&resp).unwrap();
    assert!(v["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Install the GitHub CLI"));
    std::env::remove_var("SYNCAIDYNC_BIN_GH");
    ts.shutdown().await;
}

#[tokio::test]
async fn shell_metacharacters_rejected() {
    let _guard = ENV_LOCK.lock().await;
    let ts = start_server().await;
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
    let body = serde_json::to_vec(&json!({ "ref": "o/r#1;rm -rf /" })).unwrap();
    let (status, _, resp) = request(
        "POST",
        &format!("{}/api/v1/pulls/open", ts.base()),
        &ts.host(),
        &refs,
        Some(&body),
    )
    .await;
    assert_eq!(status, 400, "{}", String::from_utf8_lossy(&resp));
    ts.shutdown().await;
}

async fn start_server_with_home(home: PathBuf) -> TestServer {
    use syncaidync::config::Config;
    use syncaidync::http::serve;
    let web = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web");
    let key = "a".repeat(32);
    let roots = match std::env::var("SYNCAIDYNC_ROOTS") {
        Ok(s) if !s.is_empty() => s
            .split(':')
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .collect(),
        _ => Vec::new(),
    };
    let config = Config {
        home: home.clone(),
        port: 0,
        key,
        roots,
        web_dir: Some(web),
        no_open: true,
        parent_pid: None,
        dev_shell: true,
    };
    let server = serve(config).await.expect("serve");
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    TestServer { server, home }
}
