//! Agent callback routes: signal + usage with per-session token.

use std::sync::Arc;

use hyper::body::Incoming;
use hyper::header::AUTHORIZATION;
use hyper::{Request, Response, StatusCode};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::http::auth::constant_time_eq;
use crate::http::error::{ApiError, ApiResult};
use crate::http::json::{empty, json_response, parse_query, query_get, read_json};
use crate::http::{AppState, Body};
use syncaidync_core::{adapter_for, Adapter, AgentId, ClaudeAdapter, ShellAdapter};

#[derive(Deserialize)]
struct AgentBody {
    agent: String,
    payload: Value,
}

fn registry(state: &AppState) -> ApiResult<Arc<crate::registry::Registry>> {
    state.registry.clone().ok_or_else(ApiError::internal)
}

fn parse_agent_id(s: &str) -> Option<AgentId> {
    match s {
        "claude" => Some(AgentId::Claude),
        "codex" => Some(AgentId::Codex),
        "pi" => Some(AgentId::Pi),
        "cursor" => Some(AgentId::Cursor),
        "shell" => Some(AgentId::Shell),
        _ => None,
    }
}

async fn authorize_session(
    state: &AppState,
    req: &Request<Incoming>,
    session_id: &str,
) -> ApiResult<()> {
    let auth = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let Some(token) = auth else {
        return Err(ApiError::unauthorized());
    };
    // Launch key must never work here.
    if constant_time_eq(token.as_bytes(), state.config.key.as_bytes()) {
        return Err(ApiError::unauthorized());
    }
    let reg = registry(state)?;
    let expected = reg
        .signal_token(session_id)
        .await
        .ok_or_else(ApiError::unauthorized)?;
    if !constant_time_eq(token.as_bytes(), expected.as_bytes()) {
        return Err(ApiError::unauthorized());
    }
    Ok(())
}

pub async fn signal(state: &AppState, req: Request<Incoming>) -> ApiResult<Response<Body>> {
    let q = req.uri().query().unwrap_or("");
    let pairs = parse_query(q);
    let session_id = query_get(&pairs, "session")
        .ok_or_else(|| ApiError::bad_request("Missing session query parameter."))?
        .to_string();
    authorize_session(state, &req, &session_id).await?;
    let body: AgentBody = read_json(req).await?;
    let agent = parse_agent_id(&body.agent)
        .ok_or_else(|| ApiError::bad_request(format!("Unknown agent: {}", body.agent)))?;

    if std::env::var_os("SYNCAIDYNC_DEBUG_SIGNALS").is_some_and(|v| v == "1") {
        let _ = debug_log(state, &session_id, &body.agent, &body.payload).await;
    }

    let reg = registry(state)?;
    reg.apply_agent_payload(&session_id, agent, &body.payload)
        .await
        .map_err(|e| e.into_api())?;
    Ok(empty(StatusCode::NO_CONTENT))
}

pub async fn usage(state: &AppState, req: Request<Incoming>) -> ApiResult<Response<Body>> {
    let q = req.uri().query().unwrap_or("");
    let pairs = parse_query(q);
    let session_id = query_get(&pairs, "session")
        .ok_or_else(|| ApiError::bad_request("Missing session query parameter."))?
        .to_string();
    authorize_session(state, &req, &session_id).await?;
    let body: AgentBody = read_json(req).await?;
    let agent = parse_agent_id(&body.agent)
        .ok_or_else(|| ApiError::bad_request(format!("Unknown agent: {}", body.agent)))?;

    if std::env::var_os("SYNCAIDYNC_DEBUG_SIGNALS").is_some_and(|v| v == "1") {
        let _ = debug_log(state, &session_id, &body.agent, &body.payload).await;
    }

    let usage = match agent {
        AgentId::Claude => ClaudeAdapter.usage(&body.payload),
        AgentId::Shell => {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
            ShellAdapter::new(shell).usage(&body.payload)
        }
        other => adapter_for(other).and_then(|a| a.usage(&body.payload)),
    };

    let reg = registry(state)?;
    if let Some(u) = usage {
        let _ = reg.set_usage(&session_id, u).await;
    }

    let line = match reg.get(&session_id).await.and_then(|s| s.usage) {
        Some(u) => usage_line(&u),
        None => "SyncAiDync".into(),
    };
    json_response(StatusCode::OK, &json!({ "line": line }))
}

fn usage_line(u: &syncaidync_core::Usage) -> String {
    let mut parts = vec!["SyncAiDync".to_string()];
    if let Some(c) = u.cost_usd {
        parts.push(format!("${c:.2}"));
    } else if let Some(t) = u.tokens {
        parts.push(format_tokens(t));
    }
    if let Some(p) = u.context_pct {
        parts.push(format!("{p}%"));
    }
    parts.join(" · ")
}

fn format_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M tokens", n as f64 / 1_000_000.0)
    } else if n >= 10_000 {
        format!("{}k tokens", (n + 500) / 1_000)
    } else if n >= 1_000 {
        format!("{:.1}k tokens", n as f64 / 1_000.0)
    } else {
        format!("{n} tokens")
    }
}

async fn debug_log(state: &AppState, session: &str, agent: &str, payload: &Value) {
    let home = state
        .registry
        .as_ref()
        .map(|r| r.home_path())
        .unwrap_or_else(|| state.config.home.clone());
    let path = home.join("logs").join("signals.jsonl");
    let _ = tokio::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new("."))).await;
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let line = json!({
        "at": at,
        "session": session,
        "agent": agent,
        "payload": payload,
    });
    if let Ok(mut s) = serde_json::to_string(&line) {
        s.push('\n');
        use tokio::io::AsyncWriteExt;
        if let Ok(mut f) = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
        {
            let _ = f.write_all(s.as_bytes()).await;
            let _ = f.flush().await;
        }
    }
}
