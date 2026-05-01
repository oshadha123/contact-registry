use axum::{
    extract::{Form, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
};
use askama::Template;

use serde::Deserialize;
use validator::Validate;

use crate::{
    error::AppError,
    models::{repo_fetch_page, repo_insert, Record, RecordForm},
    state::AppState,
};

// ─────────────────────────────────────────────────────────────────────────────
//  Askama template — compiled at build time, fields type-checked by the compiler
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexTemplate<'a> {
    pub records:      Vec<Record>,
    pub form:         &'a RecordForm,
    pub errors:       Vec<String>,
    pub show_success: bool,
    /// Cursor for the next page link (`None` = on last page)
    pub next_cursor:  Option<i64>,
    /// Cursor for the current page (used to build "previous" link)
    pub cursor:       Option<i64>,
}

// ─────────────────────────────────────────────────────────────────────────────
//  Query params
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
pub struct IndexQuery {
    /// Keyset pagination cursor — last seen id (exclusive).
    pub cursor:  Option<i64>,
    pub success: Option<u8>,
}

// ─────────────────────────────────────────────────────────────────────────────
//  Handlers
// ─────────────────────────────────────────────────────────────────────────────

/// GET / — render empty form + paginated records table
pub async fn handle_index(
    State(state): State<AppState>,
    Query(query): Query<IndexQuery>,
) -> Result<impl IntoResponse, AppError> {
    let mut redis = state.redis.clone();
    let page = repo_fetch_page(&state.pool, &mut redis, query.cursor).await?;

    let tmpl = IndexTemplate {
        records:      page.records,
        form:         &RecordForm::default(),
        errors:       vec![],
        show_success: query.success == Some(1),
        next_cursor:  page.next_cursor,
        cursor:       query.cursor,
    };
    let html = tmpl.render().map_err(|e| AppError::Template(e.to_string()))?;
    Ok(Html(html))
}

/// POST / — validate → insert + PRG redirect, or re-render with errors
pub async fn handle_submit(
    State(state): State<AppState>,
    Form(form): Form<RecordForm>,
) -> Result<Response, AppError> {
    let mut redis = state.redis.clone();

    match form.validate() {
        // ── Valid: persist and redirect (Post-Redirect-Get pattern) ──────────
        Ok(()) => {
            repo_insert(&state.pool, &mut redis, &form).await?;
            tracing::info!(first = %form.first_name, last = %form.last_name, "Record inserted");
            Ok(Redirect::to("/?success=1").into_response())
        }
        // ── Invalid: re-render the page with error messages + original input ─
        Err(ve) => {
            let mut errors: Vec<String> = ve
                .field_errors()
                .values()
                .flat_map(|errs| errs.iter())
                .filter_map(|e| e.message.as_ref().map(|m| m.to_string()))
                .collect();
            errors.sort();
            errors.dedup();

            let page = repo_fetch_page(&state.pool, &mut redis, None).await?;
            let tmpl = IndexTemplate {
                records:      page.records,
                form:         &form,
                errors,
                show_success: false,
                next_cursor:  page.next_cursor,
                cursor:       None,
            };
            let html = tmpl.render().map_err(|e| AppError::Template(e.to_string()))?;
            Ok((StatusCode::UNPROCESSABLE_ENTITY, Html(html)).into_response())
        }
    }
}

/// GET /health — liveness probe for Docker / load balancers
pub async fn handle_health() -> impl IntoResponse {
    StatusCode::OK
}
