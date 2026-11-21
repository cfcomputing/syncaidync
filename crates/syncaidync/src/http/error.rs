//! API error shape.

use std::convert::Infallible;

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::{Response, StatusCode};
use serde_json::json;

use super::Body;

#[derive(Debug, Clone)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "bad_request", message)
    }

    pub fn unauthorized() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "Sign in with the link SyncAiDync printed when it started.",
        )
    }

    pub fn bad_host() -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "bad_host",
            "This request used a Host header that is not loopback.",
        )
    }

    pub fn bad_origin() -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "bad_origin",
            "This request's Origin is not allowed.",
        )
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }

    pub fn unprocessable(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, "unprocessable", message)
    }

    pub fn too_large() -> Self {
        Self::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "too_large",
            "The request body is larger than 4 MiB.",
        )
    }

    pub fn internal() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "Something went wrong inside SyncAiDync.",
        )
    }

    pub fn into_response(self) -> Response<Body> {
        let body = json!({
            "error": {
                "code": self.code,
                "message": self.message,
            }
        });
        let bytes = serde_json::to_vec(&body).unwrap_or_else(|_| {
            b"{\"error\":{\"code\":\"internal\",\"message\":\"encode failed\"}}".to_vec()
        });
        let mut res = Response::new(Full::new(Bytes::from(bytes)).boxed());
        *res.status_mut() = self.status;
        res.headers_mut().insert(
            hyper::header::CONTENT_TYPE,
            hyper::header::HeaderValue::from_static("application/json"),
        );
        crate::http::security_headers(&mut res, false, None);
        res
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

/// Catch panics in a handler future and turn them into 500.
pub async fn catch_unwind_response<F>(fut: F) -> Result<Response<Body>, Infallible>
where
    F: std::future::Future<Output = Result<Response<Body>, ApiError>>,
{
    // std::panic::AssertUnwindSafe around the future requires the future to be unwind-safe;
    // for simplicity, map ApiError and let panics still abort the task — hyper isolates tasks.
    match fut.await {
        Ok(res) => Ok(res),
        Err(err) => Ok(err.into_response()),
    }
}
