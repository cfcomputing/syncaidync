//! Goal routes: draft checks before a session starts, and steer a session's goal after.

use std::sync::Arc;

use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use serde::Deserialize;
use serde_json::json;
use syncaidync_core::goal::{build_draft_prompt, parse_draft, Criterion, DraftAnswer, Limits};
use syncaidync_core::AgentId;

use crate::goals::{self, Trigger};
use crate::http::error::{ApiError, ApiResult};
use crate::http::json::{json_response, read_json};
use crate::http::{AppState, Body};
use crate::oneshot;
use crate::registry::{CreateWorkspace, Registry};

fn registry(state: &AppState) -> ApiResult<Arc<Registry>> {
    state.registry.clone().ok_or_else(ApiError::internal)
}

#[derive(Deserialize)]
struct DraftBody {
    agent: AgentId,
    #[serde(default)]
    model: Option<String>,
    text: String,
    criteria: Vec<Criterion>,
    #[serde(default)]
    answers: Vec<DraftAnswer>,
    workspace: CreateWorkspace,
}

pub async fn draft(state: &AppState, req: Request<Incoming>) -> ApiResult<Response<Body>> {
    let body: DraftBody = read_json(req).await?;
    if matches!(body.agent, AgentId::Shell) {
        return Err(ApiError::bad_request(
            "Pick Claude, Codex, Pi, or Cursor Agent to draft checks.",
        ));
    }
    let text = body.text.trim();
    if text.is_empty() || text.chars().count() > 500 {
        return Err(ApiError::unprocessable(
            "Write the goal first (at most 500 characters).",
        ));
    }
    let criteria: Vec<Criterion> = body
        .criteria
        .into_iter()
        .filter(|c| !c.text.trim().is_empty() && !c.id.trim().is_empty())
        .take(syncaidync_core::goal::MAX_CRITERIA)
        .collect();
    if criteria.is_empty() {
        return Err(ApiError::unprocessable("Add at least one done criterion."));
    }
    let answers: Vec<DraftAnswer> = body.answers.into_iter().take(10).collect();
    let _ = state;
    let context = goals::draft_context(&body.workspace)
        .await
        .map_err(|e| e.into_api())?;
    let prompt = build_draft_prompt(text, &criteria, &answers, &context);
    let model = body.model.as_deref().filter(|m| !m.is_empty());
    let reply = oneshot::run(body.agent, model, &prompt)
        .await
        .map_err(oneshot::OneshotError::into_api)?;
    let mut draft = parse_draft(&reply, &criteria)
        .map_err(|m| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "unavailable", m))?;
    let mut guards: Vec<String> = Vec::new();
    for g in draft.checks.iter().flat_map(|c| c.guards.iter()) {
        if !guards.contains(g) {
            guards.push(g.clone());
        }
    }
    draft.missing_guards = goals::missing_guards(&body.workspace, guards).await;
    json_response(StatusCode::OK, &draft)
}

pub async fn get(state: &AppState, id: &str) -> ApiResult<Response<Body>> {
    let reg = registry(state)?;
    let session = reg
        .get(id)
        .await
        .ok_or_else(|| ApiError::not_found("No session with that id."))?;
    let goal = session
        .goal
        .ok_or_else(|| ApiError::not_found("This session has no goal."))?;
    let rounds = goals::rounds(&reg.home_path(), id).await;
    json_response(StatusCode::OK, &json!({ "goal": goal, "rounds": rounds }))
}

async fn require_goal(reg: &Registry, id: &str) -> ApiResult<()> {
    let session = reg
        .get(id)
        .await
        .ok_or_else(|| ApiError::not_found("No session with that id."))?;
    if session.goal.is_none() {
        return Err(ApiError::not_found("This session has no goal."));
    }
    Ok(())
}

pub async fn run(state: &AppState, id: &str) -> ApiResult<Response<Body>> {
    let reg = registry(state)?;
    require_goal(&reg, id).await?;
    let session = goals::run_round(reg, id.to_string(), Trigger::Now)
        .await
        .map_err(|e| e.into_api())?;
    json_response(StatusCode::OK, &session)
}

#[derive(Deserialize)]
struct ContinueBody {
    #[serde(default)]
    limits: Option<Limits>,
}

