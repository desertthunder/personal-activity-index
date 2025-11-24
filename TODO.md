
# Personal Activity Index CLI – Roadmap & Tasks

Objective:
Build a POSIX-style Rust CLI that ingests content from Substack, Bluesky, and Leaflet into SQLite, with an optional Cloudflare Worker + D1 deployment path.

Targets:

- Self-host: single binary + SQLite.
- Cloudflare: Rust Worker + D1 + Cron triggers.

## Workspace & Architecture

**Goal:** Shared core library, CLI frontend, and Worker frontend, with clear separation of concerns.

- [x] Create Cargo workspace layout:
    - [x] `core/` – shared types, fetchers, and storage traits.
    - [x] `cli/` – POSIX-style binary (`pai`).
    - [x] `worker/` – Cloudflare Worker using `workers-rs`.
- [x] In `core/`:
    - [x] Define `SourceKind` enum: `substack`, `bluesky`, `leaflet`.
    - [x] Define `Item` struct with fields:
        - [x] `id`, `source_kind`, `source_id`, `author`, `title`, `summary`,
      `url`, `content_html`, `published_at`, `created_at`.
    - [x] Define `Storage` trait with at minimum:
        - [x] `insert_or_replace_item(&self, item: &Item) -> Result<()>`
        - [x] `list_items(&self, filter: &ListFilter) -> Result<Vec<Item>>`
    - [x] Define `SourceFetcher` trait:
        - [x] `fn sync(&self, storage: &dyn Storage) -> Result<()>`
- [x] In `cli/`:
    - [x] Add argument parsing that follows POSIX conventions:
        - Options of the form `-h`, `-V`, `-C dir`, `-d path`, etc.
        - Options come before operands/subcommands where possible.
    - [x] Define subcommands (as operands) with their own POSIX-style options:
        - [x] `sync`
        - [x] `list`
        - [x] `export`
        - [x] `serve`
- [x] In `core/`:
    - [x] Implement `sync_all_sources(config, storage)` that calls each fetcher.

## Milestone 1 – Local SQLite Storage (Self-host Base)

**Goal:** `pai` can sync data into a local SQLite file.

- [x] Choose SQLite crate (native mode):
    - [x] e.g. `rusqlite`
- [x] Define SQL schema and migrations:
    - [x] `items` table:

    ```sql
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
    ```

    - [x] Embed migrations or provide `schema.sql` + `pai db-migrate` command.
- [x] Implement `SqliteStorage` in `cli/`:
    - [x] Opens/creates DB at `-d path` or `$XDG_DATA_HOME/pai/pai.db` fallback.
    - [x] Implements `Storage` trait.
- [x] Implement `pai sync` path:
    - [x] `pai sync` → load config → open SQLite → call `sync_all_sources`.
    - [x] Exit codes:
        - [x] `0` on success, non-zero on failure.
- [x] Add `pai db-check`:
    - [x] Verifies schema and prints basic stats (item count per source).

## Milestone 2 – Source Integrations ✅

**Goal:** All three sources can be ingested via the CLI.

**Status:** COMPLETE - All three source integrations (Substack RSS, Bluesky AT Protocol, Leaflet RSS) are implemented and tested with real data.

### 2.1 Substack (Pattern Matched)

- [x] Add config support:

  ```toml
  [sources.substack]
  enabled   = true
  base_url  = "https://patternmatched.substack.com"
  ```

- [x] Implement `SubstackFetcher` in `core/`:

    - [x] Fetch `{base_url}/feed`.
    - [x] Parse RSS using `feed-rs`.
    - [x] Map `<item>`:

        - [x] `id` = GUID if present, otherwise `link`.
        - [x] `source_kind = "substack"`.
        - [x] `source_id = "patternmatched.substack.com"`.
        - [x] `title`, `summary` from RSS `title`/`description`.
        - [x] `url` from `link`.
        - [x] `published_at` from `pubDate` (normalized to ISO 8601).
- [x] Wire into `sync_all_sources` when enabled.

### 2.2 Bluesky (desertthunder.dev)

- [x] Add config support:

  ```toml
  [sources.bluesky]
  enabled = true
  handle  = "desertthunder.dev"
  ```

