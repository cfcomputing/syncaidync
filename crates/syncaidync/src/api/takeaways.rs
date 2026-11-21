//! Takeaway suggest / save routes.

use std::sync::Arc;

use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use serde::Deserialize;
use syncaidync_core::{AgentId, Takeaway};

use crate::http::error::{ApiError, ApiResult};
use crate::http::json::{json_response, read_json};
use crate::http::{AppState, Body};
use crate::takeaways;

fn registry(state: &AppState) -> ApiResult<Arc<crate::registry::Registry>> {
    state.registry.clone().ok_or_else(ApiError::internal)
}

pub async fn destinations(state: &AppState, id: &str) -> ApiResult<Response<Body>> {
    let reg = registry(state)?;
    let session = reg
        .get(id)
        .await
        .ok_or_else(|| ApiError::not_found("No session with that id."))?;
    let dest = takeaways::destinations_for(&session, &state.config.home).await;
    json_response(StatusCode::OK, &dest)
}

#[derive(Deserialize)]
struct SuggestBody {
    agent: AgentId,
    #[serde(default)]
    model: Option<String>,
}

pub async fn suggest(
    state: &AppState,
    id: &str,
    req: Request<Incoming>,
) -> ApiResult<Response<Body>> {
    let reg = registry(state)?;
    let session = reg
        .get(id)
        .await
        .ok_or_else(|| ApiError::not_found("No session with that id."))?;
    let body: SuggestBody = read_json(req).await?;
    let model = body.model.as_deref().filter(|m| !m.is_empty());
    let takeaways = takeaways::suggest(&session, &state.config.home, body.agent, model).await?;
    json_response(
        StatusCode::OK,
        &serde_json::json!({ "takeaways": takeaways }),
    )
}

#[derive(Deserialize)]
struct SaveBody {
    takeaways: Vec<Takeaway>,
}

pub async fn save(state: &AppState, id: &str, req: Request<Incoming>) -> ApiResult<Response<Body>> {
    let reg = registry(state)?;
    let session = reg
        .get(id)
        .await
        .ok_or_else(|| ApiError::not_found("No session with that id."))?;
    let body: SaveBody = read_json(req).await?;
    let written = takeaways::save(&session, &state.config.home, &body.takeaways).await?;
    json_response(StatusCode::OK, &serde_json::json!({ "written": written }))
}
