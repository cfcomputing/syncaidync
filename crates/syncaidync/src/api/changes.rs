//! ChangeSet and review routes.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use serde::Deserialize;
use syncaidync_core::{build_prompt, parse_reply, AgentId, ReviewNote, READPLAN_BUDGET};

use crate::changes::{self, ChangeSource};
use crate::http::error::{ApiError, ApiResult};
use crate::http::json::{json_response, read_json};
use crate::http::{AppState, Body};
use crate::oneshot;

fn registry(state: &AppState) -> ApiResult<Arc<crate::registry::Registry>> {
    state.registry.clone().ok_or_else(ApiError::internal)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub async fn get_changes(state: &AppState, id: &str) -> ApiResult<Response<Body>> {
    let reg = registry(state)?;
    let session = reg
        .get(id)
        .await
        .ok_or_else(|| ApiError::not_found("No session with that id."))?;
    let root = PathBuf::from(&session.workspace.path);
    let base_commit = session.workspace.base_commit.clone().ok_or_else(|| {
        ApiError::unprocessable(
            "This folder is not a git repository, so there is nothing to compare against.",
        )
    })?;
    let base_label = session
        .workspace
        .base
        .clone()
        .unwrap_or_else(|| base_commit.chars().take(7).collect());

    let cs = changes::gather_async(ChangeSource::Folder {
        root,
        base_commit,
        base_label,
    })
    .await
    .map_err(changes::ChangesError::into_api)?;
    json_response(StatusCode::OK, &cs)
}

pub async fn get_review(state: &AppState, id: &str) -> ApiResult<Response<Body>> {
    let reg = registry(state)?;
    if reg.get(id).await.is_none() {
        return Err(ApiError::not_found("No session with that id."));
    }
    let reviews = state.reviews.as_ref().ok_or_else(ApiError::internal)?;
    let review = reviews.get(&crate::reviews::Reviews::session_key(id)).await;
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
    let reg = registry(state)?;
    if reg.get(id).await.is_none() {
        return Err(ApiError::not_found("No session with that id."));
    }
    let body: PutReview = read_json(req).await?;
    let reviews = state.reviews.as_ref().ok_or_else(ApiError::internal)?;
    let review = reviews
        .put_session(id, body.reviewed, body.notes, body.overall, now_ms())
        .await
        .map_err(ApiError::bad_request)?;
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
    let reg = registry(state)?;
    let session = reg
        .get(id)
        .await
        .ok_or_else(|| ApiError::not_found("No session with that id."))?;
    let body: PlanBody = read_json(req).await?;
    if matches!(body.agent, AgentId::Shell) {
        return Err(ApiError::bad_request(
            "Pick Claude, Codex, Pi, or Cursor Agent for a read-through plan.",
        ));
    }

    let root = PathBuf::from(&session.workspace.path);
    let base_commit = session.workspace.base_commit.clone().ok_or_else(|| {
        ApiError::unprocessable(
            "This folder is not a git repository, so there is nothing to compare against.",
        )
    })?;
    let base_label = session
        .workspace
        .base
        .clone()
        .unwrap_or_else(|| base_commit.chars().take(7).collect());

    let cs = changes::gather_async(ChangeSource::Folder {
        root,
        base_commit,
        base_label,
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

    let reviews = state.reviews.as_ref().ok_or_else(ApiError::internal)?;
    reviews.set_plan(id, plan.clone(), now_ms()).await;
    json_response(StatusCode::OK, &plan)
}
