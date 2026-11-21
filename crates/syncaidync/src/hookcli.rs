//! `syncaidync signal` / `usage`: tiny HTTP/1.1 client over TcpStream. Always exits 0.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// Run the signal or usage subcommand. Never returns an error to the caller for exit status —
/// prints nothing on failure and the process should exit 0.
pub fn run(kind: Kind, agent: &str) {
    let _ = run_inner(kind, agent);
}

#[derive(Clone, Copy)]
pub enum Kind {
    Signal,
    Usage,
}

fn run_inner(kind: Kind, agent: &str) -> Result<(), ()> {
    let url = std::env::var("SYNCAIDYNC_URL").map_err(|_| ())?;
    let session = std::env::var("SYNCAIDYNC_SESSION").map_err(|_| ())?;
    let token = std::env::var("SYNCAIDYNC_SIGNAL_TOKEN").map_err(|_| ())?;

    let payload = read_payload()?;
    let body = serde_json::json!({
        "agent": agent,
        "payload": payload,
    });
    let body_bytes = serde_json::to_vec(&body).map_err(|_| ())?;

    let (host, port, _) = parse_loopback_url(&url).ok_or(())?;
    let path = match kind {
        Kind::Signal => format!("/agent/v1/signal?session={session}"),
        Kind::Usage => format!("/agent/v1/usage?session={session}"),
    };

    let mut stream = TcpStream::connect_timeout(
        &format!("{host}:{port}").parse().map_err(|_| ())?,
        Duration::from_secs(2),
    )
    .map_err(|_| ())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|_| ())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|_| ())?;

    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}:{port}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body_bytes.len()
    );
    stream.write_all(req.as_bytes()).map_err(|_| ())?;
    stream.write_all(&body_bytes).map_err(|_| ())?;
    stream.flush().map_err(|_| ())?;

    let mut resp = Vec::new();
    stream.read_to_end(&mut resp).map_err(|_| ())?;
    let text = String::from_utf8_lossy(&resp);
    let Some(body_start) = text.find("\r\n\r\n") else {
        return Ok(());
    };
    let resp_body = text[body_start + 4..].trim();

    if matches!(kind, Kind::Usage) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(resp_body) {
            if let Some(line) = v.get("line").and_then(|l| l.as_str()) {
                println!("{line}");
            }
        }
    }
    Ok(())
}

fn read_payload() -> Result<serde_json::Value, ()> {
    // Prefer stdin when it is not a terminal; else last CLI argument (Codex notify form).
    let use_stdin = !atty_stdin();
    if use_stdin {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf).map_err(|_| ())?;
        let trimmed = buf.trim();
        if !trimmed.is_empty() {
            return serde_json::from_str(trimmed).map_err(|_| ());
        }
    }
    // Last argument after --agent <id>
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // signal|usage --agent <id> [payload]
    if let Some(pos) = args.iter().position(|a| a == "--agent") {
        if args.len() > pos + 2 {
            let payload = args.remove(pos + 2);
            return serde_json::from_str(&payload).map_err(|_| ());
        }
    }
    // Empty object if nothing
    Ok(serde_json::json!({}))
}

fn atty_stdin() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}

fn parse_loopback_url(url: &str) -> Option<(String, u16, String)> {
    let rest = url.strip_prefix("http://")?;
    let (hostport, path) = match rest.split_once('/') {
        Some((h, p)) => (h, format!("/{p}")),
        None => (rest, "/".into()),
    };
    let (host, port) = match hostport.split_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().ok()?),
        None => (hostport.to_string(), 80u16),
    };
    if host != "127.0.0.1" && host != "localhost" && host != "[::1]" {
        return None;
    }
    Some((host, port, path))
}
