mod common;

use std::time::Duration;

use bytes::Bytes;
use common::{request, start_server, TestServer};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

async fn create_shell(ts: &TestServer) -> serde_json::Value {
    let body = serde_json::to_vec(&json!({
        "agent": "shell",
        "workspace": { "kind": "scratch" },
        "title": "test shell"
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

async fn open_terminal(
    ts: &TestServer,
    id: &str,
    view: bool,
) -> WebSocketStream<tokio::net::TcpStream> {
    use tokio::net::TcpStream;
    use tokio_tungstenite::client_async;

    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let path = if view {
        format!("/api/v1/sessions/{id}/terminal?mode=view")
    } else {
        format!("/api/v1/sessions/{id}/terminal")
    };
    let uri = format!("ws://127.0.0.1:{}{path}", ts.port());
    let mut req = uri.into_client_request().expect("client request");
    let headers = req.headers_mut();
    headers.insert(
        http::header::AUTHORIZATION,
        format!("Bearer {}", ts.key()).parse().unwrap(),
    );
    headers.insert(
        http::header::ORIGIN,
        format!("http://127.0.0.1:{}", ts.port()).parse().unwrap(),
    );
    let stream = TcpStream::connect(format!("127.0.0.1:{}", ts.port()))
        .await
        .unwrap();
    let (ws, resp) = client_async(req, stream).await.expect("ws handshake");
    assert_eq!(resp.status(), 101);
    ws
}

async fn read_until(ws: &mut WebSocketStream<tokio::net::TcpStream>, needle: &[u8], ms: u64) {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(ms);
    let mut buf = Vec::new();
    while !buf.windows(needle.len()).any(|w| w == needle) {
        if tokio::time::Instant::now() > deadline {
            panic!(
                "timeout waiting for {:?}; got {}",
                String::from_utf8_lossy(needle),
                String::from_utf8_lossy(&buf)
            );
        }
        match tokio::time::timeout(Duration::from_millis(400), ws.next()).await {
            Ok(Some(Ok(Message::Binary(b)))) => buf.extend_from_slice(&b),
            Ok(Some(Ok(Message::Text(_)))) => {}
            Ok(Some(Ok(_))) => {}
            Ok(Some(Err(e))) => panic!("ws error: {e}"),
            Ok(None) => panic!("ws closed; got {}", String::from_utf8_lossy(&buf)),
            Err(_) => continue,
        }
    }
}

#[tokio::test]
async fn create_attach_echo_stop_resume() {
    let ts = start_server().await;
    let session = create_shell(&ts).await;
    assert_eq!(session["state"], "launching");
    assert_eq!(session["live"], true);
    let id = session["id"].as_str().unwrap();

    let mut ws = open_terminal(&ts, id, false).await;
    // size + replay_done
    let first = ws.next().await.unwrap().unwrap();
    assert!(matches!(first, Message::Text(_)));
    // may get binary replay then replay_done
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(2), ws.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if let Message::Text(t) = msg {
            if t.contains("replay_done") {
                break;
            }
        }
    }

    // Malformed resize messages leave the connection usable.
    for text in [
        "null",
        r#"{"t":"resize","cols":65536,"rows":24}"#,
        r#"{"t":"resize","cols":80}"#,
    ] {
        ws.send(Message::Text(text.into())).await.unwrap();
    }

    ws.send(Message::Binary(Bytes::from_static(
        b"echo marker-$((6*7))\r",
    )))
    .await
    .unwrap();
    read_until(&mut ws, b"marker-42", 5000).await;

    // Stop
    let (status, _, body) = request(
        "POST",
        &format!("{}/api/v1/sessions/{id}/stop", ts.base()),
        &ts.host(),
        &[
            ("Authorization", &format!("Bearer {}", ts.key())),
            ("Origin", &format!("http://127.0.0.1:{}", ts.port())),
        ],
        Some(b"{}"),
    )
    .await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    let stopped: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(stopped["state"], "ended");
    assert_eq!(stopped["live"], false);

    // Let the stopped PTY finish tearing down before racing resumes.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Concurrent requests may only start one replacement process.
    let resume_url = format!("{}/api/v1/sessions/{id}/resume", ts.base());
    let host = ts.host();
    let authorization = format!("Bearer {}", ts.key());
    let origin = format!("http://127.0.0.1:{}", ts.port());
    let headers = [
        ("Authorization", authorization.as_str()),
        ("Origin", origin.as_str()),
    ];
    let (first, second) = tokio::join!(
        request("POST", &resume_url, &host, &headers, Some(b"{}")),
        request("POST", &resume_url, &host, &headers, Some(b"{}")),
    );
    let mut statuses = [first.0, second.0];
    statuses.sort_unstable();
    assert_eq!(
        statuses,
        [200, 409],
        "first={} second={}",
        String::from_utf8_lossy(&first.2),
        String::from_utf8_lossy(&second.2)
    );

    // The original socket gets a new replay as well as live output after resume.
    tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(msg) = ws.next().await {
            if matches!(msg.unwrap(), Message::Text(t) if t.contains("replay_done")) {
                return;
            }
        }
        panic!("terminal socket closed before replaying the replacement terminal");
    })
    .await
    .unwrap();
    ws.send(Message::Binary(Bytes::from_static(
        b"echo resumed-$((6*7))\r",
    )))
    .await
    .unwrap();
    read_until(&mut ws, b"resumed-42", 5000).await;

    // Remove
    let (status, _, _) = request(
        "DELETE",
        &format!("{}/api/v1/sessions/{id}?workspace=remove", ts.base()),
        &ts.host(),
        &[
            ("Authorization", &format!("Bearer {}", ts.key())),
            ("Origin", &format!("http://127.0.0.1:{}", ts.port())),
        ],
        None,
    )
    .await;
    assert_eq!(status, 204);

    ts.shutdown().await;
}

#[tokio::test]
async fn restart_keeps_ended_session() {
    let ts = start_server().await;
    let session = create_shell(&ts).await;
    let id = session["id"].as_str().unwrap().to_string();
    let home = ts.home.clone();
    let key = ts.key().to_string();

    let (status, _, _) = request(
        "POST",
        &format!("{}/api/v1/sessions/{id}/stop", ts.base()),
        &ts.host(),
        &[
            ("Authorization", &format!("Bearer {key}")),
            ("Origin", &format!("http://127.0.0.1:{}", ts.port())),
        ],
        Some(b"{}"),
    )
    .await;
    assert_eq!(status, 200);
    // Flush
    tokio::time::sleep(Duration::from_millis(400)).await;
    ts.shutdown().await;

    // Restart on same home
    let config = syncaidync::config::Config {
        home: home.clone(),
        port: 0,
        key: key.clone(),
        roots: Vec::new(),
        web_dir: Some(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web")),
        no_open: true,
        parent_pid: None,
        dev_shell: true,
    };
    let server = syncaidync::http::serve(config).await.unwrap();
    let port = server.addr.port();
    let host = format!("127.0.0.1:{port}");
    let base = format!("http://{host}");
    tokio::time::sleep(Duration::from_millis(30)).await;

    let (status, _, body) = request(
        "GET",
        &format!("{base}/api/v1/sessions"),
        &host,
        &[("Authorization", &format!("Bearer {key}"))],
        None,
    )
    .await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let found = v["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id)
        .expect("session present");
    assert_eq!(found["state"], "ended");
    assert_eq!(found["live"], false);

    server.shutdown().await;
}

#[tokio::test]
async fn corrupt_sessions_json_starts() {
    let home = std::env::temp_dir().join(format!(
        "syncaidync-corrupt-{}",
        syncaidync::ids::random_hex(4)
    ));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("sessions.json"), b"{not json").unwrap();
    let config = syncaidync::config::Config {
        home: home.clone(),
        port: 0,
        key: "b".repeat(32),
        roots: Vec::new(),
        web_dir: Some(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web")),
        no_open: true,
        parent_pid: None,
        dev_shell: true,
    };
    let server = syncaidync::http::serve(config).await.unwrap();
    let (status, _, body) = request(
        "GET",
        &format!("http://127.0.0.1:{}/api/v1/sessions", server.addr.port()),
        &format!("127.0.0.1:{}", server.addr.port()),
        &[("Authorization", &format!("Bearer {}", server.key))],
        None,
    )
    .await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["sessions"].as_array().unwrap().len(), 0);
    assert!(home.read_dir().unwrap().any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .contains("broken")));
    server.shutdown().await;
    let _ = std::fs::remove_dir_all(home);
}

#[tokio::test]
async fn archive_ended_session_and_restore_it() {
    let ts = start_server().await;
    let session = create_shell(&ts).await;
    let id = session["id"].as_str().unwrap();
    let base = ts.base();
    let host = ts.host();
    let authorization = format!("Bearer {}", ts.key());
    let origin = base.clone();
    let post = |action: &str, body: &'static [u8]| {
        let url = format!("{base}/api/v1/sessions/{id}/{action}");
        let headers = [
            ("Authorization", authorization.as_str()),
            ("Origin", origin.as_str()),
            ("Content-Type", "application/json"),
        ];
        let host = &host;
        async move { request("POST", &url, host, &headers, Some(body)).await }
    };

    assert_eq!(post("archive", br#"{"archived":true}"#).await.0, 409);
    assert_eq!(post("stop", b"{}").await.0, 200);
    let (status, _, body) = post("archive", br#"{"archived":true}"#).await;
    assert_eq!(status, 200);
    let archived: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(archived["archived"], true);
    assert_eq!(archived["state"], "ended");
    assert_eq!(archived["workspace"], session["workspace"]);
    assert_eq!(post("resume", b"{}").await.0, 409);

    let path = ts.home.join("sessions.json");
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if std::fs::read(&path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                .is_some_and(|file| file["sessions"][0]["archived"] == true)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();

    let (status, _, body) = post("archive", br#"{"archived":false}"#).await;
    assert_eq!(status, 200);
    let restored: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(restored["archived"], false);
    assert_eq!(restored["state"], "ended");
    assert_eq!(post("resume", b"{}").await.0, 200);
    ts.shutdown().await;
}

#[tokio::test]
async fn editable_terminal_replays_output_older_than_the_live_ring() {
    let ts = start_server().await;
    let session = create_shell(&ts).await;
    let id = session["id"].as_str().unwrap();
    let mut ws = open_terminal(&ts, id, false).await;
    ws.send(Message::Binary(Bytes::from_static(
        b"awk 'BEGIN { print \"history-\" 123; for (i=0;i<4400;i++) printf \"%079d\\n\", i; print \"finished-\" 456 }'\r",
    )))
    .await
    .unwrap();
    read_until(&mut ws, b"finished-456", 10000).await;

    let mut reattached = open_terminal(&ts, id, false).await;
    read_until(&mut reattached, b"history-123", 5000).await;
}

async fn next_text(ws: &mut WebSocketStream<tokio::net::TcpStream>) -> serde_json::Value {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(2), ws.next())
            .await
            .expect("timeout waiting for a text message")
            .expect("socket closed")
            .unwrap();
        if let Message::Text(t) = msg {
            return serde_json::from_str(t.as_str()).unwrap();
        }
    }
}

#[tokio::test]
async fn ended_terminal_replays_at_its_last_size() {
    let ts = start_server().await;
    let session = create_shell(&ts).await;
    let id = session["id"].as_str().unwrap();

    let mut ws = open_terminal(&ts, id, false).await;
    while next_text(&mut ws).await["t"] != "replay_done" {}
    ws.send(Message::Text(
        r#"{"t":"resize","cols":137,"rows":41}"#.into(),
    ))
    .await
    .unwrap();
    let resized = next_text(&mut ws).await;
    assert_eq!(
        (resized["cols"].as_u64(), resized["rows"].as_u64()),
        (Some(137), Some(41))
    );

    let (status, _, body) = request(
        "POST",
        &format!("{}/api/v1/sessions/{id}/stop", ts.base()),
        &ts.host(),
        &[
            ("Authorization", &format!("Bearer {}", ts.key())),
            ("Origin", &format!("http://127.0.0.1:{}", ts.port())),
        ],
        Some(b"{}"),
    )
    .await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    tokio::time::sleep(Duration::from_millis(100)).await;

    for view in [true, false] {
        let mut ended = open_terminal(&ts, id, view).await;
        let size = next_text(&mut ended).await;
        assert_eq!(size["t"], "size");
        assert_eq!(
            (size["cols"].as_u64(), size["rows"].as_u64()),
            (Some(137), Some(41))
        );
    }
    ts.shutdown().await;
}
