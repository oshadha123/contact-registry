use axum::{http::StatusCode, response::{IntoResponse, Response}};
use thiserror::Error;

/// Unified application error type.
/// Implementing IntoResponse means handlers can return `Result<_, AppError>`
/// and Axum converts failures into proper HTTP responses automatically.
#[derive(Debug, Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Cache error: {0}")]
    Cache(#[from] redis::RedisError),

    #[error("Template error: {0}")]
    Template(String),

    #[error("Bad request: {0}")]
    BadRequest(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self {
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            _                       => StatusCode::INTERNAL_SERVER_ERROR,
        };
        tracing::error!(error = %self, "Application error");
        (status, self.to_string()).into_response()
    }
}
