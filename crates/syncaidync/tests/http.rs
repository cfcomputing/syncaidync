mod common;

use common::{request, start_server};
use serde_json::json;

#[tokio::test]
async fn wrong_host_is_forbidden() {
    let ts = start_server().await;
    let url = format!("{}/", ts.base());
    let (status, _, body) = request("GET", &url, "evil.example", &[], None).await;
    assert_eq!(status, 403);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["error"]["code"], "bad_host");

    let meta = format!("{}/api/v1/meta", ts.base());
    let (status, _, _) = request(
        "GET",
        &meta,
        "evil.example",
        &[("Authorization", &format!("Bearer {}", ts.key()))],
        None,
    )
    .await;
    assert_eq!(status, 403);
    ts.shutdown().await;
}

#[tokio::test]
async fn auth_cookie_and_bearer() {
    let ts = start_server().await;
    let host = ts.host();
    let meta = format!("{}/api/v1/meta", ts.base());

    let (status, _, _) = request("GET", &meta, &host, &[], None).await;
    assert_eq!(status, 401);

    let (status, _, _) = request(
        "GET",
        &meta,
        &host,
        &[("Authorization", "Bearer wrong-key-wrong-key-wrong-key!!")],
        None,
    )
    .await;
    assert_eq!(status, 401);

    let (status, _, body) = request(
        "GET",
        &meta,
        &host,
        &[("Authorization", &format!("Bearer {}", ts.key()))],
        None,
    )
    .await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["name"], "SyncAiDync");

    let auth = format!("{}/api/v1/auth", ts.base());
    let payload = serde_json::to_vec(&json!({ "key": ts.key() })).unwrap();
    let (status, headers, _) = request(
        "POST",
        &auth,
        &host,
        &[
            ("Content-Type", "application/json"),
            ("Origin", &format!("http://127.0.0.1:{}", ts.port())),
        ],
        Some(&payload),
    )
    .await;
    assert_eq!(status, 204);
    let cookie = headers
        .iter()
        .find(|(k, _)| k == "set-cookie")
        .map(|(_, v)| v.clone())
        .expect("set-cookie");
    assert!(cookie.contains(&format!("syncaidync_{}=", ts.port())));

    let (status, _, _) = request("GET", &meta, &host, &[("Cookie", &cookie)], None).await;
    assert_eq!(status, 200);
    ts.shutdown().await;
}

#[tokio::test]
async fn origin_check_on_post() {
    let ts = start_server().await;
    let host = ts.host();
    let auth = format!("{}/api/v1/auth", ts.base());
    let payload = serde_json::to_vec(&json!({ "key": ts.key() })).unwrap();

    let (status, _, body) = request(
        "POST",
        &auth,
        &host,
        &[
            ("Content-Type", "application/json"),
            ("Origin", "http://evil.example"),
        ],
        Some(&payload),
    )
    .await;
    assert_eq!(status, 403);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["error"]["code"], "bad_origin");

    let (status, _, _) = request(
        "POST",
        &auth,
        &host,
        &[
            ("Content-Type", "application/json"),
            ("Origin", &format!("http://127.0.0.1:{}", ts.port())),
        ],
        Some(&payload),
    )
    .await;
    assert_eq!(status, 204);
    ts.shutdown().await;
}

#[tokio::test]
async fn load_reports_host_rss() {
    let ts = start_server().await;
    let host = ts.host();
    let url = format!("{}/api/v1/load", ts.base());
    let (status, _, body) = request(
        "GET",
        &url,
        &host,
        &[("Authorization", &format!("Bearer {}", ts.key()))],
        None,
    )
    .await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        v["host"]["rss_bytes"].as_u64().unwrap_or(0) > 0,
        "host rss missing: {v}"
    );
    assert!(v["host"]["cpu_pct"].as_f64().is_some());
    assert_eq!(v["agents"]["sessions"].as_u64(), Some(0));
    ts.shutdown().await;
}

#[tokio::test]
async fn body_too_large() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    let ts = start_server().await;
    let host = ts.host();
    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", ts.port()))
        .await
        .unwrap();
    // Claim a huge body via Content-Length; the server rejects before reading it.
    let req = format!(
        "POST /api/v1/auth HTTP/1.1\r\nHost: {host}\r\nOrigin: http://127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        ts.port(),
        4 * 1024 * 1024 + 10
    );
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf).await;
    let head = String::from_utf8_lossy(&buf);
    assert!(
        head.contains("413") || head.contains("too_large"),
        "expected 413, got: {}",
        &head[..head.len().min(200)]
    );
    ts.shutdown().await;
}

#[tokio::test]
async fn static_files_and_denials() {
    let ts = start_server().await;
    let host = ts.host();
    let (status, headers, body) =
        request("GET", &format!("{}/", ts.base()), &host, &[], None).await;
    assert_eq!(status, 200);
    let html = String::from_utf8_lossy(&body);
    assert!(html.contains("SyncAiDync"));
    assert!(headers
        .iter()
        .any(|(k, v)| k == "content-security-policy" && v.contains("default-src")));

    let (status, _, _) = request(
        "GET",
        &format!("{}/../Cargo.toml", ts.base()),
        &host,
        &[],
        None,
    )
    .await;
    assert_eq!(status, 404);

    let (status, _, _) = request(
        "GET",
        &format!("{}/test/smoke.test.js", ts.base()),
        &host,
        &[],
        None,
    )
    .await;
    assert_eq!(status, 404);
    ts.shutdown().await;
}

#[tokio::test]
async fn stream_initial_state_and_publish() {
    let ts = start_server().await;
    let host = ts.host();
    // Read the first event from one connection in a task.
    let port = ts.port();
    let key = ts.key().to_string();
    let hub = ts.server.state.hub.clone();

    let reader = tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpStream;
        let mut stream = TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .unwrap();
        let req = format!(
            "GET /api/v1/stream HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {key}\r\n\r\n"
        );
        stream.write_all(req.as_bytes()).await.unwrap();
        let mut buf = vec![0u8; 8192];
        let mut got = String::new();
        for _ in 0..20 {
            let n = stream.read(&mut buf).await.unwrap();
            if n == 0 {
                break;
            }
            got.push_str(&String::from_utf8_lossy(&buf[..n]));
            if got.contains("event: state") && got.contains("event: session") {
                break;
            }
            if got.contains("event: state") {
                // wait a bit more for published event
            }
        }
        got
    });

    tokio::time::sleep(Duration::from_millis(100)).await;
    hub.publish("session", &json!({ "id": "s1", "title": "hi" }));
    tokio::time::sleep(Duration::from_millis(100)).await;

    let got = reader.await.unwrap();
    assert!(got.contains("event: state"), "{got}");
    assert!(got.contains("\"sessions\":[]"), "{got}");
    assert!(got.contains("event: session"), "{got}");

    // Two subscribers both get a publish.
    let mut a = hub.subscribe();
    let mut b = hub.subscribe();
    hub.publish("pull", &json!({ "id": "p1" }));
    let ma = a.recv().await.unwrap();
    let mb = b.recv().await.unwrap();
    assert!(ma.contains("event: pull"));
    assert!(mb.contains("event: pull"));

    ts.shutdown().await;
}

#[tokio::test]
async fn unknown_route_json_error() {
    let ts = start_server().await;
    let (status, _, body) = request(
        "GET",
        &format!("{}/api/v1/nope", ts.base()),
        &ts.host(),
        &[("Authorization", &format!("Bearer {}", ts.key()))],
        None,
    )
    .await;
    assert_eq!(status, 404);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["error"]["code"], "not_found");
    ts.shutdown().await;
}

use std::time::Duration;
