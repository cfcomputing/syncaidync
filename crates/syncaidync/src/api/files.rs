//! Session workspace file routes.

use std::path::PathBuf;
use std::sync::Arc;

use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::{Request, Response, StatusCode};
use serde::Deserialize;
use serde_json::json;

use crate::files::{self, FileError};
use crate::http::error::{ApiError, ApiResult};
use crate::http::json::{json_response, parse_query, query_get, read_json};
use crate::http::{workspace_media_headers, AppState, Body};
use crate::markdown::{self, RenderContext};

fn registry(state: &AppState) -> ApiResult<Arc<crate::registry::Registry>> {
    state.registry.clone().ok_or_else(ApiError::internal)
}

async fn workspace_root(state: &AppState, id: &str) -> ApiResult<PathBuf> {
    let reg = registry(state)?;
    let session = reg
        .get(id)
        .await
        .ok_or_else(|| ApiError::not_found("No session with that id."))?;
    let path = PathBuf::from(&session.workspace.path);
    if !path.is_dir() {
        return Err(ApiError::not_found("This session's folder is gone."));
    }
    Ok(path)
}

pub async fn tree(state: &AppState, id: &str, query: &str) -> ApiResult<Response<Body>> {
    let root = workspace_root(state, id).await?;
    let pairs = parse_query(query);
    let path = query_get(&pairs, "path").unwrap_or("");
    let root_c = root.clone();
    let path = path.to_string();
    let result = tokio::task::spawn_blocking(move || files::list(&root_c, &path))
        .await
        .map_err(|_| ApiError::internal())?
        .map_err(FileError::into_api)?;
    json_response(StatusCode::OK, &result)
}

pub async fn find(state: &AppState, id: &str, query: &str) -> ApiResult<Response<Body>> {
    let root = workspace_root(state, id).await?;
    let pairs = parse_query(query);
    let q = query_get(&pairs, "q").unwrap_or("").to_string();
    let root_c = root.clone();
    let result = tokio::task::spawn_blocking(move || files::find(&root_c, &q))
        .await
        .map_err(|_| ApiError::internal())?
        .map_err(FileError::into_api)?;
    json_response(StatusCode::OK, &result)
}

pub async fn get_file(state: &AppState, id: &str, query: &str) -> ApiResult<Response<Body>> {
    let root = workspace_root(state, id).await?;
    let pairs = parse_query(query);
    let path = query_get(&pairs, "path")
        .ok_or_else(|| ApiError::bad_request("Missing path query parameter."))?
        .to_string();
    let root_c = root.clone();
    let result = tokio::task::spawn_blocking(move || files::read(&root_c, &path))
        .await
        .map_err(|_| ApiError::internal())?
        .map_err(FileError::into_api)?;
    json_response(StatusCode::OK, &result)
}

#[derive(Deserialize)]
struct WriteBody {
    path: String,
    text: String,
    revision: String,
    #[serde(default)]
    force: bool,
}

pub async fn put_file(
    state: &AppState,
    id: &str,
    req: Request<Incoming>,
) -> ApiResult<Response<Body>> {
    let root = workspace_root(state, id).await?;
    let body: WriteBody = read_json(req).await?;
    let root_c = root.clone();
    let revision = tokio::task::spawn_blocking(move || {
        files::write(&root_c, &body.path, &body.text, &body.revision, body.force)
    })
    .await
    .map_err(|_| ApiError::internal())?
    .map_err(FileError::into_api)?;
    json_response(StatusCode::OK, &json!({ "revision": revision }))
}

#[derive(Deserialize)]
struct MarkdownBody {
    text: String,
    #[serde(default)]
    base_path: Option<String>,
    /// When true, block tags get `data-source-line` for note placement in View.
    #[serde(default)]
    source_lines: bool,
}

pub async fn post_markdown(
    state: &AppState,
    id: &str,
    req: Request<Incoming>,
) -> ApiResult<Response<Body>> {
    // Ensure the session still has a workspace (same gate as other file routes).
    let _root = workspace_root(state, id).await?;
    let body: MarkdownBody = read_json(req).await?;
    if body.text.len() > 1024 * 1024 {
        return Err(ApiError::unprocessable(
            "Markdown text is too large to render (max 1 MiB).",
        ));
    }
    let html = match body
        .base_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(base) => {
            let owner = format!("sessions/{id}");
            let ctx = RenderContext {
                owner: &owner,
                base_path: base,
                margin_id: None,
            };
            if body.source_lines {
                markdown::to_safe_html_with_source_lines(&body.text, Some(&ctx))
            } else {
                markdown::to_safe_html_with_context(&body.text, Some(&ctx))
            }
        }
        None => {
            if body.source_lines {
                markdown::to_safe_html_with_source_lines(&body.text, None)
            } else {
                markdown::to_safe_html(&body.text)
            }
        }
    };
    json_response(StatusCode::OK, &json!({ "html": html }))
}

pub async fn get_media(state: &AppState, id: &str, query: &str) -> ApiResult<Response<Body>> {
    let root = workspace_root(state, id).await?;
    let pairs = parse_query(query);
    let path = query_get(&pairs, "path")
        .ok_or_else(|| ApiError::bad_request("Missing path query parameter."))?
        .to_string();
    let root_c = root.clone();
    let (bytes, content_type) =
        tokio::task::spawn_blocking(move || files::read_media(&root_c, &path))
            .await
            .map_err(|_| ApiError::internal())?
            .map_err(FileError::into_api)?;
    let mut res = Response::new(Full::new(Bytes::from(bytes)).boxed());
    *res.status_mut() = StatusCode::OK;
    workspace_media_headers(&mut res, content_type);
    Ok(res)
}
