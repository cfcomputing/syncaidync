//! Host, Origin, and credential checks.

use hyper::header::{HeaderMap, HeaderValue, AUTHORIZATION, COOKIE, HOST, ORIGIN, SET_COOKIE};
use hyper::{Method, Request, StatusCode};
use serde::Deserialize;

use super::error::{ApiError, ApiResult};
use super::json::{empty, read_json};
use super::AppState;
use hyper::body::Incoming;

#[derive(Debug, Deserialize)]
pub struct AuthBody {
    pub key: String,
}

pub fn check_host(headers: &HeaderMap, port: u16) -> ApiResult<()> {
    let host = headers
        .get(HOST)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(ApiError::bad_host)?;
    let ok = [
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        format!("[::1]:{port}"),
    ];
    if ok.iter().any(|h| h == host) {
        Ok(())
    } else {
        Err(ApiError::bad_host())
    }
}

pub fn check_origin(method: &Method, headers: &HeaderMap, port: u16, is_ws: bool) -> ApiResult<()> {
    let needs = is_ws || !matches!(*method, Method::GET | Method::HEAD);
    if !needs {
        return Ok(());
    }
    let Some(origin) = headers.get(ORIGIN).and_then(|v| v.to_str().ok()) else {
        return Ok(());
    };
    let ok = [
        format!("http://127.0.0.1:{port}"),
        format!("http://localhost:{port}"),
    ];
    if ok.iter().any(|o| o == origin) {
        Ok(())
    } else {
        Err(ApiError::bad_origin())
    }
}

pub fn check_api_auth(headers: &HeaderMap, state: &AppState) -> ApiResult<()> {
    let key = &state.config.key;
    if let Some(auth) = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        if let Some(token) = auth.strip_prefix("Bearer ") {
            if constant_time_eq(token.as_bytes(), key.as_bytes()) {
                return Ok(());
            }
        }
    }
    if let Some(cookie) = headers.get(COOKIE).and_then(|v| v.to_str().ok()) {
        let name = state.config.cookie_name();
        for part in cookie.split(';') {
            let part = part.trim();
            if let Some(v) = part.strip_prefix(&format!("{name}=")) {
                if constant_time_eq(v.as_bytes(), key.as_bytes()) {
                    return Ok(());
                }
            }
        }
    }
    Err(ApiError::unauthorized())
}

pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        // Still walk to keep timing flatter for length mismatch of secrets of known size.
        let mut diff = a.len() ^ b.len();
        for i in 0..a.len().max(b.len()) {
            let x = a.get(i).copied().unwrap_or(0);
            let y = b.get(i).copied().unwrap_or(0);
            diff |= (x ^ y) as usize;
        }
        return diff == 0;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

pub async fn post_auth(
    state: &AppState,
    req: Request<Incoming>,
) -> ApiResult<hyper::Response<super::Body>> {
    let body: AuthBody = read_json(req).await?;
    if !constant_time_eq(body.key.as_bytes(), state.config.key.as_bytes()) {
        return Err(ApiError::unauthorized());
    }
    let mut res = empty(StatusCode::NO_CONTENT);
    let cookie = format!(
        "{}={}; HttpOnly; SameSite=Strict; Path=/",
        state.config.cookie_name(),
        state.config.key
    );
    if let Ok(v) = HeaderValue::from_str(&cookie) {
        res.headers_mut().insert(SET_COOKIE, v);
    }
    Ok(res)
}

pub fn get_auth(state: &AppState, headers: &HeaderMap) -> ApiResult<hyper::Response<super::Body>> {
    check_api_auth(headers, state)?;
    Ok(empty(StatusCode::NO_CONTENT))
}
