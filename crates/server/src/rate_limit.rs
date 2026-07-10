use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::ConnectInfo;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{body::Body, extract::Request, middleware::Next};
use dashmap::DashMap;

const WINDOW: Duration = Duration::from_secs(60);
const MAX_REQUESTS_PER_WINDOW: u32 = 120;

/// A minimal in-memory fixed-window rate limiter keyed by client IP address.
/// Not distributed-safe (per-process only), which is acceptable for a
/// single-instance self-hosted forge; caps abusive clients hammering
/// `/graphql` or `/health`.
#[derive(Clone, Default)]
pub struct RateLimiter {
    buckets: Arc<DashMap<IpAddr, (Instant, u32)>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    fn check(&self, ip: IpAddr) -> bool {
        let now = Instant::now();
        let mut entry = self.buckets.entry(ip).or_insert((now, 0));
        let (window_start, count) = *entry;
        if now.duration_since(window_start) > WINDOW {
            *entry = (now, 1);
            true
        } else if count < MAX_REQUESTS_PER_WINDOW {
            entry.1 += 1;
            true
        } else {
            false
        }
    }
}

pub async fn rate_limit_middleware(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    axum::extract::State(limiter): axum::extract::State<RateLimiter>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if limiter.check(addr.ip()) {
        next.run(request).await
    } else {
        (StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded").into_response()
    }
}
