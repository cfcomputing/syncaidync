//! Meta, update, load, and plain markdown render.

use hyper::body::Incoming;
use hyper::{Request, StatusCode};
use serde::Deserialize;
use serde_json::json;

use crate::agents_discover;
use crate::http::error::{ApiError, ApiResult};
use crate::http::json::{json_response, read_json};
use crate::http::{AppState, Body};
use crate::load;
use crate::markdown;

pub async fn meta(state: &AppState) -> ApiResult<hyper::Response<Body>> {
    let platform = if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "other"
    };
    let agents = agents_discover::list_agents(state.config.dev_shell).await;
    let body = json!({
        "name": "SyncAiDync",
        "version": env!("CARGO_PKG_VERSION"),
        "home": state.config.home,
        "platform": platform,
        "git": which("git"),
        "gh": which("gh"),
        "agents": agents,
    });
    json_response(StatusCode::OK, &body)
}

pub async fn update(state: &AppState) -> ApiResult<hyper::Response<Body>> {
    json_response(
        StatusCode::OK,
        &state.update.status(&state.config.home).await,
    )
}

/// Live CPU and memory for SyncAiDync and the sum of live agent process groups.
pub async fn load(state: &AppState) -> ApiResult<hyper::Response<Body>> {
    let pgids = if let Some(reg) = &state.registry {
        reg.live_session_pids().await
    } else {
        Vec::new()
    };
    let snap = tokio::task::spawn_blocking(move || load::snapshot(&pgids))
        .await
        .unwrap_or(load::LoadSnapshot {
            host: load::Sample {
                cpu_pct: 0.0,
                rss_bytes: 0,
            },
            agents: load::Sample {
                cpu_pct: 0.0,
                rss_bytes: 0,
            },
            live_sessions: 0,
        });
    json_response(
        StatusCode::OK,
        &json!({
            "host": {
                "cpu_pct": snap.host.cpu_pct,
                "rss_bytes": snap.host.rss_bytes,
            },
            "agents": {
                "cpu_pct": snap.agents.cpu_pct,
                "rss_bytes": snap.agents.rss_bytes,
                "sessions": snap.live_sessions,
            },
        }),
    )
}

fn which(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            for p in std::env::split_paths(&paths) {
                let candidate = p.join(name);
                if candidate.is_file() {
                    return true;
                }
            }
            false
        })
        .unwrap_or(false)
}

#[derive(Deserialize)]
struct MarkdownBody {
    text: String,
    /// When true, block tags get `data-source-line` for note placement in View.
    #[serde(default)]
    source_lines: bool,
}

/// Render markdown without session media rewriting (PR descriptions, pull Changes View).
pub async fn post_markdown(req: Request<Incoming>) -> ApiResult<hyper::Response<Body>> {
    let body: MarkdownBody = read_json(req).await?;
    if body.text.len() > 1024 * 1024 {
        return Err(ApiError::unprocessable(
            "Markdown text is too large to render (max 1 MiB).",
        ));
    }
    let html = if body.source_lines {
        markdown::to_safe_html_with_source_lines(&body.text, None)
    } else {
        markdown::to_safe_html(&body.text)
    };
    json_response(StatusCode::OK, &json!({ "html": html }))
}
