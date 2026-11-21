//! JSON body reading and query parsing.

use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::Request;
use serde::de::DeserializeOwned;

use super::error::{ApiError, ApiResult};

pub const MAX_BODY: usize = 4 * 1024 * 1024;

pub async fn read_body(req: Request<Incoming>) -> ApiResult<Bytes> {
    if let Some(len) = req
        .headers()
        .get(hyper::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<usize>().ok())
    {
        if len > MAX_BODY {
            return Err(ApiError::too_large());
        }
    }
    let body = req
        .collect()
        .await
        .map_err(|_| ApiError::bad_request("Could not read the request body."))?;
    let bytes = body.to_bytes();
    if bytes.len() > MAX_BODY {
        return Err(ApiError::too_large());
    }
    Ok(bytes)
}

pub async fn read_json<T: DeserializeOwned>(req: Request<Incoming>) -> ApiResult<T> {
    let bytes = read_body(req).await?;
    serde_json::from_slice(&bytes)
        .map_err(|e| ApiError::bad_request(format!("The request body is not valid JSON: {e}")))
}

pub fn json_response<T: serde::Serialize>(
    status: hyper::StatusCode,
    value: &T,
) -> Result<hyper::Response<super::Body>, ApiError> {
    let bytes = serde_json::to_vec(value).map_err(|_| ApiError::internal())?;
    let mut res = hyper::Response::new(Full::new(Bytes::from(bytes)).boxed());
    *res.status_mut() = status;
    res.headers_mut().insert(
        hyper::header::CONTENT_TYPE,
        hyper::header::HeaderValue::from_static("application/json"),
    );
    crate::http::security_headers(&mut res, false, None);
    Ok(res)
}

pub fn empty(status: hyper::StatusCode) -> hyper::Response<super::Body> {
    let mut res = hyper::Response::new(Full::new(Bytes::new()).boxed());
    *res.status_mut() = status;
    crate::http::security_headers(&mut res, false, None);
    res
}

/// Percent-decode a query string into key/value pairs (first value wins).
pub fn parse_query(q: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for pair in q.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => (pair, ""),
        };
        out.push((percent_decode(k), percent_decode(v)));
    }
    out
}

pub fn query_get<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                if let (Some(h), Some(l)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                    out.push((h << 4) | l);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
