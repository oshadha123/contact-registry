mod config;
mod error;
mod handlers;
mod models;
mod rate_limit;
mod state;
mod static_assets;

use std::time::Duration;

use axum::{routing::get, Router};
use redis::aio::ConnectionManager;
use sqlx::{postgres::PgPoolOptions, PgPool};
use tower::ServiceBuilder;
use tower_http::{
    compression::CompressionLayer,
    request_id::{MakeRequestUuid, SetRequestIdLayer},
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
};
use tracing::Level;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use config::Config;
use rate_limit::{new_limiter, RateLimitLayer};
use state::AppState;

async fn init_db(database_url: &str) -> anyhow::Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .min_connections(2)
        .acquire_timeout(Duration::from_secs(5))
        .connect(database_url)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

async fn init_redis(redis_url: &str) -> anyhow::Result<ConnectionManager> {
    let client  = redis::Client::open(redis_url)?;
    let manager = ConnectionManager::new(client).await?;
    Ok(manager)
}

fn build_app(state: AppState) -> Router {
    // Rate limiters applied per-route so they sit inside the body-wrapping
    // middleware, where the Response type is plain axum Response.
    let read_limiter  = new_limiter(60);
    let write_limiter = new_limiter(20);

    Router::new()
        .route("/static/css/app.css", get(static_assets::handle_static_css))
        .route(
            "/",
            get(handlers::handle_index)
                .layer(RateLimitLayer(read_limiter))
                .post(handlers::handle_submit)
                .layer(RateLimitLayer(write_limiter)),
        )
        .route("/health", get(handlers::handle_health))
        .with_state(state)
        .layer(
            ServiceBuilder::new()
                .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
                .layer(
                    TraceLayer::new_for_http()
                        .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                        .on_response(DefaultOnResponse::new().level(Level::INFO)),
                )
                .layer(CompressionLayer::new()),
        )
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    let cfg = Config::from_env()?;

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| cfg.rust_log.as_str().into()))
        .with(fmt::layer().with_target(false).compact())
        .init();

    tracing::info!("Connecting to Postgres…");
    let pool = init_db(&cfg.database_url).await?;

    tracing::info!("Connecting to Redis…");
    let redis = init_redis(&cfg.redis_url).await?;

    let state    = AppState { pool, redis };
    let app      = build_app(state);
    let addr     = format!("0.0.0.0:{}", cfg.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    tracing::info!("Listening → http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
