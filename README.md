# Contact Registry v3 — Production Edition

Refactored from SQLite single-writer to a production Rust web stack capable of serving at scale.

```bash
docker compose up --build -d
open http://localhost:8080
```

---

## What Changed From v2 and Why

| Layer          | v2 (SQLite)              | v3 (Production)                          | Reason                                      |
|----------------|--------------------------|------------------------------------------|---------------------------------------------|
| DB             | SQLite single-writer     | Postgres 16                              | SQLite serialises all writes; can't scale   |
| Pool           | 8 SQLite connections     | PgBouncer → Postgres (transaction mode)  | Proper connection multiplexing              |
| Query          | SELECT * all records     | Keyset pagination (cursor-based)         | Can't load 100M rows into memory            |
| Caching        | None                     | Redis (TTL 60 s, cache-aside)            | Offload DB on hot read paths                |
| Rate limiting  | None                     | Tower middleware (governor token-bucket) | Protect POST from spam/abuse                |
| Tailwind       | CDN (400 KB, runtime)    | CLI compiled + purged (~5 KB, build time)| CDN is not prod; purge removes unused classes|
| Static assets  | Served by Axum, no cache | `include_bytes!` + ETag + immutable      | Proper browser caching, no disk I/O         |
| Infra          | Single container         | app + Postgres + PgBouncer + Redis       | Horizontal scalability                      |
| Config         | .env only                | `envy` typed deserialization             | Fail fast on bad/missing env vars at startup|
| Migrations     | SQLite SQL               | Postgres SQL                             | Schema updated for new DB                   |

---

## Stack

| Layer         | Crate / Tool            | Role                                                      |
|---------------|-------------------------|-----------------------------------------------------------|
| Runtime       | **tokio 1**             | Multi-threaded async executor                             |
| Web           | **Axum 0.7**            | Routing, extractors, middleware composition               |
| Database      | **SQLx 0.7 + Postgres** | Async Postgres, compile-time query checks, auto-migration |
| Pool          | **PgBouncer 1.22**      | Transaction-mode connection pooling in front of Postgres  |
| Cache         | **Redis 7**             | TTL-based page cache, invalidated on INSERT               |
| Pagination    | Keyset / cursor-based   | `WHERE id < $cursor ORDER BY id DESC LIMIT 21` — O(log n)|
| Templates     | **Askama 0.12**         | Jinja2-style templates compiled at build time             |
| Validation    | **validator 0.18**      | Derive macros: length, range, regex rules on structs      |
| Rate limiting | **governor 0.6**        | Token-bucket: 60 req/min global, 20 req/min on POST       |
| Middleware    | **tower-http 0.5**      | Gzip compression, X-Request-Id, structured tracing spans  |
| CSS           | **Tailwind CLI 3**      | Purged + minified at Docker build time (~5 KB)            |
| Static assets | **include_bytes!**      | CSS embedded in binary; ETag + `Cache-Control: immutable` |
| Config        | **envy 0.4**            | Typed env-var deserialization — fails fast on bad config   |
| Errors        | **thiserror**           | Typed AppError implementing IntoResponse                  |
| Hashing       | **sha2 + hex**          | SHA-256 ETag for the embedded CSS                         |

---

## Module Layout

```
src/
├── main.rs           — startup, DB + Redis init, router, middleware stack
├── config.rs         — Config struct deserialized from env (fail-fast)
├── state.rs          — AppState { pool: PgPool, redis: ConnectionManager }
├── error.rs          — AppError enum implementing IntoResponse
├── models.rs         — Record, RecordForm + validation, repo_* + Redis cache
├── handlers.rs       — Axum handlers + Askama template structs
├── rate_limit.rs     — Tower Layer/Service wrapping governor; returns 429
├── static_assets.rs  — Embedded CSS with ETag + Cache-Control: immutable
└── tailwind.css      — Tailwind source (input to CLI, not served directly)

templates/
└── index.html        — Askama template; references /static/css/app.css

static/css/
└── app.css           — Compiled by Tailwind CLI; embedded via include_bytes!
                        Committed as a build stub; Docker stage 1 overwrites it.

migrations/
└── 0001_init.sql     — Postgres schema: BIGSERIAL id, TIMESTAMPTZ, DESC index
```

