use chrono::{DateTime, Utc};
use once_cell::sync::Lazy;
use redis::{aio::ConnectionManager, AsyncCommands};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use validator::Validate;

// ─────────────────────────────────────────────────────────────────────────────
//  Domain model
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Record {
    pub id:         i64,
    pub first_name: String,
    pub last_name:  String,
    pub phone:      String,
    pub address:    String,
    pub age:        i32,
    pub created_at: DateTime<Utc>,
}

// ─────────────────────────────────────────────────────────────────────────────
//  Pagination
// ─────────────────────────────────────────────────────────────────────────────

pub const PAGE_SIZE: i64 = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageResult {
    pub records:     Vec<Record>,
    pub next_cursor: Option<i64>,
}

// ─────────────────────────────────────────────────────────────────────────────
//  Validation regexes
// ─────────────────────────────────────────────────────────────────────────────

pub static RE_NAME:  Lazy<Regex> = Lazy::new(|| Regex::new(r"^[\p{L}\-']+$").unwrap());
pub static RE_PHONE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\d+$").unwrap());

// ─────────────────────────────────────────────────────────────────────────────
//  Form
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone, Deserialize, Serialize, Validate)]
pub struct RecordForm {
    #[validate(
        length(min = 2, max = 50, message = "Given name must be 2–50 characters"),
        regex(path = "*RE_NAME", message = "Given name: letters, hyphens and apostrophes only")
    )]
    pub first_name: String,

    #[validate(
        length(min = 2, max = 50, message = "Family name must be 2–50 characters"),
        regex(path = "*RE_NAME", message = "Family name: letters, hyphens and apostrophes only")
    )]
    pub last_name: String,

    #[validate(
        length(min = 8, max = 8, message = "Phone must be exactly 8 digits"),
        regex(path = "*RE_PHONE", message = "Phone must contain digits only")
    )]
    pub phone: String,

    #[validate(length(min = 5, max = 200, message = "Address must be 5–200 characters"))]
    pub address: String,

    #[validate(length(min = 1, message = "Age is required"))]
    pub age: String,
}

// ─────────────────────────────────────────────────────────────────────────────
//  Cache helpers
// ─────────────────────────────────────────────────────────────────────────────

const CACHE_TTL: u64 = 60;

fn cache_key(cursor: Option<i64>) -> String {
    match cursor {
        None    => "page:first".into(),
        Some(c) => format!("page:{c}"),
    }
}

async fn cache_get(redis: &mut ConnectionManager, key: &str) -> Option<PageResult> {
    let raw: Option<String> = redis.get(key).await.ok()?;
    serde_json::from_str(&raw?).ok()
}

async fn cache_set(redis: &mut ConnectionManager, key: &str, page: &PageResult) {
    if let Ok(json) = serde_json::to_string(page) {
        let _: Result<(), _> = redis.set_ex(key, json, CACHE_TTL).await;
    }
}

pub async fn cache_invalidate_all(redis: &mut ConnectionManager) {
    let keys: Vec<String> = redis::cmd("KEYS")
        .arg("page:*")
        .query_async(redis)
        .await
        .unwrap_or_default();
    for key in keys {
        let _: Result<(), _> = redis.del(&key).await;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Repository — runtime queries (no compile-time macro checking)
// ─────────────────────────────────────────────────────────────────────────────

pub async fn repo_insert(
    pool:  &PgPool,
    redis: &mut ConnectionManager,
    form:  &RecordForm,
) -> Result<i64, sqlx::Error> {
    let age: i32 = form.age.trim().parse().unwrap_or(0);

    let row: (i64,) = sqlx::query_as(
        "INSERT INTO records (first_name, last_name, phone, address, age)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id",
    )
    .bind(&form.first_name)
    .bind(&form.last_name)
    .bind(&form.phone)
    .bind(&form.address)
    .bind(age)
    .fetch_one(pool)
    .await?;

    cache_invalidate_all(redis).await;
    Ok(row.0)
}

pub async fn repo_fetch_page(
    pool:   &PgPool,
    redis:  &mut ConnectionManager,
    cursor: Option<i64>,
) -> Result<PageResult, sqlx::Error> {
    let key = cache_key(cursor);

    if let Some(cached) = cache_get(redis, &key).await {
        tracing::debug!(key, "cache hit");
        return Ok(cached);
    }

    let limit = PAGE_SIZE + 1;

    let mut rows: Vec<Record> = match cursor {
        None => {
            sqlx::query_as(
                "SELECT id, first_name, last_name, phone, address, age, created_at
                 FROM records
                 ORDER BY id DESC
                 LIMIT $1",
            )
            .bind(limit)
            .fetch_all(pool)
            .await?
        }
        Some(c) => {
            sqlx::query_as(
                "SELECT id, first_name, last_name, phone, address, age, created_at
                 FROM records
                 WHERE id < $1
                 ORDER BY id DESC
                 LIMIT $2",
            )
            .bind(c)
            .bind(limit)
            .fetch_all(pool)
            .await?
        }
    };

    let next_cursor = if rows.len() as i64 > PAGE_SIZE {
        rows.pop();
        rows.last().map(|r| r.id)
    } else {
        None
    };

    let result = PageResult { records: rows, next_cursor };
    cache_set(redis, &key, &result).await;
    Ok(result)
}
