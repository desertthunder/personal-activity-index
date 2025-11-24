-- Personal Activity Index D1 Schema
-- This schema is compatible with both SQLite (CLI) and D1 (Worker)

CREATE TABLE IF NOT EXISTS items (
  id            TEXT PRIMARY KEY,
  source_kind   TEXT NOT NULL,
  source_id     TEXT NOT NULL,
  author        TEXT,
  title         TEXT,
  summary       TEXT,
  url           TEXT NOT NULL,
  content_html  TEXT,
  published_at  TEXT NOT NULL,
  created_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_items_source_date ON items (source_kind, source_id, published_at DESC);
CREATE INDEX IF NOT EXISTS idx_items_published ON items (published_at DESC);