- [x] Implement `BlueskyFetcher` in `core/`:

    - [x] Fetch:

        - [x] `https://public.api.bsky.app/xrpc/app.bsky.feed.getAuthorFeed?actor=desertthunder.dev&limit=N`
    - [x] Filter out reposts/quotes (only original posts).
    - [x] Map `post` record:

        - [x] `id` = `uri` (AT URI).
        - [x] `source_kind = "bluesky"`.
        - [x] `source_id = "desertthunder.dev"`.
        - [x] `title` = truncated text up to N chars.
        - [x] `summary` = full text (or truncated).
        - [x] `url` = canonical `https://bsky.app/profile/…/post/…` derived from URI.
        - [x] `published_at` = `record.createdAt` (ISO 8601 already).
    - [ ] Optional:

        - [ ] Support pagination via `cursor` until a configured max number of posts.

### 2.3 Leaflet (desertthunder / stormlightlabs)

- [x] Add config support:

  ```toml
  [[sources.leaflet]]
  enabled    = true
  id         = "desertthunder"
  base_url   = "https://desertthunder.leaflet.pub"

  [[sources.leaflet]]
  enabled    = true
  id         = "stormlightlabs"
  base_url   = "https://stormlightlabs.leaflet.pub"
  ```

- [x] Use AT Protocol instead of HTML parsing:

    - [x] Use `com.atproto.repo.listRecords` with collection `pub.leaflet.post`.

- [x] Implement `LeafletFetcher` in `core/`:

    - [x] For each configured pub:

        - [x] Fetch records using AT Protocol.
        - [x] Parse `pub.leaflet.post` records.
        - [x] For each post:

            - [x] Extract `title` from record.
            - [x] Extract `publishedAt` or `createdAt`.
            - [x] Derive summary from `summary` or `content` field.
            - [x] Generate URL using `slug` or record ID.
            - [x] Normalize date to ISO 8601 for `published_at`.
        - [x] Insert or replace items in storage.

- [x] Wire into `sync_all_sources`.

## Milestone 3 – Query, Filter, and Export (CLI Only)

**Goal:** Make local data usable even without HTTP.

- [x] Implement `pai list`:
    - [x] Syntax: `pai list [options]` (options before operands).
    - [x] Options:
        - [x] `-k kind` filter by `source_kind` (`substack`, `bluesky`, `leaflet`).
        - [x] `-S id` filter by `source_id` (host/handle).
        - [x] `-n N` limit number of results (default 20).
        - [x] `-s time` “since time” (e.g. ISO 8601, or “7d” shorthand if desired).
        - [x] `-q pattern` simple substring filter on title/summary.
    - [x] Render as ASCII table or simple text.
- [x] Implement `pai export`:
    - [x] Syntax: `pai export -f format [-o file]`.
    - [x] Supported formats:
        - [x] `json` (default).
        - [x] `ndjson` (optional).
        - [x] `rss` (optional aggregate).
    - [x] Options:
        - [x] `-f format` (`json`, `rss`, …).
        - [x] `-o path` output file (default stdout).
- [x] Implement exit statuses for typical cases:
    - [x] `0` on success.
    - [x] `>0` on error (bad args, DB error, network failure, etc.).

## Milestone 4 – Self-hosted HTTP Server Mode

**Goal:** Provide a small HTTP API backed by SQLite for self-hosted deployments.

- [x] Add `serve` subcommand in `cli/`:
    - [x] Syntax: `pai serve [options]`.
    - [x] Options:
        - [x] `-d path` database path.
        - [x] `-a addr` listen address (default `127.0.0.1:8080`).
    - [x] Follows POSIX conventions: all options before operands.
- [x] Implement HTTP server (`axum`):
    - [x] `GET /api/feed` – list all items, newest first.
    - [x] Query params:
        - [x] `source_kind`, `source_id`, `limit`, `since`, `q`.
    - [x] Optional:
        - [x] `GET /api/item/{id}` for a single item.
- [x] Ensure graceful shutdown and clean error handling.
- [x] Document reverse-proxy examples (Caddy, nginx).

## Milestone 5 – Cloudflare Worker + D1 Frontend

**Goal:** Provide an alternative deployment path using Cloudflare Workers with D1 and Cron triggers.

- [ ] In `worker/`:
    - [ ] Depend on `worker` crate with `d1` feature enabled.
    - [ ] Reuse `core::Item` and parsing code (ensure crates are WASM-friendly).
