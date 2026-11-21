//! Session and pull-clone margin routes.

use std::path::PathBuf;
use std::sync::Arc;

use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::{Request, Response, StatusCode};
use serde::Deserialize;
use serde_json::json;

use crate::files::FileError;
use crate::http::error::{ApiError, ApiResult};
use crate::http::json::{empty, json_response, parse_query, query_get, read_body, read_json};
use crate::http::{workspace_media_headers, AppState, Body};
use crate::margins::{self, Margin};

#[derive(Clone, Copy)]
pub enum MarginOwner<'a> {
    Session(&'a str),
    Pull(&'a str),
}

impl MarginOwner<'_> {
    fn api_owner(self) -> String {
        match self {
            MarginOwner::Session(id) => format!("sessions/{id}"),
            MarginOwner::Pull(id) => format!("pulls/{id}"),
        }
    }
}

fn registry(state: &AppState) -> ApiResult<Arc<crate::registry::Registry>> {
    state.registry.clone().ok_or_else(ApiError::internal)
}

fn pulls(state: &AppState) -> ApiResult<Arc<crate::pulls::Pulls>> {
    state.pulls.clone().ok_or_else(ApiError::internal)
}

async fn owner_root(state: &AppState, owner: MarginOwner<'_>) -> ApiResult<PathBuf> {
    match owner {
        MarginOwner::Session(id) => {
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
        MarginOwner::Pull(id) => {
            let p = pulls(state)?;
            let pull = p
                .get(id)
                .await
                .ok_or_else(|| ApiError::not_found("No pull request with that id."))?;
            let path = PathBuf::from(&pull.clone);
            if !path.is_dir() {
                return Err(ApiError::not_found("This pull's local clone is gone."));
            }
            Ok(path)
        }
    }
}

fn margin_json(m: &Margin, html: &str) -> serde_json::Value {
    json!({
        "id": m.id,
        "path": m.path,
        "start": m.start,
        "end": m.end,
        "quote": m.quote,
        "title": m.title,
        "body": m.body,
        "body_html": html,
        "media": m.media,
        "updated_at": m.updated_at,
    })
}

// Saving a margin updates its entry and the shared index. Serialize those transactions
// across sessions and pull views, including writes that outlive an HTTP request.
async fn margin_io<T: Send + 'static>(op: impl FnOnce() -> T + Send + 'static) -> ApiResult<T> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    tokio::task::spawn_blocking(move || {
        let _guard = LOCK.lock().map_err(|_| ApiError::internal())?;
        Ok(op())
    })
    .await
    .map_err(|_| ApiError::internal())?
}

pub async fn list(
    state: &AppState,
    owner: MarginOwner<'_>,
    query: &str,
) -> ApiResult<Response<Body>> {
    let root = owner_root(state, owner).await?;
    let pairs = parse_query(query);
    let path = query_get(&pairs, "path").map(|s| s.to_string());
    let root_c = root.clone();
    let margins = margin_io(move || margins::list(&root_c, path.as_deref()))
        .await?
        .map_err(FileError::into_api)?;
    json_response(StatusCode::OK, &json!({ "margins": margins }))
}

pub async fn get(state: &AppState, owner: MarginOwner<'_>, mid: &str) -> ApiResult<Response<Body>> {
    let root = owner_root(state, owner).await?;
    let api_owner = owner.api_owner();
    let mid = mid.to_string();
    let root_c = root.clone();
    let (margin, html) = margin_io(move || -> ApiResult<(Margin, String)> {
        let m = margins::get(&root_c, &mid).map_err(FileError::into_api)?;
        let html = margins::render_body_html(&api_owner, &m.id, &m.body);
        Ok((m, html))
    })
    .await??;
    json_response(StatusCode::OK, &margin_json(&margin, &html))
}

#[derive(Deserialize)]
struct PutBody {
    id: String,
    path: String,
    start: u32,
    end: u32,
    #[serde(default)]
    quote: String,
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    media: Vec<String>,
}

pub async fn put(
    state: &AppState,
    owner: MarginOwner<'_>,
    mid: &str,
    req: Request<Incoming>,
) -> ApiResult<Response<Body>> {
    let root = owner_root(state, owner).await?;
    let api_owner = owner.api_owner();
    let body: PutBody = read_json(req).await?;
    if body.id != mid {
        return Err(ApiError::bad_request(
            "The margin id in the path and body must match.",
        ));
    }
    let margin = Margin {
        id: body.id,
        path: body.path,
        start: body.start,
        end: body.end,
        quote: body.quote,
        title: body.title,
        body: body.body,
        media: body.media,
        updated_at: 0,
    };
    let root_c = root.clone();
    let (saved, html) = margin_io(move || -> ApiResult<(Margin, String)> {
        let m = margins::put(&root_c, margin).map_err(FileError::into_api)?;
        let html = margins::render_body_html(&api_owner, &m.id, &m.body);
        Ok((m, html))
    })
    .await??;
    json_response(StatusCode::OK, &margin_json(&saved, &html))
}

pub async fn delete(
    state: &AppState,
    owner: MarginOwner<'_>,
    mid: &str,
) -> ApiResult<Response<Body>> {
    let root = owner_root(state, owner).await?;
    let mid = mid.to_string();
    let root_c = root.clone();
    margin_io(move || margins::delete(&root_c, &mid))
        .await?
        .map_err(FileError::into_api)?;
    Ok(empty(StatusCode::NO_CONTENT))
}

pub async fn put_media(
    state: &AppState,
    owner: MarginOwner<'_>,
    mid: &str,
    name: &str,
    req: Request<Incoming>,
) -> ApiResult<Response<Body>> {
    let root = owner_root(state, owner).await?;
    let bytes = read_body(req).await?;
    let mid = mid.to_string();
    let name_owned = name.to_string();
    let root_c = root.clone();
    let name_for_write = name_owned.clone();
    margin_io(move || margins::put_media(&root_c, &mid, &name_for_write, &bytes))
        .await?
        .map_err(FileError::into_api)?;
    json_response(StatusCode::OK, &json!({ "name": name_owned }))
}

pub async fn get_media(
    state: &AppState,
    owner: MarginOwner<'_>,
    mid: &str,
    name: &str,
) -> ApiResult<Response<Body>> {
    let root = owner_root(state, owner).await?;
    let mid = mid.to_string();
    let name = name.to_string();
    let root_c = root.clone();
    let (bytes, content_type) = margin_io(move || margins::get_media(&root_c, &mid, &name))
        .await?
        .map_err(FileError::into_api)?;
    let mut res = Response::new(Full::new(Bytes::from(bytes)).boxed());
    *res.status_mut() = StatusCode::OK;
    workspace_media_headers(&mut res, content_type);
    Ok(res)
}

pub async fn delete_media(
    state: &AppState,
    owner: MarginOwner<'_>,
    mid: &str,
    name: &str,
) -> ApiResult<Response<Body>> {
    let root = owner_root(state, owner).await?;
    let mid = mid.to_string();
    let name = name.to_string();
    let root_c = root.clone();
    margin_io(move || margins::delete_media(&root_c, &mid, &name))
        .await?
        .map_err(FileError::into_api)?;
    Ok(empty(StatusCode::NO_CONTENT))
}
