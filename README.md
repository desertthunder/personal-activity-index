<!-- markdownlint-disable MD033 -->

# Personal Activity Index

A CLI that ingests content from Substack, Bluesky, and Leaflet into SQLite, with an optional Cloudflare Worker + D1 deployment path.

## Features

- Fetch posts from multiple sources:
    - **Substack** via RSS feeds
    - **Bluesky** via AT Protocol
    - **Leaflet** publications via RSS feeds
- Local SQLite storage with full-text search
- Flexible filtering and querying via `pai list` / `pai export`
- Self-hostable HTTP API (`pai serve` exposes `/api/feed`, `/api/item/{id}`, and `/status`)
- Cloudflare Worker deployment path (D1) for serverless setups

## Quick Start

```bash
# Install
cargo install --path cli

# Initialize config (creates ~/.config/pai/config.toml)
pai init

# Edit config with your sources
$EDITOR ~/.config/pai/config.toml

# Sync content
pai sync

# List items
pai list -n 10

# Check database
pai db-check

# Install the manpage so `man pai` works
pai man --install

# Generate manpage to a file
pai man -o pai.1
```

<details>
<summary>For server mode, run the built-in HTTP server against your SQLite database:</summary>

<br>

```bash
pai serve -d /var/lib/pai/pai.db -a 127.0.0.1:8080
```

Endpoints:

- `GET /api/feed` – list newest items (supports `source_kind`, `source_id`, `limit`, `since`, `q`)
- `GET /api/item/{id}` – fetch a single item
- `GET /status` – health/status summary (total items, counts per source)

For reverse-proxy examples (nginx, Caddy, Docker), see [DEPLOYMENT.md](./DEPLOYMENT.md).

</details>

## Configuration

Configuration is loaded from `$XDG_CONFIG_HOME/pai/config.toml` or `$HOME/.config/pai/config.toml`.

See [config.example.toml](./config.example.toml) for a complete example with all available options.

## Documentation

- CLI synopsis: `pai -h`, `pai <command> -h`, or `pai man` for the generated `pai(1)` page.
- `pai man --install [--install-dir DIR]` copies `pai.1` into a MANPATH directory (defaults to `~/.local/share/man/man1`) so `man pai` works like any other UNIX tool.
- Database schema and config reference: [config.example.toml](./config.example.toml).
- Deployment topologies: [DEPLOYMENT.md](./DEPLOYMENT.md).

## Architecture

The project is organized as a Cargo workspace

```sh
.
├── core    # Shared types, fetchers, and the storage trait
├── cli     # CLI binary (POSIX-compliant)
└── worker  # Cloudflare Worker deployment using workers-rs
```

<details>
<summary><strong>Source Implementations</strong></summary>

### Substack (RSS)

Substack fetcher uses standard RSS 2.0 feeds available at `{base_url}/feed`.

**Implementation:**

- Fetches RSS feed using `feed-rs` parser
- Maps RSS `<item>` elements to standardized `Item` struct
- Uses GUID as item ID, falls back to link if GUID is missing
- Normalizes `pubDate` to ISO 8601 format

**Key mappings:**

- `id` = RSS GUID or link
- `source_kind` = `substack`
- `source_id` = Domain extracted from base_url
- `title` = RSS title
- `summary` = RSS description
- `url` = RSS link
- `content_html` = RSS content (if available)
- `published_at` = RSS pubDate (normalized to ISO 8601)

**Example RSS structure:**

```xml
<item>
    <title>Post Title</title>
    <link>https://example.substack.com/p/post-slug</link>
    <guid>https://example.substack.com/p/post-slug</guid>
    <pubDate>Mon, 01 Jan 2024 12:00:00 +0000</pubDate>
    <description>Post summary or excerpt</description>
</item>
```

### AT Protocol Integration (Bluesky)

#### Overview

Bluesky is built on the AT Protocol (Authenticated Transfer Protocol), a decentralized social networking protocol.

**Key Concepts:**

