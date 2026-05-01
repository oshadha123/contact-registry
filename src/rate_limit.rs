use std::{num::NonZeroU32, sync::Arc, task::{Context, Poll}};

use axum::{body::Body, http::Request, response::Response};
use futures_util::future::BoxFuture;
use governor::{
    clock::DefaultClock, middleware::NoOpMiddleware,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter,
};
use tower::{Layer, Service};

pub type SharedLimiter = Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock, NoOpMiddleware>>;

pub fn new_limiter(requests_per_minute: u32) -> SharedLimiter {
    Arc::new(RateLimiter::direct(
        Quota::per_minute(NonZeroU32::new(requests_per_minute).unwrap()),
    ))
}

#[derive(Clone)]
pub struct RateLimitLayer(pub SharedLimiter);

impl<S> Layer<S> for RateLimitLayer {
    type Service = RateLimitService<S>;
    fn layer(&self, inner: S) -> Self::Service {
        RateLimitService { inner, limiter: self.0.clone() }
    }
}

#[derive(Clone)]
pub struct RateLimitService<S> {
    inner:   S,
    limiter: SharedLimiter,
}

impl<S> Service<Request<Body>> for RateLimitService<S>
where
    S: Service<Request<Body>, Response = Response> + Send + Clone + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error    = S::Error;
    type Future   = BoxFuture<'static, Result<Response, S::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), S::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        match self.limiter.check() {
            Ok(_) => {
                let fut = self.inner.call(req);
                Box::pin(async move { fut.await })
            }
            Err(_) => {
                tracing::warn!("Rate limit exceeded — 429");
                Box::pin(async {
                    Ok(Response::builder()
                        .status(429)
                        .header("Retry-After", "60")
                        .header("Content-Type", "text/plain")
                        .body(Body::from("Too many requests — try again in a minute."))
                        .unwrap())
                })
            }
        }
    }
}
