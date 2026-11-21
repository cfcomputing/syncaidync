mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

use common::{request, start_server, TestServer};
use serde_json::json;

async fn create_shell(ts: &TestServer) -> serde_json::Value {
    let body = serde_json::to_vec(&json!({
        "agent": "shell",
        "workspace": { "kind": "scratch" },
        "title": "files test"
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

fn auth(ts: &TestServer) -> [(&'static str, String); 3] {
    [
        ("Authorization", format!("Bearer {}", ts.key())),
        ("Content-Type", "application/json".into()),
        ("Origin", format!("http://127.0.0.1:{}", ts.port())),
    ]
}

fn auth_refs<'a>(headers: &'a [(&'static str, String); 3]) -> [(&'static str, &'a str); 3] {
    [
        (headers[0].0, headers[0].1.as_str()),
        (headers[1].0, headers[1].1.as_str()),
        (headers[2].0, headers[2].1.as_str()),
    ]
}

async fn api_get(ts: &TestServer, path: &str) -> (u16, serde_json::Value) {
    let h = auth(ts);
    let refs = auth_refs(&h);
    let (status, _, body) = request(
        "GET",
        &format!("{}{path}", ts.base()),
        &ts.host(),
        &refs,
        None,
    )
    .await;
    let v = serde_json::from_slice(&body).unwrap_or(json!({}));
    (status, v)
}

async fn api_put(ts: &TestServer, path: &str, body: serde_json::Value) -> (u16, serde_json::Value) {
    let h = auth(ts);
    let refs = auth_refs(&h);
    let bytes = serde_json::to_vec(&body).unwrap();
    let (status, _, resp) = request(
        "PUT",
        &format!("{}{path}", ts.base()),
        &ts.host(),
        &refs,
        Some(&bytes),
    )
    .await;
    let v = serde_json::from_slice(&resp).unwrap_or(json!({}));
    (status, v)
}

async fn api_post(
    ts: &TestServer,
    path: &str,
    body: serde_json::Value,
) -> (u16, serde_json::Value) {
    let h = auth(ts);
    let refs = auth_refs(&h);
    let bytes = serde_json::to_vec(&body).unwrap();
    let (status, _, resp) = request(
        "POST",
        &format!("{}{path}", ts.base()),
        &ts.host(),
        &refs,
        Some(&bytes),
    )
    .await;
    let v = serde_json::from_slice(&resp).unwrap_or(json!({}));
    (status, v)
}

/// Returns (status, content-type, content-security-policy, body).
async fn api_get_raw(ts: &TestServer, path: &str) -> (u16, String, String, Vec<u8>) {
    let h = auth(ts);
    let refs = auth_refs(&h);
    let (status, headers, body) = request(
        "GET",
        &format!("{}{path}", ts.base()),
        &ts.host(),
        &refs,
        None,
    )
    .await;
    let ct = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    let csp = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-security-policy"))
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    (status, ct, csp, body)
}

#[tokio::test]
async fn list_read_write_stale_force_and_limits() {
    let ts = start_server().await;
    let session = create_shell(&ts).await;
    let id = session["id"].as_str().unwrap();
    let root = PathBuf::from(session["workspace"]["path"].as_str().unwrap());

    fs::write(root.join("hello.txt"), "hello").unwrap();
    fs::create_dir_all(root.join("sub")).unwrap();
    fs::write(root.join("sub/nested.txt"), "nest").unwrap();
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(root.join("hello.txt")).unwrap().permissions();
        perms.set_mode(0o640);
        fs::set_permissions(root.join("hello.txt"), perms).unwrap();
    }

    let (status, tree) = api_get(&ts, &format!("/api/v1/sessions/{id}/tree?path=")).await;
    assert_eq!(status, 200, "{tree}");
    let names: Vec<_> = tree["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"hello.txt"));
    assert!(names.contains(&"sub"));

    let (status, file) = api_get(&ts, &format!("/api/v1/sessions/{id}/file?path=hello.txt")).await;
    assert_eq!(status, 200, "{file}");
    assert_eq!(file["text"], "hello");
    let rev = file["revision"].as_str().unwrap().to_string();

    let (status, wrote) = api_put(
        &ts,
        &format!("/api/v1/sessions/{id}/file"),
        json!({
            "path": "hello.txt",
            "text": "hello world",
            "revision": rev,
            "force": false
        }),
    )
    .await;
    assert_eq!(status, 200, "{wrote}");
    let new_rev = wrote["revision"].as_str().unwrap().to_string();
    assert_ne!(new_rev, rev);
    assert_eq!(
        fs::read_to_string(root.join("hello.txt")).unwrap(),
        "hello world"
    );
    #[cfg(unix)]
    {
        let mode = fs::metadata(root.join("hello.txt"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o640);
    }

    let (status, stale) = api_put(
        &ts,
        &format!("/api/v1/sessions/{id}/file"),
        json!({
            "path": "hello.txt",
            "text": "stale attempt",
            "revision": rev,
            "force": false
        }),
    )
    .await;
    assert_eq!(status, 409, "{stale}");
    assert_eq!(stale["error"]["code"], "stale");

    let (status, forced) = api_put(
        &ts,
        &format!("/api/v1/sessions/{id}/file"),
        json!({
            "path": "hello.txt",
            "text": "forced",
            "revision": rev,
            "force": true
        }),
    )
    .await;
    assert_eq!(status, 200, "{forced}");
    assert_eq!(
        fs::read_to_string(root.join("hello.txt")).unwrap(),
        "forced"
    );

    let (status, created) = api_put(
        &ts,
        &format!("/api/v1/sessions/{id}/file"),
        json!({
            "path": "sub/new.txt",
            "text": "brand new",
            "revision": "",
            "force": false
        }),
    )
    .await;
    assert_eq!(status, 200, "{created}");
    assert_eq!(
        fs::read_to_string(root.join("sub/new.txt")).unwrap(),
        "brand new"
    );

    let (status, missing) = api_put(
        &ts,
        &format!("/api/v1/sessions/{id}/file"),
        json!({
            "path": "nope/dir/x.txt",
            "text": "no",
            "revision": "",
            "force": false
        }),
    )
    .await;
    assert_eq!(status, 404, "{missing}");

    // Over 1 MiB
    let big = "x".repeat(1024 * 1024 + 1);
    fs::write(root.join("big.txt"), &big).unwrap();
    let (status, too_big) = api_get(&ts, &format!("/api/v1/sessions/{id}/file?path=big.txt")).await;
    assert_eq!(status, 422, "{too_big}");

    // Non-UTF-8
    fs::write(root.join("bin.dat"), [0xff, 0xfe, 0xfd]).unwrap();
    let (status, bad_utf8) =
        api_get(&ts, &format!("/api/v1/sessions/{id}/file?path=bin.dat")).await;
    assert_eq!(status, 422, "{bad_utf8}");

    // Find in plain folder
    let (status, found) = api_get(&ts, &format!("/api/v1/sessions/{id}/find?q=nested")).await;
    assert_eq!(status, 200, "{found}");
    let paths = found["paths"].as_array().unwrap();
    assert!(paths.iter().any(|p| p.as_str() == Some("sub/nested.txt")));

    ts.shutdown().await;
}

#[tokio::test]
async fn find_in_git_repo() {
    let ts = start_server().await;
    let session = create_shell(&ts).await;
    let id = session["id"].as_str().unwrap();
    let root = PathBuf::from(session["workspace"]["path"].as_str().unwrap());

    assert!(Command::new("git")
        .args(["init"])
        .current_dir(&root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .status()
        .unwrap()
        .success());
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
    fs::write(root.join("README.md"), "# hi").unwrap();
    assert!(Command::new("git")
        .args(["add", "."])
        .current_dir(&root)
        .status()
        .unwrap()
        .success());

    let (status, found) = api_get(&ts, &format!("/api/v1/sessions/{id}/find?q=main")).await;
    assert_eq!(status, 200, "{found}");
    let paths = found["paths"].as_array().unwrap();
    assert!(paths.iter().any(|p| p.as_str() == Some("src/main.rs")));

    ts.shutdown().await;
}

#[tokio::test]
async fn markdown_render_and_media() {
    let ts = start_server().await;
    let session = create_shell(&ts).await;
    let id = session["id"].as_str().unwrap();
    let root = PathBuf::from(session["workspace"]["path"].as_str().unwrap());

    fs::create_dir_all(root.join("docs/img")).unwrap();
    fs::write(root.join("docs/img/a.png"), b"\x89PNG\r\n\x1a\n").unwrap();
    fs::write(root.join("docs/readme.md"), "# Hi\n\n![d](./img/a.png)\n").unwrap();

    let (status, rendered) = api_post(
        &ts,
        &format!("/api/v1/sessions/{id}/markdown"),
        json!({
            "text": "# Hi\n\n![d](./img/a.png)\n",
            "base_path": "docs/readme.md"
        }),
    )
    .await;
    assert_eq!(status, 200, "{rendered}");
    let html = rendered["html"].as_str().unwrap();
    assert!(html.contains("<h1>"), "{html}");
    assert!(
        html.contains(&format!("/api/v1/sessions/{id}/media?path=docs/img/a.png")),
        "{html}"
    );

    let (status, ct, csp, body) = api_get_raw(
        &ts,
        &format!("/api/v1/sessions/{id}/media?path=docs/img/a.png"),
    )
    .await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    assert!(ct.starts_with("image/png"), "{ct}");
    assert_eq!(body, b"\x89PNG\r\n\x1a\n");
    assert!(csp.contains("sandbox"), "{csp}");

    // An SVG opened directly must not be able to run script on this origin.
    fs::write(
        root.join("docs/img/x.svg"),
        "<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>",
    )
    .unwrap();
    let (status, ct, csp, _) = api_get_raw(
        &ts,
        &format!("/api/v1/sessions/{id}/media?path=docs/img/x.svg"),
    )
    .await;
    assert_eq!(status, 200);
    assert!(ct.starts_with("image/svg+xml"), "{ct}");
    assert!(csp.contains("default-src 'none'"), "{csp}");
    assert!(csp.contains("sandbox"), "{csp}");

    let (status, bad) = api_get(
        &ts,
        &format!("/api/v1/sessions/{id}/media?path=docs/readme.md"),
    )
    .await;
    assert_eq!(status, 422, "{bad}");

    ts.shutdown().await;
}

#[tokio::test]
async fn margins_round_trip_and_index() {
    let ts = start_server().await;
    let session = create_shell(&ts).await;
    let id = session["id"].as_str().unwrap();
    let root = PathBuf::from(session["workspace"]["path"].as_str().unwrap());
    fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();

    let mid = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
    let (status, saved) = api_put(
        &ts,
        &format!("/api/v1/sessions/{id}/margins/{mid}"),
        json!({
            "id": mid,
            "path": "main.rs",
            "start": 1,
            "end": 1,
            "quote": "fn main() {}",
            "title": "Entry",
            "body": "Hello **world**\n",
            "media": []
        }),
    )
    .await;
    assert_eq!(status, 200, "{saved}");
    assert_eq!(saved["title"], "Entry");
    assert!(saved["body_html"].as_str().unwrap().contains("<strong>"));

    let (status, listed) =
        api_get(&ts, &format!("/api/v1/sessions/{id}/margins?path=main.rs")).await;
    assert_eq!(status, 200, "{listed}");
    assert_eq!(listed["margins"].as_array().unwrap().len(), 1);

    let index_md = fs::read_to_string(root.join(".syncaidync/margins/INDEX.md")).unwrap();
    assert!(index_md.contains("Entry"));
    assert!(root.join(".syncaidync/.gitignore").exists());

    let media = root.join(format!(".syncaidync/margins/media/{mid}"));
    fs::create_dir(&media).unwrap();
    fs::write(
        media.join("note.svg"),
        "<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>",
    )
    .unwrap();
    let (status, ct, csp, _) = api_get_raw(
        &ts,
        &format!("/api/v1/sessions/{id}/margins/{mid}/media/note.svg"),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(ct, "image/svg+xml");
    assert!(
        csp.contains("default-src 'none'") && csp.contains("sandbox"),
        "{csp}"
    );

    let (status, _, _) = {
        let h = auth(&ts);
        let refs = auth_refs(&h);
        request(
            "DELETE",
            &format!("{}/api/v1/sessions/{id}/margins/{mid}", ts.base()),
            &ts.host(),
            &refs,
            None,
        )
        .await
    };
    assert_eq!(status, 204);

    ts.shutdown().await;
}