- [ ] Configure D1:
    - [ ] Provide `schema.sql` compatible with D1 (same `items` table).
    - [ ] Example `wrangler.toml` with `[[d1_databases]]` binding.
- [ ] Implement Worker routes:
    - [ ] `GET /api/feed` with similar semantics as CLI server.
- [ ] Implement `scheduled` handler for Cron:
    - [ ] On each scheduled run, call per-source syncers writing to D1.
    - [ ] Document cron configuration in `wrangler.toml`.
- [ ] Add `pai cf-init` in `cli/`:
    - [ ] Generates a starter `wrangler.toml`.
    - [ ] Prints instructions to create D1 DB and bind it.

## Milestone 6 – POSIX Polish, Packaging, and Docs

**Goal:** Make the CLI feel like a “real UNIX utility” and easy to adopt.

- [ ] Verify POSIX-style argument handling:
    - [ ] Short options only in usage syntax; long options are optional extensions.
    - [ ] Options before operands/subcommands in docs and examples.
    - [ ] Support grouped short options where meaningful (e.g. `-hv`).
- [ ] Implement:
    - [ ] `-h` – usage synopsis and options (per POSIX convention).
    - [ ] `-V` – version info.
- [ ] Add manpage-style documentation using clap_mangen (<https://crates.io/crates/clap_mangen>) in build.rs:
    - [ ] `man/pai.1` with SYNOPSIS, DESCRIPTION, OPTIONS, OPERANDS, EXIT STATUS, ENVIRONMENT, FILES, EXAMPLES.
- [ ] Publish `pai` crate to crates.io.
- [ ] Write README with:
    - [ ] Self-hosted quickstart.
    - [ ] Cloudflare Worker quickstart.
    - [ ] Config reference (`config.toml`).

## 2. CLI & Config Spec (POSIX-style)

### 2.1 POSIX argument conventions you’re aligning with

Key constraints you want to follow:

- Options are introduced by a single `-` followed by a single letter (`-h`, `-V`, `-d path`). :contentReference[oaicite:0]{index=0}
- Options that require arguments use a separate token: `-d path` rather than `-dpath`. :contentReference[oaicite:1]{index=1}
- Options appear before operands (here, subcommands and file paths) in the recommended syntax:
  `utility_name [-a] [-b arg] operand1 operand2 …`. :contentReference[oaicite:2]{index=2}
- `-h` for help, `-V` for version are widely conventional. :contentReference[oaicite:3]{index=3}

You *can* still offer `--long-option` aliases as a GNU-style extension; just document the POSIX short forms as canonical. :contentReference[oaicite:4]{index=4}

### 2.2 CLI synopsis

**Utility name:** `pai` (single binary).

#### Global synopsis

```text
pai [-hV] [-C config_dir] [-d db_path] command [command-options] [command-operands]
```

- `-h`
  Print usage and exit.

- `-V`
  Print version and exit.

- `-C config_dir`
  Set configuration directory. Default: `$XDG_CONFIG_HOME/pai` or `$HOME/.config/pai`.

- `-d db_path`
  Path to SQLite database file. Default: `$XDG_DATA_HOME/pai/pai.db` or `$HOME/.local/share/pai/pai.db`.

Subcommands are treated as **operands** in POSIX terms; each subcommand then has its own POSIX-style options.

### 2.3 Subcommands and their options

#### 1. `sync` – fetch and store content

```text
pai [-C config_dir] [-d db_path] sync [-a] [-k kind] [-S source_id]
```

Options:

- `-a`
  Sync all configured sources (default if `-k` not specified).

- `-k kind`
  Sync only a particular source kind:

    - `substack`
    - `bluesky`
    - `leaflet`

- `-S source_id`
  Sync only a specific source instance (e.g. `patternmatched.substack.com`, `desertthunder.dev`, `desertthunder.leaflet.pub`, `stormlightlabs.leaflet.pub`).

Examples:

```sh
pai sync -a
pai sync -k substack
pai sync -k leaflet -S desertthunder.leaflet.pub
```

#### 2. `list` – inspect stored items

```text
pai [-C config_dir] [-d db_path] list [-k kind] [-S source_id] [-n number] [-s since] [-q pattern]
```

Options:

- `-k kind`
  Filter by source kind (`substack`, `bluesky`, `leaflet`).

- `-S source_id`
  Filter by specific source id (host or handle).

- `-n number`
  Maximum number of items to display (default 20).

- `-s since`
  Only show items published at or after this time. The CLI can accept ISO 8601 (`2025-11-23T00:00:00Z`) and, as a convenience, relative strings like `7d`, `24h` if you want.

- `-q pattern`
  Filter items whose title/summary contains the given substring.

#### 3. `export` – produce feeds/files

```text
pai [-C config_dir] [-d db_path] export [-k kind] [-S source_id] [-n number] [-s since] [-q pattern] [-f format] [-o file]
```

Options (in addition to `list` filters):

- `-f format`
  Output format:

    - `json` (default)
    - `ndjson`
    - `rss` (optional)

- `-o file`
  Output file. Default is standard output.

Examples:

```sh
pai export -f json -o activity.json
pai export -k bluesky -n 50 -f ndjson
```

#### 4. `serve` – self-host HTTP API

```text
pai [-C config_dir] [-d db_path] serve [-a address]
```

Options:

- `-a address`
  Address to bind HTTP server to. Default: `127.0.0.1:8080`.

The HTTP API mirrors the query semantics of `list` and `export`:

- `GET /api/feed?source_kind=bluesky&limit=50&since=...&q=...`

#### 5. `cf-init` – scaffold Cloudflare deployment

```text
pai cf-init [-o dir]
```

Options:

- `-o dir`
  Directory into which to write `wrangler.toml`, `schema.sql`, and a sample `worker` entry point. Default: current directory.

This command doesn’t need DB access; it just writes templates and prints next steps (create D1 DB, bind it, set up Cron).

### 2.4 `config.toml` spec

**Default location:**

- `$XDG_CONFIG_HOME/pai/config.toml` or
- `$HOME/.config/pai/config.toml` if `XDG_CONFIG_HOME` is unset.

**Top-level layout:**

```toml
[database]
# Path to SQLite database for self-host mode.
# Ignored by the Worker; used only by `pai` binary.
path = "/home/owais/.local/share/pai/pai.db"

[deployment]
# Which deploy targets are configured.
# "sqlite" is always available; "cloudflare" is optional.
mode = "sqlite"        # or "cloudflare"

[deployment.cloudflare]
# Optional metadata for generating wrangler.toml, etc.
worker_name   = "personal-activity-index"
d1_binding    = "DB"
database_name = "personal_activity_db"

[sources.substack]
enabled   = true
base_url  = "https://patternmatched.substack.com"

[sources.bluesky]
enabled = true
handle  = "desertthunder.dev"

[[sources.leaflet]]
enabled   = true
id        = "desertthunder"
base_url  = "https://desertthunder.leaflet.pub"

[[sources.leaflet]]
enabled   = true
id        = "stormlightlabs"
base_url  = "https://stormlightlabs.leaflet.pub"
```

**Notes:**

- The CLI should **not** require the Cloudflare section unless a user explicitly wants to generate Worker scaffolding.
- The Worker itself will get its D1 binding and Cron schedule from `wrangler.toml` and the Cloudflare dashboard, not from this config file; you just reuse the same schema and `Item` type.

### 2.5 POSIX compliance checklist

When you implement the CLI parsing, you can sanity-check against POSIX & GNU guidance:

- Short options are single letters with a single leading `-`. ([The Open Group][1])
- Options precede non-option arguments (your commands and operands) in the usage examples. ([The Open Group][1])
- Options that take arguments are formatted as `-x arg` rather than `-xarg` in documentation. ([gnu.org][2])
- You provide `-h` / `-V` and consistent help text. ([Baeldung on Kotlin][3])
- Long options (`--help`, `--version`, `--config-dir`, etc.) can be supported as extensions but are not required for conformance. ([Software Engineering Stack Exchange][4])

[1]: https://pubs.opengroup.org/onlinepubs/9699919799/basedefs/V1_chap12.html "12. Utility Conventions"
[2]: https://www.gnu.org/s/libc/manual/html_node/Argument-Syntax.html "Argument Syntax (The GNU C Library)"
[3]: https://www.baeldung.com/linux/posix "A Guide to POSIX | Baeldung on Linux"
[4]: https://softwareengineering.stackexchange.com/questions/70357/command-line-options-style-posix-or-what "Command line options style - POSIX or what?"
