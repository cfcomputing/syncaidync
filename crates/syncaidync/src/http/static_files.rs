//! Serve the web UI from disk or the embedded copy.

use std::path::{Path, PathBuf};

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::header::{CACHE_CONTROL, CONTENT_TYPE, ETAG, IF_NONE_MATCH};
use hyper::{Request, Response, StatusCode};
use include_dir::{include_dir, Dir};
use sha2::{Digest, Sha256};

use super::error::{ApiError, ApiResult};
use super::Body;
use crate::config::Config;

static EMBEDDED: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../web");

pub fn serve(
    config: &Config,
    port: u16,
    req: &Request<hyper::body::Incoming>,
) -> ApiResult<Response<Body>> {
    let path = req.uri().path();
    if path.contains("..") {
        return Err(ApiError::not_found("No such file."));
    }
    let rel = if path == "/" || path.is_empty() {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };
    if rel == "test" || rel.starts_with("test/") {
        return Err(ApiError::not_found("No such file."));
    }

    if let Some(dir) = &config.web_dir {
        return serve_disk(dir, rel, port);
    }
    serve_embedded(rel, req, port)
}

fn serve_disk(root: &Path, rel: &str, port: u16) -> ApiResult<Response<Body>> {
    let mut full = root.join(rel);
    // Normalize: refuse escape.
    let root_canon = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    if let Ok(c) = full.canonicalize() {
        if !c.starts_with(&root_canon) {
            return Err(ApiError::not_found("No such file."));
        }
        full = c;
    } else if !full.exists() {
        return Err(ApiError::not_found("No such file."));
    }
    let bytes = std::fs::read(&full).map_err(|_| ApiError::not_found("No such file."))?;
    Ok(file_response(rel, bytes, true, None, port))
}

fn serve_embedded(
    rel: &str,
    req: &Request<hyper::body::Incoming>,
    port: u16,
) -> ApiResult<Response<Body>> {
    let file = EMBEDDED
        .get_file(rel)
        .ok_or_else(|| ApiError::not_found("No such file."))?;
    let bytes = file.contents();
    let etag = etag_for(bytes);
    if let Some(inm) = req
        .headers()
        .get(IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
    {
        if inm == etag {
            let mut res = Response::new(Full::new(Bytes::new()).boxed());
            *res.status_mut() = StatusCode::NOT_MODIFIED;
            if let Ok(v) = hyper::header::HeaderValue::from_str(&etag) {
                res.headers_mut().insert(ETAG, v);
            }
            crate::http::security_headers(&mut res, is_html(rel), Some(port));
            return Ok(res);
        }
    }
    Ok(file_response(rel, bytes.to_vec(), false, Some(etag), port))
}

fn file_response(
    rel: &str,
    bytes: Vec<u8>,
    no_store: bool,
    etag: Option<String>,
    port: u16,
) -> Response<Body> {
    let mut res = Response::new(Full::new(Bytes::from(bytes)).boxed());
    *res.status_mut() = StatusCode::OK;
    let ct = content_type(rel);
    res.headers_mut()
        .insert(CONTENT_TYPE, hyper::header::HeaderValue::from_static(ct));
    let cache = if no_store { "no-store" } else { "no-cache" };
    res.headers_mut().insert(
        CACHE_CONTROL,
        hyper::header::HeaderValue::from_static(cache),
    );
    if let Some(etag) = etag {
        if let Ok(v) = hyper::header::HeaderValue::from_str(&etag) {
            res.headers_mut().insert(ETAG, v);
        }
    }
    crate::http::security_headers(&mut res, is_html(rel), Some(port));
    res
}

fn etag_for(bytes: &[u8]) -> String {
    let dig = Sha256::digest(bytes);
    let hex: String = dig.iter().map(|b| format!("{b:02x}")).collect();
    format!("\"{hex}\"")
}

fn is_html(rel: &str) -> bool {
    rel.ends_with(".html") || rel == "index.html"
}

fn content_type(rel: &str) -> &'static str {
    match PathBuf::from(rel)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
    {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "map" => "application/json",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}
