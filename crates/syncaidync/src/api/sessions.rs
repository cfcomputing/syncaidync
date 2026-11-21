//! Session HTTP routes.

use std::sync::Arc;

use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use serde::Deserialize;
use serde_json::json;

use crate::http::error::{ApiError, ApiResult};
use crate::http::json::{empty, json_response, read_json};
use crate::http::{ws, AppState, Body};
use crate::registry::CreateSession;

fn registry(state: &AppState) -> ApiResult<Arc<crate::registry::Registry>> {
    state.registry.clone().ok_or_else(ApiError::internal)
}

pub async fn list(state: &AppState) -> ApiResult<Response<Body>> {
    let reg = registry(state)?;
    let sessions = reg.list().await;
    json_response(StatusCode::OK, &json!({ "sessions": sessions }))
}

pub async fn create(state: &AppState, req: Request<Incoming>) -> ApiResult<Response<Body>> {
    let reg = registry(state)?;
    let body: CreateSession = read_json(req).await?;
    let session = reg.create(body).await.map_err(|e| e.into_api())?;
    json_response(StatusCode::CREATED, &session)
}

pub async fn delete(
    state: &AppState,
    id: &str,
    workspace: Option<&str>,
) -> ApiResult<Response<Body>> {
    let reg = registry(state)?;
    let remove_ws = workspace == Some("remove");
    reg.remove(id, remove_ws).await.map_err(|e| e.into_api())?;
    if let Some(reviews) = &state.reviews {
        reviews.remove_session(id).await;
    }
    Ok(empty(StatusCode::NO_CONTENT))
}

pub async fn removal(state: &AppState, id: &str) -> ApiResult<Response<Body>> {
    let reg = registry(state)?;
    let info = reg.removal_info(id).await.map_err(|e| e.into_api())?;
    json_response(StatusCode::OK, &info)
}

pub async fn resume(state: &AppState, id: &str) -> ApiResult<Response<Body>> {
    let reg = registry(state)?;
    let session = reg.resume(id).await.map_err(|e| e.into_api())?;
    json_response(StatusCode::OK, &session)
}

pub async fn stop(state: &AppState, id: &str) -> ApiResult<Response<Body>> {
    let reg = registry(state)?;
    let session = reg.stop(id).await.map_err(|e| e.into_api())?;
    json_response(StatusCode::OK, &session)
}

#[derive(Deserialize)]
struct ArchiveBody {
    archived: bool,
}

pub async fn archive(
    state: &AppState,
    id: &str,
    req: Request<Incoming>,
) -> ApiResult<Response<Body>> {
    let reg = registry(state)?;
    let body: ArchiveBody = read_json(req).await?;
    let session = reg
        .set_archived(id, body.archived)
        .await
        .map_err(|e| e.into_api())?;
    json_response(StatusCode::OK, &session)
}

#[derive(Deserialize)]
pub struct SnoozeBody {
    pub snoozed: bool,
}

pub async fn snooze(
    state: &AppState,
    id: &str,
    req: Request<Incoming>,
) -> ApiResult<Response<Body>> {
    let reg = registry(state)?;
    let body: SnoozeBody = read_json(req).await?;
    let session = reg
        .snooze(id, body.snoozed)
        .await
        .map_err(|e| e.into_api())?;
    json_response(StatusCode::OK, &session)
}

#[derive(Deserialize)]
pub struct TitleBody {
    pub title: String,
}

pub async fn title(
    state: &AppState,
    id: &str,
    req: Request<Incoming>,
) -> ApiResult<Response<Body>> {
    let reg = registry(state)?;
    let body: TitleBody = read_json(req).await?;
    let session = reg
        .rename(id, &body.title)
        .await
        .map_err(|e| e.into_api())?;
    json_response(StatusCode::OK, &session)
}

#[derive(Deserialize)]
pub struct SendBody {
    pub text: String,
    #[serde(default)]
    pub submit: bool,
}

pub async fn send(state: &AppState, id: &str, req: Request<Incoming>) -> ApiResult<Response<Body>> {
    let reg = registry(state)?;
    let body: SendBody = read_json(req).await?;
    reg.send(id, &body.text, body.submit)
        .await
        .map_err(|e| e.into_api())?;
    Ok(empty(StatusCode::NO_CONTENT))
}

pub async fn terminal(
    state: Arc<AppState>,
    id: String,
    req: Request<Incoming>,
) -> ApiResult<Response<Body>> {
    let view = req
        .uri()
        .query()
        .map(|q| {
            crate::http::json::parse_query(q)
                .iter()
                .any(|(k, v)| k == "mode" && v == "view")
        })
        .unwrap_or(false);
    crate::http::auth::check_origin(req.method(), req.headers(), state.bound_port(), true)?;
    ws::upgrade_terminal(state, req, id, view).await
}

pub async fn links(state: &AppState, id: &str) -> ApiResult<Response<Body>> {
    let session = registry(state)?
        .get(id)
        .await
        .ok_or_else(|| ApiError::not_found("No session with that id."))?;
    json_response(StatusCode::OK, &state.links.get(&session).await)
}
