use redis::aio::ConnectionManager;
use sqlx::PgPool;

/// Shared application state — injected into every handler via Axum's State extractor.
/// Both PgPool and ConnectionManager are internally Arc-backed; Clone is cheap.
#[derive(Clone)]
pub struct AppState {
    pub pool:  PgPool,
    pub redis: ConnectionManager,
}
