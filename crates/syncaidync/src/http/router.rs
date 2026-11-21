//! Path matching and request dispatch.

use std::sync::Arc;

use hyper::body::Incoming;
use hyper::{Method, Request, Response};

use super::auth::{self, check_api_auth, check_host, check_origin};
use super::error::{ApiError, ApiResult};
use super::json::{parse_query, query_get};
use super::sse;
use super::static_files;
use super::{AppState, Body};
use crate::api;

pub async fn handle(state: Arc<AppState>, req: Request<Incoming>) -> ApiResult<Response<Body>> {
    let port = state.bound_port();
    check_host(req.headers(), port)?;

    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    let is_api = segments.first().copied() == Some("api");
    let is_agent = segments.first().copied() == Some("agent");
    if is_api || is_agent {
        check_origin(&method, req.headers(), port, false)?;
    }

    if segments == ["api", "v1", "auth"] {
        return match method {
            Method::POST => auth::post_auth(&state, req).await,
            Method::GET => {
                check_api_auth(req.headers(), &state)?;
                auth::get_auth(&state, req.headers())
            }
            _ => Err(ApiError::not_found("No such route.")),
        };
    }

    if is_api {
        check_api_auth(req.headers(), &state)?;
        return dispatch_api(state, method, &segments, req).await;
    }

    if is_agent {
        return dispatch_agent(state, method, &segments, req).await;
    }

    if matches!(method, Method::GET | Method::HEAD) {
        return static_files::serve(&state.config, port, &req);
    }

    Err(ApiError::not_found("No such route."))
}

