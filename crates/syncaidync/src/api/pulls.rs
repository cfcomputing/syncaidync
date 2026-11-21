//! Pull request HTTP routes.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use serde::Deserialize;
use syncaidync_core::{build_prompt, parse_reply, AgentId, ReviewNote, READPLAN_BUDGET};

use crate::changes::{self, ChangeSource};
use crate::files;
use crate::http::error::{ApiError, ApiResult};
use crate::http::json::{empty, json_response, parse_query, query_get, read_json};
use crate::http::{AppState, Body};
use crate::oneshot;
use crate::reviews::Reviews;

fn pulls(state: &AppState) -> ApiResult<Arc<crate::pulls::Pulls>> {
    state.pulls.clone().ok_or_else(ApiError::internal)
}

fn reviews(state: &AppState) -> ApiResult<Arc<Reviews>> {
    state.reviews.clone().ok_or_else(ApiError::internal)
}

fn registry(state: &AppState) -> ApiResult<Arc<crate::registry::Registry>> {
    state.registry.clone().ok_or_else(ApiError::internal)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub async fn list(state: &AppState) -> ApiResult<Response<Body>> {
    let list = pulls(state)?.list().await;
    json_response(StatusCode::OK, &serde_json::json!({ "pulls": list }))
}

#[derive(Deserialize)]
struct OpenBody {
    #[serde(rename = "ref")]
    ref_text: String,
}

pub async fn open(state: &AppState, req: Request<Incoming>) -> ApiResult<Response<Body>> {
    let body: OpenBody = read_json(req).await?;
    let pull = pulls(state)?.open(&body.ref_text).await?;
    json_response(StatusCode::OK, &pull)
}

pub async fn get_changes(state: &AppState, id: &str, query: &str) -> ApiResult<Response<Body>> {
    let pairs = parse_query(query);
    let refresh = query_get(&pairs, "refresh")
        .map(|v| v == "1")
        .unwrap_or(false);
    let cs = pulls(state)?
        .changes(id, refresh, reviews(state)?.as_ref())
        .await?;
    json_response(StatusCode::OK, &cs)
}

pub async fn get_review(state: &AppState, id: &str) -> ApiResult<Response<Body>> {
    let p = pulls(state)?;
    if p.get(id).await.is_none() {
        return Err(ApiError::not_found("No pull request with that id."));
    }
    let review = reviews(state)?.get(&Reviews::pull_key(id)).await;
    json_response(StatusCode::OK, &review)
}

#[derive(Deserialize)]
struct PutReview {
    #[serde(default)]
    reviewed: BTreeMap<String, String>,
    #[serde(default)]
    notes: Vec<ReviewNote>,
    #[serde(default)]
    overall: String,
}

pub async fn put_review(
    state: &AppState,
    id: &str,
    req: Request<Incoming>,
) -> ApiResult<Response<Body>> {
    let p = pulls(state)?;
    if p.get(id).await.is_none() {
        return Err(ApiError::not_found("No pull request with that id."));
    }
    let body: PutReview = read_json(req).await?;
    let revs = reviews(state)?;
    let review = revs
        .put_pull(id, body.reviewed, body.notes, body.overall, now_ms())
        .await
        .map_err(ApiError::bad_request)?;
    let _ = p.update_progress(id, revs.as_ref()).await;
    json_response(StatusCode::OK, &review)
}

#[derive(Deserialize)]
struct PlanBody {
    agent: AgentId,
    #[serde(default)]
    model: Option<String>,
}

pub async fn post_plan(
    state: &AppState,
    id: &str,
    req: Request<Incoming>,
) -> ApiResult<Response<Body>> {
    let p = pulls(state)?;
    let pull = p
        .get(id)
        .await
        .ok_or_else(|| ApiError::not_found("No pull request with that id."))?;
    let body: PlanBody = read_json(req).await?;
    if matches!(body.agent, AgentId::Shell) {
        return Err(ApiError::bad_request(
            "Pick Claude, Codex, Pi, or Cursor Agent for a read-through plan.",
        ));
    }
    let cs = changes::gather_async(ChangeSource::Commits {
        repo: std::path::PathBuf::from(&pull.clone),
        base: pull.base_commit.clone(),
        head: pull.head_commit.clone(),
        base_label: pull.base_ref.clone(),
        head_label: pull.head_ref.clone(),
    })
    .await
    .map_err(changes::ChangesError::into_api)?;
    if cs.files.is_empty() {
        return Err(ApiError::unprocessable("There are no changes to plan."));
    }
    let prompt = build_prompt(&cs, READPLAN_BUDGET);
    let model = body.model.as_deref().filter(|m| !m.is_empty());
    let reply = oneshot::run(body.agent, model, &prompt)
        .await
        .map_err(oneshot::OneshotError::into_api)?;
    let plan = parse_reply(&reply, &cs, now_ms(), body.agent.as_str(), model)
        .map_err(|m| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "unavailable", m))?;
    reviews(state)?
        .set_plan_pull(id, plan.clone(), now_ms())
        .await;
    json_response(StatusCode::OK, &plan)
}

