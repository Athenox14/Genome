use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::Result;

/// Forwards an incoming HTTP request to a workspace's code-server instance running on
/// `127.0.0.1:{host_port}`, so the UI can be served under Genome's own domain instead of
/// exposing the container's port directly to end users.
pub async fn proxy_to_workspace(
    host_port: u16,
    req_path: &str,
    req: Request,
) -> Result<Response> {
    let client = reqwest::Client::new();

    let method = reqwest::Method::from_bytes(req.method().as_str().as_bytes())
        .unwrap_or(reqwest::Method::GET);

    let target_url = format!("http://127.0.0.1:{}{}", host_port, req_path);

    let mut headers = reqwest::header::HeaderMap::new();
    for (name, value) in req.headers().iter() {
        if name == axum::http::header::HOST {
            continue;
        }
        if let Ok(v) = reqwest::header::HeaderValue::from_bytes(value.as_bytes()) {
            if let Ok(n) = reqwest::header::HeaderName::from_bytes(name.as_str().as_bytes()) {
                headers.insert(n, v);
            }
        }
    }

    let body_bytes = axum::body::to_bytes(req.into_body(), usize::MAX)
        .await
        .map_err(|e| crate::DevEnvError::InvalidRequest(e.to_string()))?;

    let upstream_response = client
        .request(method, &target_url)
        .headers(headers)
        .body(body_bytes)
        .send()
        .await?;

    let status =
        StatusCode::from_u16(upstream_response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);

    // `transfer-encoding`/`content-length` describe upstream's original framing,
    // but the body below is fully buffered into a single `Bytes` chunk -- axum
    // recomputes the real content-length itself. Forwarding the stale header
    // (e.g. `chunked`) alongside a non-chunked body is a framing mismatch that
    // some HTTP clients (observed with .NET's HttpWebRequest) treat as a reset
    // connection rather than a well-formed response.
    let mut response_headers = HeaderMap::new();
    for (name, value) in upstream_response.headers().iter() {
        if name == reqwest::header::TRANSFER_ENCODING || name == reqwest::header::CONTENT_LENGTH {
            continue;
        }
        if let Ok(n) = axum::http::HeaderName::from_bytes(name.as_str().as_bytes()) {
            if let Ok(v) = axum::http::HeaderValue::from_bytes(value.as_bytes()) {
                response_headers.insert(n, v);
            }
        }
    }

    let bytes = upstream_response.bytes().await?;

    let mut response = (status, Body::from(bytes)).into_response();
    *response.headers_mut() = response_headers;

    Ok(response)
}
