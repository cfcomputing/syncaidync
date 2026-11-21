//! Repo discovery endpoints.

use hyper::StatusCode;
use serde_json::json;

use crate::http::error::{ApiError, ApiResult};
use crate::http::json::{json_response, parse_query, query_get};
use crate::http::{AppState, Body};
use crate::workspaces::expand_tilde;

pub async fn list_repos(state: &AppState) -> ApiResult<hyper::Response<Body>> {
    let home = state.config.home.clone();
    let roots = state.config.roots.clone();
    let repos = tokio::task::spawn_blocking(move || crate::discovery::list_repos(&roots, &home))
        .await
        .map_err(|_| ApiError::internal())?;
    json_response(StatusCode::OK, &json!({ "repos": repos }))
}

pub async fn list_folders(state: &AppState, query: &str) -> ApiResult<hyper::Response<Body>> {
    let pairs = parse_query(query);
    let path = query_get(&pairs, "path").map(|s| s.to_string());
    let roots = state.config.roots.clone();
    let listing = tokio::task::spawn_blocking(move || {
        crate::discovery::list_folders(&roots, path.as_deref())
    })
    .await
    .map_err(|_| ApiError::internal())?
    .map_err(ApiError::bad_request)?;
    json_response(StatusCode::OK, &listing)
}

pub async fn repo_branches(state: &AppState, query: &str) -> ApiResult<hyper::Response<Body>> {
    let pairs = parse_query(query);
    let path = query_get(&pairs, "path")
        .ok_or_else(|| ApiError::bad_request("Query parameter 'path' is required."))?;
    let path = expand_tilde(path);
    if !path.is_dir() {
        return Err(ApiError::bad_request("That path is not a folder."));
    }
    if !crate::git::is_repo(&path).await {
        return Err(ApiError::bad_request(
            "That folder is not a git repository.",
        ));
    }
    let top = crate::git::toplevel(&path)
        .await
        .map_err(|e| ApiError::unprocessable(e.message))?;
    // Allow any git repo the user typed, or one from the scan.
    let _ = state;
    let default = crate::git::default_branch(&top)
        .await
        .map_err(|e| ApiError::unprocessable(e.message))?;
    let current = crate::git::current_branch(&top).await.unwrap_or_default();
    let branches = crate::git::branches(&top)
        .await
        .map_err(|e| ApiError::unprocessable(e.message))?;
    json_response(
        StatusCode::OK,
        &json!({
            "default": default,
            "current": current,
            "branches": branches,
        }),
    )
}

pub async fn history(state: &AppState) -> ApiResult<hyper::Response<Body>> {
    let reg = state.registry.as_ref().ok_or_else(ApiError::internal)?;
    let sessions = reg.list().await;
    let items: Vec<_> = state
        .history
        .items()
        .await
        .into_iter()
        .filter(|item| {
            !sessions.iter().any(|s| {
                s.agent == item.agent && s.agent_session.as_deref() == Some(&item.agent_session)
            })
        })
        .take(200)
        .collect();
    json_response(StatusCode::OK, &json!({ "items": items }))
}

#[derive(serde::Deserialize)]
struct ImportBody {
    agent: syncaidync_core::AgentId,
    agent_session: String,
}

pub async fn import(
    state: &AppState,
    req: hyper::Request<hyper::body::Incoming>,
) -> ApiResult<hyper::Response<Body>> {
    let body: ImportBody = crate::http::json::read_json(req).await?;
    let item = state
        .history
        .items()
        .await
        .into_iter()
        .find(|item| item.agent == body.agent && item.agent_session == body.agent_session)
        .ok_or_else(|| ApiError::not_found("That past session could not be found."))?;
    let reg = state.registry.as_ref().ok_or_else(ApiError::internal)?;
    let session = reg.import_history(item).await.map_err(|e| e.into_api())?;
    json_response(StatusCode::CREATED, &session)
}
