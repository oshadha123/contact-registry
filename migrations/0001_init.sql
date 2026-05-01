-- Contact Registry v3 — Postgres schema
-- Replaces the SQLite version; runs automatically via sqlx::migrate! on startup.

CREATE TABLE IF NOT EXISTS records (
    id         BIGSERIAL    PRIMARY KEY,
    first_name TEXT         NOT NULL,
    last_name  TEXT         NOT NULL,
    phone      TEXT         NOT NULL,
    address    TEXT         NOT NULL,
    age        INTEGER      NOT NULL,
    created_at TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

-- Keyset pagination scans id DESC; this index makes it O(log n) not O(n).
CREATE INDEX IF NOT EXISTS idx_records_id_desc ON records (id DESC);