pub async fn draft(state: &AppState, id: &str) -> ApiResult<Response<Body>> {
    let url = pulls(state)?.draft(id, reviews(state)?.as_ref()).await?;
    json_response(StatusCode::OK, &serde_json::json!({ "url": url }))
}

#[derive(Deserialize)]
struct SessionBody {
    agent: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    autonomy: bool,
}

pub async fn session(
    state: &AppState,
    id: &str,
    req: Request<Incoming>,
) -> ApiResult<Response<Body>> {
    let body: SessionBody = read_json(req).await?;
    let agent = match body.agent.as_str() {
        "claude" => AgentId::Claude,
        "codex" => AgentId::Codex,
        "pi" => AgentId::Pi,
        "cursor" => AgentId::Cursor,
        "shell" => AgentId::Shell,
        _ => {
            return Err(ApiError::bad_request(format!(
                "Unknown agent: {}",
                body.agent
            )))
        }
    };
    let session = pulls(state)?
        .start_session(id, &registry(state)?, agent, body.model, body.autonomy)
        .await?;
    json_response(StatusCode::CREATED, &session)
}

#[derive(Deserialize)]
struct PatchBody {
    status: String,
}

pub async fn patch(
    state: &AppState,
    id: &str,
    req: Request<Incoming>,
) -> ApiResult<Response<Body>> {
    let body: PatchBody = read_json(req).await?;
    let pull = pulls(state)?.set_status(id, &body.status).await?;
    json_response(StatusCode::OK, &pull)
}

pub async fn delete(state: &AppState, id: &str) -> ApiResult<Response<Body>> {
    pulls(state)?.delete(id, reviews(state)?.as_ref()).await?;
    Ok(empty(StatusCode::NO_CONTENT))
}

/// Read a text file from the pull's local clone at the PR head (for markdown View).
pub async fn get_file(state: &AppState, id: &str, query: &str) -> ApiResult<Response<Body>> {
    let pairs = parse_query(query);
    let path = query_get(&pairs, "path")
        .ok_or_else(|| ApiError::bad_request("Missing path query parameter."))?
        .to_string();
    let p = pulls(state)?;
    let pull = p
        .get(id)
        .await
        .ok_or_else(|| ApiError::not_found("No pull request with that id."))?;
    let clone = PathBuf::from(&pull.clone);
    if !clone.is_dir() {
        return Err(ApiError::not_found("This pull's local clone is gone."));
    }
    let head = pull.head_commit.clone();
    let path_c = path.clone();
    let text = tokio::task::spawn_blocking(move || -> ApiResult<String> {
        // Same path rules as workspace files (no `..`, no `.git`); do not require the
        // path to exist on disk — the blob comes from `git show` at the PR head.
        files::validate_rel(&path_c).map_err(|e| e.into_api())?;
        let spec = format!("{head}:{path_c}");
        let out = std::process::Command::new("git")
            .env_remove("SYNCAIDYNC_KEY")
            .args(["show", &spec])
            .current_dir(&clone)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .output()
            .map_err(|_| ApiError::internal())?;
        if !out.status.success() {
            return Err(ApiError::not_found(
                "That file is not in this pull at the head commit.",
            ));
        }
        if out.stdout.len() > 1024 * 1024 {
            return Err(ApiError::unprocessable(
                "This file is larger than 1 MiB; SyncAiDync opens files up to 1 MiB.",
            ));
        }
        String::from_utf8(out.stdout)
            .map_err(|_| ApiError::unprocessable("This file is not UTF-8 text."))
    })
    .await
    .map_err(|_| ApiError::internal())??;
    json_response(
        StatusCode::OK,
        &serde_json::json!({ "path": path, "text": text }),
    )
}