pub async fn continue_goal(
    state: &AppState,
    id: &str,
    req: Request<Incoming>,
) -> ApiResult<Response<Body>> {
    let reg = registry(state)?;
    require_goal(&reg, id).await?;
    let body: ContinueBody = read_json(req).await?;
    let session = goals::continue_goal(reg, id.to_string(), body.limits)
        .await
        .map_err(|e| e.into_api())?;
    json_response(StatusCode::OK, &session)
}

#[derive(Deserialize)]
struct CheckBody {
    /// One check, or every flagged check when absent.
    #[serde(default)]
    check: Option<String>,
}

pub async fn accept(
    state: &AppState,
    id: &str,
    req: Request<Incoming>,
) -> ApiResult<Response<Body>> {
    let reg = registry(state)?;
    require_goal(&reg, id).await?;
    let body: CheckBody = read_json(req).await?;
    let session = goals::accept(reg, id.to_string(), body.check)
        .await
        .map_err(|e| e.into_api())?;
    json_response(StatusCode::OK, &session)
}

#[derive(Deserialize)]
struct ManualBody {
    check: String,
    done: bool,
}

pub async fn manual(
    state: &AppState,
    id: &str,
    req: Request<Incoming>,
) -> ApiResult<Response<Body>> {
    let reg = registry(state)?;
    require_goal(&reg, id).await?;
    let body: ManualBody = read_json(req).await?;
    let session = goals::set_manual(reg, id.to_string(), body.check, body.done)
        .await
        .map_err(|e| e.into_api())?;
    json_response(StatusCode::OK, &session)
}

pub async fn drop_goal(state: &AppState, id: &str) -> ApiResult<Response<Body>> {
    let reg = registry(state)?;
    require_goal(&reg, id).await?;
    let session = goals::drop_goal(reg, id.to_string())
        .await
        .map_err(|e| e.into_api())?;
    json_response(StatusCode::OK, &session)
}

pub async fn rewrite_report(state: &AppState, id: &str) -> ApiResult<Response<Body>> {
    let reg = registry(state)?;
    require_goal(&reg, id).await?;
    let session = goals::rewrite_report(reg, id.to_string())
        .await
        .map_err(|e| e.into_api())?;
    json_response(StatusCode::OK, &session)
}

pub async fn commit_preview(state: &AppState, id: &str) -> ApiResult<Response<Body>> {
    let reg = registry(state)?;
    require_goal(&reg, id).await?;
    let preview = goals::commit_preview(reg, id.to_string())
        .await
        .map_err(|e| e.into_api())?;
    json_response(StatusCode::OK, &preview)
}

#[derive(Deserialize)]
struct CommitBody {
    message: String,
}

pub async fn commit(
    state: &AppState,
    id: &str,
    req: Request<Incoming>,
) -> ApiResult<Response<Body>> {
    let reg = registry(state)?;
    require_goal(&reg, id).await?;
    let body: CommitBody = read_json(req).await?;
    let session = goals::commit(reg, id.to_string(), body.message)
        .await
        .map_err(|e| e.into_api())?;
    json_response(StatusCode::OK, &session)
}

#[derive(Deserialize)]
struct MoreBody {
    text: String,
    #[serde(default)]
    rounds: Option<u32>,
}

pub async fn keep_working(
    state: &AppState,
    id: &str,
    req: Request<Incoming>,
) -> ApiResult<Response<Body>> {
    let reg = registry(state)?;
    require_goal(&reg, id).await?;
    let body: MoreBody = read_json(req).await?;
    let session = goals::keep_working(reg, id.to_string(), body.text, body.rounds)
        .await
        .map_err(|e| e.into_api())?;
    json_response(StatusCode::OK, &session)
}

pub async fn merge_preview(state: &AppState, id: &str) -> ApiResult<Response<Body>> {
    let reg = registry(state)?;
    require_goal(&reg, id).await?;
    let preview = goals::merge_preview(reg, id.to_string())
        .await
        .map_err(|e| e.into_api())?;
    json_response(StatusCode::OK, &preview)
}

pub async fn merge(state: &AppState, id: &str) -> ApiResult<Response<Body>> {
    let reg = registry(state)?;
    require_goal(&reg, id).await?;
    let session = goals::merge(reg, id.to_string())
        .await
        .map_err(|e| e.into_api())?;
    json_response(StatusCode::OK, &session)
}