- **DID (Decentralized Identifier)**: Unique identifier for users (e.g., `did:plc:xyz123`)
- **Handle**: Human-readable identifier (e.g., `user.bsky.social`)
- **AT URI**: Resource identifier (e.g., `at://did:plc:xyz/app.bsky.feed.post/abc123`)
- **Lexicon**: Schema definition language for records and API methods
- **XRPC**: HTTP API wrapper for AT Protocol methods
- **PDS (Personal Data Server)**: Server that stores user data

#### Implementation

Bluesky uses standard `app.bsky.feed.post` records and provides a public API for fetching posts.

**Endpoint:** `GET https://public.api.bsky.app/xrpc/app.bsky.feed.getAuthorFeed`

**Parameters:**

- `actor` - User handle or DID
- `limit` - Number of posts to fetch (default: 50)
- `cursor` - Pagination cursor (optional)

**Implementation:**

- Fetches author feed using `app.bsky.feed.getAuthorFeed`
- Filters out reposts and quotes (only includes original posts)
- Converts AT URIs to canonical Bluesky URLs
- Truncates long post text to create titles

**Key mappings:**

- `id` = AT URI (e.g., `at://did:plc:xyz/app.bsky.feed.post/abc123`)
- `source_kind` = `bluesky`
- `source_id` = User handle
- `title` = Truncated post text (first 100 chars)
- `summary` = Full post text
- `url` = Canonical URL (`https://bsky.app/profile/{handle}/post/{post_id}`)
- `author` = Post author handle
- `published_at` = Post `createdAt` timestamp

**Filtering reposts:**
Posts with a `reason` field (indicating repost or quote) are excluded to fetch only original content.

### Leaflet (RSS)

#### Overview

Leaflet publications provide RSS feeds at `{base_url}/rss`, making them straightforward to fetch using standard RSS parsing.

**Note:** While Leaflet is built on AT Protocol and uses custom `pub.leaflet.post` records, we use RSS feeds for simplicity and reliability. Leaflet's RSS implementation provides all necessary metadata without requiring AT Protocol PDS queries.

**Implementation:**

- Fetches RSS feed using `feed-rs` parser
- Maps RSS `<item>` elements to standardized `Item` struct
- Supports multiple publications via config array
- Uses entry ID from feed, falls back to link if missing
- Normalizes publication dates to ISO 8601 format

**Key mappings:**

- `id` = RSS entry ID or link
- `source_kind` = `leaflet`
- `source_id` = Publication ID from config (e.g., `desertthunder`, `stormlightlabs`)
- `title` = RSS entry title
- `summary` = RSS entry summary/description
- `url` = RSS entry link
- `content_html` = RSS content body (if available)
- `author` = RSS entry author
- `published_at` = RSS published date or updated date (normalized to ISO 8601)

**Configuration:**

Leaflet supports multiple publications through array configuration:

```toml
[[sources.leaflet]]
enabled = true
id = "desertthunder"
base_url = "https://desertthunder.leaflet.pub"

[[sources.leaflet]]
enabled = true
id = "stormlightlabs"
base_url = "https://stormlightlabs.leaflet.pub"
```

**Example RSS structure:**

```xml
<item>
    <title>Dev Log: 2025-11-22</title>
    <link>https://desertthunder.leaflet.pub/3m6a7fuk7u22p</link>
    <guid>https://desertthunder.leaflet.pub/3m6a7fuk7u22p</guid>
    <pubDate>Fri, 22 Nov 2025 16:22:54 +0000</pubDate>
    <description>Post summary or excerpt</description>
</item>
```

</details>

## References

- [AT Protocol Documentation](https://atproto.com)
- [Lexicon Guide](https://atproto.com/guides/lexicon) - Schema definition language
- [XRPC Specification](https://atproto.com/specs/xrpc) - HTTP API wrapper
- [Bluesky API Documentation](https://docs.bsky.app/)
- [Leaflet](https://tangled.org/leaflet.pub/leaflet) - Leaflet source code
- [Leaflet Manual](https://about.leaflet.pub/) - User-facing documentation

## License

See [LICENSE](./LICENSE)