---

## How Each Feature Works

### Keyset Pagination
```sql
-- First page (no cursor)
SELECT ... FROM records ORDER BY id DESC LIMIT 21;

-- Subsequent pages
SELECT ... FROM records WHERE id < $cursor ORDER BY id DESC LIMIT 21;
```
Fetching 21 rows when page size is 20: if we get 21 back, there's a next page (pop the sentinel). The `idx_records_id_desc` index makes this scan `O(log n)` at any table size.

### Redis Cache
- **Key**: `page:first` or `page:{last_id}`
- **TTL**: 60 seconds
- **Invalidation**: on every `INSERT`, `cache_invalidate_all` runs `KEYS page:* | DEL`
- **Miss**: query Postgres → serialize → `SET EX`
- **Hit**: deserialize JSON → return immediately

### Rate Limiting
- **Global**: 60 req/min (all routes) — rejects before any processing
- **POST /**: 20 req/min (write endpoint) — tighter limit on the mutation path
- **Response**: HTTP 429 + `Retry-After: 60` header
- **Implementation**: `governor` token-bucket wrapped in a custom Tower `Layer` + `Service`

### Static Assets
```rust
static CSS_BYTES: &[u8] = include_bytes!("../static/css/app.css");
static CSS_ETAG: Lazy<String> = ...SHA-256 of CSS_BYTES...;
```
- Embedded in the binary at compile time — zero disk I/O at runtime
- `Cache-Control: public, max-age=31536000, immutable` — browsers cache for 1 year
- `ETag` supports conditional GET → 304 Not Modified (no payload on re-request)

### Config Validation
```rust
#[derive(Deserialize)]
pub struct Config {
    pub database_url: String,  // required — process exits if missing
    pub redis_url:    String,  // required — process exits if missing
    pub port:         u16,     // optional, default 8080
    pub rust_log:     String,  // optional, default "webapp=info,..."
}
// envy::from_env::<Config>() → fails with clear message on startup
```

---

## Infrastructure

```
┌──────────────────────────────────────────────────────┐
│                  docker compose network              │
│                                                      │
│  ┌──────────┐    ┌───────────┐    ┌──────────────┐  │
│  │  webapp  │───▶│ PgBouncer │───▶│  Postgres 16 │  │
│  │  :8080   │    │  :5432    │    │  :5432       │  │
│  └──────────┘    └───────────┘    └──────────────┘  │
│       │                                              │
│       ▼                                              │
│  ┌──────────┐                                        │
│  │  Redis 7 │                                        │
│  │  :6379   │                                        │
│  └──────────┘                                        │
└──────────────────────────────────────────────────────┘
```

Postgres is not exposed on the host. Only PgBouncer and the webapp are internal to the compose network. The webapp port 8080 is published to the host.

---

## Local Development

```bash
# Prerequisites: Rust, Node 20+, a running Postgres + Redis, sqlx-cli

cargo install sqlx-cli --no-default-features --features postgres
npm install -g tailwindcss@3

cp .env.example .env
# Edit .env: set DATABASE_URL to your local Postgres, REDIS_URL to your Redis

# Create DB and run migrations
sqlx database create --database-url "$DATABASE_URL"
sqlx migrate run    --database-url "$DATABASE_URL"

# Generate .sqlx/ offline metadata (required for SQLX_OFFLINE=true Docker build)
cargo sqlx prepare

# Compile Tailwind CSS
npx tailwindcss -i src/tailwind.css -o static/css/app.css --minify

cargo run
```

---

## Environment Variables

| Variable       | Required | Default           | Description                    |
|----------------|----------|-------------------|--------------------------------|
| `DATABASE_URL` | ✅        | —                 | Postgres / PgBouncer DSN       |
| `REDIS_URL`    | ✅        | —                 | Redis URL                      |
| `PORT`         | ❌        | `8080`            | TCP listen port                |
| `RUST_LOG`     | ❌        | `webapp=info,...` | tracing-subscriber filter      |