async fn dispatch_api(
    state: Arc<AppState>,
    method: Method,
    segments: &[&str],
    req: Request<Incoming>,
) -> ApiResult<Response<Body>> {
    if segments.len() < 2 || segments[0] != "api" || segments[1] != "v1" {
        return Err(ApiError::not_found("No such route."));
    }
    let rest = &segments[2..];
    match (method.clone(), rest) {
        (Method::GET, ["meta"]) => api::misc::meta(&state).await,
        (Method::GET, ["update"]) => api::misc::update(&state).await,
        (Method::GET, ["load"]) => api::misc::load(&state).await,
        (Method::POST, ["markdown"]) => api::misc::post_markdown(req).await,
        (Method::GET, ["history"]) => api::discovery::history(&state).await,
        (Method::POST, ["history", "import"]) => api::discovery::import(&state, req).await,
        (Method::GET, ["sessions", id, "links"]) => api::sessions::links(&state, id).await,
        (Method::GET, ["stream"]) => {
            let rx = state.hub.subscribe();
            let snapshot = state.stream_snapshot().await;
            sse::stream_response(rx, snapshot)
        }
        (Method::GET, ["sessions"]) => api::sessions::list(&state).await,
        (Method::POST, ["sessions"]) => api::sessions::create(&state, req).await,
        (Method::DELETE, ["sessions", id]) => {
            let q = req.uri().query().unwrap_or("");
            let pairs = parse_query(q);
            let workspace = query_get(&pairs, "workspace");
            api::sessions::delete(&state, id, workspace).await
        }
        (Method::GET, ["sessions", id, "removal"]) => api::sessions::removal(&state, id).await,
        (Method::POST, ["sessions", id, "resume"]) => api::sessions::resume(&state, id).await,
        (Method::POST, ["sessions", id, "stop"]) => api::sessions::stop(&state, id).await,
        (Method::POST, ["sessions", id, "archive"]) => {
            api::sessions::archive(&state, id, req).await
        }
        (Method::POST, ["sessions", id, "snooze"]) => api::sessions::snooze(&state, id, req).await,
        (Method::POST, ["sessions", id, "title"]) => api::sessions::title(&state, id, req).await,
        (Method::POST, ["sessions", id, "send"]) => api::sessions::send(&state, id, req).await,
        (Method::GET, ["sessions", id, "terminal"]) => {
            api::sessions::terminal(state, (*id).to_string(), req).await
        }
        (Method::GET, ["sessions", id, "tree"]) => {
            let q = req.uri().query().unwrap_or("");
            api::files::tree(&state, id, q).await
        }
        (Method::GET, ["sessions", id, "find"]) => {
            let q = req.uri().query().unwrap_or("");
            api::files::find(&state, id, q).await
        }
        (Method::GET, ["sessions", id, "file"]) => {
            let q = req.uri().query().unwrap_or("");
            api::files::get_file(&state, id, q).await
        }
        (Method::PUT, ["sessions", id, "file"]) => api::files::put_file(&state, id, req).await,
        (Method::POST, ["sessions", id, "markdown"]) => {
            api::files::post_markdown(&state, id, req).await
        }
        (Method::GET, ["sessions", id, "media"]) => {
            let q = req.uri().query().unwrap_or("");
            api::files::get_media(&state, id, q).await
        }
        (Method::GET, ["sessions", id, "margins"]) => {
            let q = req.uri().query().unwrap_or("");
            api::margins::list(&state, api::margins::MarginOwner::Session(id), q).await
        }
        (Method::GET, ["sessions", id, "margins", mid]) => {
            api::margins::get(&state, api::margins::MarginOwner::Session(id), mid).await
        }
        (Method::PUT, ["sessions", id, "margins", mid]) => {
            api::margins::put(&state, api::margins::MarginOwner::Session(id), mid, req).await
        }
        (Method::DELETE, ["sessions", id, "margins", mid]) => {
            api::margins::delete(&state, api::margins::MarginOwner::Session(id), mid).await
        }
        (Method::GET, ["sessions", id, "margins", mid, "media", name]) => {
            api::margins::get_media(&state, api::margins::MarginOwner::Session(id), mid, name).await
        }
        (Method::PUT, ["sessions", id, "margins", mid, "media", name]) => {
            api::margins::put_media(
                &state,
                api::margins::MarginOwner::Session(id),
                mid,
                name,
                req,
            )
            .await
        }
        (Method::DELETE, ["sessions", id, "margins", mid, "media", name]) => {
            api::margins::delete_media(&state, api::margins::MarginOwner::Session(id), mid, name)
                .await
        }
        (Method::GET, ["sessions", id, "changes"]) => api::changes::get_changes(&state, id).await,
        (Method::GET, ["sessions", id, "review"]) => api::changes::get_review(&state, id).await,
        (Method::PUT, ["sessions", id, "review"]) => {
            api::changes::put_review(&state, id, req).await
        }
        (Method::POST, ["sessions", id, "changes", "plan"]) => {
            api::changes::post_plan(&state, id, req).await
        }
        (Method::GET, ["sessions", id, "takeaways", "destinations"]) => {
            api::takeaways::destinations(&state, id).await
        }
        (Method::POST, ["sessions", id, "takeaways", "suggest"]) => {
            api::takeaways::suggest(&state, id, req).await
        }
        (Method::POST, ["sessions", id, "takeaways", "save"]) => {
            api::takeaways::save(&state, id, req).await
        }
        (Method::POST, ["goals", "draft"]) => api::goals::draft(&state, req).await,
        (Method::GET, ["sessions", id, "goal"]) => api::goals::get(&state, id).await,
        (Method::POST, ["sessions", id, "goal", "run"]) => api::goals::run(&state, id).await,
        (Method::POST, ["sessions", id, "goal", "continue"]) => {
            api::goals::continue_goal(&state, id, req).await
        }
        (Method::POST, ["sessions", id, "goal", "accept"]) => {
            api::goals::accept(&state, id, req).await
        }
        (Method::POST, ["sessions", id, "goal", "manual"]) => {
            api::goals::manual(&state, id, req).await
        }
        (Method::POST, ["sessions", id, "goal", "drop"]) => api::goals::drop_goal(&state, id).await,
        (Method::POST, ["sessions", id, "goal", "report"]) => {
            api::goals::rewrite_report(&state, id).await
        }
        (Method::GET, ["sessions", id, "goal", "commit"]) => {
            api::goals::commit_preview(&state, id).await
        }
        (Method::POST, ["sessions", id, "goal", "commit"]) => {
            api::goals::commit(&state, id, req).await
        }
        (Method::GET, ["sessions", id, "goal", "merge"]) => {
            api::goals::merge_preview(&state, id).await
        }
        (Method::POST, ["sessions", id, "goal", "merge"]) => api::goals::merge(&state, id).await,
        (Method::POST, ["sessions", id, "goal", "more"]) => {
            api::goals::keep_working(&state, id, req).await
        }
        (Method::GET, ["repos"]) => api::discovery::list_repos(&state).await,
        (Method::GET, ["repos", "branches"]) => {
            let q = req.uri().query().unwrap_or("");
            api::discovery::repo_branches(&state, q).await
        }
        (Method::GET, ["folders"]) => {
            let q = req.uri().query().unwrap_or("");
            api::discovery::list_folders(&state, q).await
        }
        (Method::GET, ["pulls"]) => api::pulls::list(&state).await,
        (Method::POST, ["pulls", "open"]) => api::pulls::open(&state, req).await,
        (Method::GET, ["pulls", id, "changes"]) => {
            let q = req.uri().query().unwrap_or("");
            api::pulls::get_changes(&state, id, q).await
        }
        (Method::GET, ["pulls", id, "file"]) => {
            let q = req.uri().query().unwrap_or("");
            api::pulls::get_file(&state, id, q).await
        }
        (Method::GET, ["pulls", id, "margins"]) => {
            let q = req.uri().query().unwrap_or("");
            api::margins::list(&state, api::margins::MarginOwner::Pull(id), q).await
        }
        (Method::GET, ["pulls", id, "margins", mid]) => {
            api::margins::get(&state, api::margins::MarginOwner::Pull(id), mid).await
        }
        (Method::PUT, ["pulls", id, "margins", mid]) => {
            api::margins::put(&state, api::margins::MarginOwner::Pull(id), mid, req).await
        }
        (Method::DELETE, ["pulls", id, "margins", mid]) => {
            api::margins::delete(&state, api::margins::MarginOwner::Pull(id), mid).await
        }
        (Method::GET, ["pulls", id, "margins", mid, "media", name]) => {
            api::margins::get_media(&state, api::margins::MarginOwner::Pull(id), mid, name).await
        }
        (Method::PUT, ["pulls", id, "margins", mid, "media", name]) => {
            api::margins::put_media(&state, api::margins::MarginOwner::Pull(id), mid, name, req)
                .await
        }
        (Method::DELETE, ["pulls", id, "margins", mid, "media", name]) => {
            api::margins::delete_media(&state, api::margins::MarginOwner::Pull(id), mid, name).await
        }
        (Method::GET, ["pulls", id, "review"]) => api::pulls::get_review(&state, id).await,
        (Method::PUT, ["pulls", id, "review"]) => api::pulls::put_review(&state, id, req).await,
        (Method::POST, ["pulls", id, "plan"]) => api::pulls::post_plan(&state, id, req).await,
        (Method::POST, ["pulls", id, "draft"]) => api::pulls::draft(&state, id).await,
        (Method::POST, ["pulls", id, "session"]) => api::pulls::session(&state, id, req).await,
        (Method::PATCH, ["pulls", id]) => api::pulls::patch(&state, id, req).await,
        (Method::DELETE, ["pulls", id]) => api::pulls::delete(&state, id).await,
        _ => Err(ApiError::not_found("No such route.")),
    }
}

async fn dispatch_agent(
    state: Arc<AppState>,
    method: Method,
    segments: &[&str],
    req: Request<Incoming>,
) -> ApiResult<Response<Body>> {
    if segments.len() < 2 || segments[0] != "agent" || segments[1] != "v1" {
        return Err(ApiError::not_found("No such route."));
    }
    match (method, &segments[2..]) {
        (Method::POST, ["signal"]) => api::agent::signal(&state, req).await,
        (Method::POST, ["usage"]) => api::agent::usage(&state, req).await,
        _ => Err(ApiError::not_found("No such route.")),
    }
}
