//! Shared helpers for integration tests.

#![allow(dead_code)]

use std::path::PathBuf;
use std::time::Duration;

use syncaidync::config::Config;
use syncaidync::http::{serve, RunningServer};

pub struct TestServer {
    pub server: RunningServer,
    pub home: PathBuf,
}

impl TestServer {
    pub fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.server.addr.port())
    }

    pub fn port(&self) -> u16 {
        self.server.addr.port()
    }

    pub fn key(&self) -> &str {
        &self.server.key
    }

    pub fn host(&self) -> String {
        format!("127.0.0.1:{}", self.port())
    }

    pub async fn shutdown(self) {
        self.server.shutdown().await;
    }
}

pub async fn start_server() -> TestServer {
    let roots = match std::env::var("SYNCAIDYNC_ROOTS") {
        Ok(s) if !s.is_empty() => s
            .split(':')
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .collect(),
        _ => Vec::new(),
    };
    start_server_with_roots(roots).await
}

/// Start a server with explicit roots. Prefer this over setting `SYNCAIDYNC_ROOTS`,
/// which is process-wide and races between tests running in parallel.
pub async fn start_server_with_roots(roots: Vec<PathBuf>) -> TestServer {
    let home = tempfile_dir("syncaidync-home");
    let web = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web");
    let key = "a".repeat(32);
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
    tokio::time::sleep(Duration::from_millis(20)).await;
    TestServer { server, home }
}

fn tempfile_dir(prefix: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let unique = format!(
        "{prefix}-{}-{}",
        std::process::id(),
        syncaidync::ids::random_hex(4)
    );
    p.push(unique);
    std::fs::create_dir_all(&p).unwrap();
    p
}

pub async fn request(
    method: &str,
    url: &str,
    host: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> (u16, Vec<(String, String)>, Vec<u8>) {
    let mut last_err = None;
    for _ in 0..3 {
        match request_once(method, url, host, headers, body).await {
            Ok(r) => return r,
            Err(e) => {
                last_err = Some(e);
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        }
    }
    panic!("request failed: {last_err:?}");
}

async fn request_once(
    method: &str,
    url: &str,
    host: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> std::io::Result<(u16, Vec<(String, String)>, Vec<u8>)> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    let url = url::parse_loose(url);
    let mut stream = TcpStream::connect(&url.addr).await?;
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n",
        path = url.path_query
    );
    if let Some(body) = body {
        req.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    stream.write_all(req.as_bytes()).await?;
    if let Some(body) = body {
        // For oversized bodies the server may close early after Content-Length check.
        let _ = stream.write_all(body).await;
    }
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await?;
    Ok(parse_http_response(&buf))
}

fn parse_http_response(raw: &[u8]) -> (u16, Vec<(String, String)>, Vec<u8>) {
    let sep = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("http header end");
    let head = std::str::from_utf8(&raw[..sep]).unwrap();
    let body = raw[sep + 4..].to_vec();
    let mut lines = head.lines();
    let status: u16 = lines
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let mut headers = Vec::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    if headers
        .iter()
        .any(|(k, v)| k == "transfer-encoding" && v.contains("chunked"))
    {
        return (status, headers, decode_chunked(&body));
    }
    (status, headers, body)
}

fn decode_chunked(mut data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(pos) = data.windows(2).position(|w| w == b"\r\n") {
        let len_str = std::str::from_utf8(&data[..pos]).unwrap_or("0");
        let len = usize::from_str_radix(len_str.trim(), 16).unwrap_or(0);
        data = &data[pos + 2..];
        if len == 0 || data.len() < len {
            break;
        }
        out.extend_from_slice(&data[..len]);
        data = &data[len..];
        if data.starts_with(b"\r\n") {
            data = &data[2..];
        }
    }
    out
}

mod url {
    pub struct Parts {
        pub addr: String,
        pub path_query: String,
    }

    pub fn parse_loose(url: &str) -> Parts {
        let rest = url.strip_prefix("http://").expect("http url");
        let (hostport, path) = rest.split_once('/').unwrap_or((rest, ""));
        let path_query = if path.is_empty() {
            "/".into()
        } else {
            format!("/{path}")
        };
        Parts {
            addr: hostport.to_string(),
            path_query,
        }
    }
}
