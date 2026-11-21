//! Terminal WebSocket protocol.

use std::sync::Arc;

use bytes::Bytes;
use futures_util::{Sink, SinkExt, StreamExt};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::header::{
    HeaderValue, CONNECTION, SEC_WEBSOCKET_ACCEPT, SEC_WEBSOCKET_KEY, SEC_WEBSOCKET_VERSION,
    UPGRADE,
};
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;
use tokio_tungstenite::tungstenite::protocol::Role;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use super::error::{ApiError, ApiResult};
use super::{AppState, Body};
use crate::registry::Registry;

#[derive(Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
enum TerminalRequest {
    Resize {
        cols: std::num::NonZeroU16,
        rows: std::num::NonZeroU16,
    },
}

pub fn is_upgrade(req: &Request<Incoming>) -> bool {
    req.headers()
        .get(UPGRADE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
}

pub async fn upgrade_terminal(
    state: Arc<AppState>,
    mut req: Request<Incoming>,
    session_id: String,
    view_only: bool,
) -> ApiResult<Response<Body>> {
    let registry = state.registry.clone().ok_or_else(ApiError::internal)?;

    if registry.get(&session_id).await.is_none() {
        return Err(ApiError::not_found("No session with that id."));
    }

    let key = req
        .headers()
        .get(SEC_WEBSOCKET_KEY)
        .ok_or_else(|| ApiError::bad_request("Missing Sec-WebSocket-Key."))?;
    let version = req
        .headers()
        .get(SEC_WEBSOCKET_VERSION)
        .and_then(|v| v.to_str().ok());
    if version != Some("13") {
        return Err(ApiError::bad_request("Unsupported WebSocket version."));
    }
    if !is_upgrade(&req) {
        return Err(ApiError::bad_request("Expected Upgrade: websocket."));
    }

    let accept = derive_accept_key(key.as_bytes());
    tokio::spawn(async move {
        match hyper::upgrade::on(&mut req).await {
            Ok(upgraded) => {
                let ws =
                    WebSocketStream::from_raw_socket(TokioIo::new(upgraded), Role::Server, None)
                        .await;
                if let Err(e) = run_terminal_socket(registry, session_id, view_only, ws).await {
                    eprintln!("syncaidync: terminal ws: {e}");
                }
            }
            Err(e) => eprintln!("syncaidync: websocket upgrade: {e}"),
        }
    });

    let mut res = Response::new(Full::new(Bytes::new()).boxed());
    *res.status_mut() = StatusCode::SWITCHING_PROTOCOLS;
    res.headers_mut()
        .insert(CONNECTION, HeaderValue::from_static("Upgrade"));
    res.headers_mut()
        .insert(UPGRADE, HeaderValue::from_static("websocket"));
    if let Ok(v) = HeaderValue::from_str(&accept) {
        res.headers_mut().insert(SEC_WEBSOCKET_ACCEPT, v);
    }
    Ok(res)
}

async fn run_terminal_socket<S>(
    registry: Arc<Registry>,
    session_id: String,
    view_only: bool,
    ws: WebSocketStream<S>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (mut sink, mut source) = ws.split();

    let mut live_rx = if let Some(term) = registry.terminal(&session_id).await {
        Some(attach(&mut sink, &term, !view_only).await?)
    } else {
        let log = registry
            .home_path()
            .join("logs")
            .join(format!("{session_id}.log"));
        let replay = crate::pty::read_log(&log, !view_only).await;
        // Replay at the grid the agent drew for. Older logs have no size; the client fits its own.
        let size = crate::pty::read_log_size(&log).await;
        send_replay(&mut sink, size, &replay).await?;
        None
    };

    'socket: loop {
        let mut rx = match live_rx.take() {
            Some(rx) => rx,
            None => {
                let code = registry
                    .get(&session_id)
                    .await
                    .and_then(|s| s.exit_code)
                    .unwrap_or(0);
                sink.send(text_msg(json!({ "t": "exit", "code": code })))
                    .await?;
                let mut poll = tokio::time::interval(std::time::Duration::from_millis(400));
                loop {
                    tokio::select! {
                        msg = source.next() => match msg {
                            Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return Ok(()),
                            Some(Ok(_)) => {}
                        },
                        _ = poll.tick() => {
                            if let Some(term) = registry.terminal(&session_id).await {
                                break attach(&mut sink, &term, false).await?;
                            }
                            if registry.get(&session_id).await.is_none() {
                                return Ok(());
                            }
                        }
                    }
                }
            }
        };

        loop {
            tokio::select! {
                msg = source.next() => match msg {
                    Some(Ok(Message::Binary(data))) => {
                        if !view_only {
                            if let Some(term) = registry.terminal(&session_id).await {
                                let _ = term.write(&data).await;
                            }
                        }
                    }
                    Some(Ok(Message::Text(text))) if !view_only => {
                        if let Ok(TerminalRequest::Resize { cols, rows }) = serde_json::from_str(text.as_str()) {
                            if let Some(term) = registry.terminal(&session_id).await {
                                let (cols, rows) = (cols.get(), rows.get());
                                if term.resize(rows, cols).await.is_ok() {
                                    sink.send(text_msg(json!({ "t": "size", "cols": cols, "rows": rows }))).await?;
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return Ok(()),
                    Some(Ok(_)) => {}
                },
                chunk = rx.recv() => match chunk {
                    Ok(chunk) => sink.send(Message::Binary(chunk)).await?,
                    Err(broadcast::error::RecvError::Closed) => continue 'socket,
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        // Reconnect to replay the current buffer after missing output.
                        sink.send(Message::Close(None)).await?;
                        return Ok(());
                    }
                }
            }
        }
    }
}

async fn attach<S>(
    sink: &mut S,
    term: &crate::pty::Terminal,
    full_history: bool,
) -> Result<broadcast::Receiver<Bytes>, S::Error>
where
    S: Sink<Message> + Unpin,
{
    let (rows, cols) = term.size().await;
    let (replay, rx) = term.snapshot_and_subscribe(full_history).await;
    send_replay(sink, Some((rows, cols)), &replay).await?;
    Ok(rx)
}

async fn send_replay<S>(
    sink: &mut S,
    size: Option<(u16, u16)>,
    replay: &[u8],
) -> Result<(), S::Error>
where
    S: Sink<Message> + Unpin,
{
    let size = match size {
        Some((rows, cols)) => json!({ "t": "size", "cols": cols, "rows": rows }),
        None => json!({ "t": "size" }),
    };
    sink.send(text_msg(size)).await?;
    for chunk in replay.chunks(64 * 1024) {
        sink.send(Message::Binary(Bytes::copy_from_slice(chunk)))
            .await?;
    }
    sink.send(text_msg(json!({ "t": "replay_done" }))).await
}

fn text_msg(v: serde_json::Value) -> Message {
    Message::Text(v.to_string().into())
}
