use pai_core::{Item, ListFilter, SourceKind};
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;
use worker::*;

#[derive(Deserialize)]
struct SyncConfig {
    substack: Option<SubstackConfig>,
    bluesky: Option<BlueskyConfig>,
    leaflet: Vec<LeafletConfig>,
}

#[derive(Deserialize)]
struct SubstackConfig {
    base_url: String,
}

#[derive(Deserialize)]
struct BlueskyConfig {
    handle: String,
}

#[derive(Deserialize)]
struct LeafletConfig {
    id: String,
    base_url: String,
}

#[derive(Deserialize)]
struct FeedParams {
    source_kind: Option<SourceKind>,
    source_id: Option<String>,
    limit: Option<usize>,
    since: Option<String>,
    q: Option<String>,
}

#[derive(Serialize)]
struct FeedResponse {
    items: Vec<Item>,
}

#[derive(Serialize)]
struct StatusResponse {
    status: &'static str,
    version: &'static str,
}

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let router = Router::new();
    router
        .get_async("/api/feed", |req, ctx| async move { handle_feed(req, ctx).await })
        .get_async("/api/item/:id", |_req, ctx| async move {
            let id = ctx
                .param("id")
                .ok_or_else(|| Error::RustError("Missing id parameter".into()))?;
            handle_item(id, &ctx).await
        })
        .get("/status", |_req, _ctx| {
            let version = env!("CARGO_PKG_VERSION");
            let status = StatusResponse { status: "ok", version };
            Response::from_json(&status)
        })
        .run(req, env)
        .await
}

#[event(scheduled)]
async fn scheduled(_event: ScheduledEvent, env: Env, _ctx: ScheduleContext) {
    if let Err(e) = run_sync(&env).await {
        console_error!("Scheduled sync failed: {}", e);
    }
}

async fn handle_feed(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let url = req.url()?;
    let params: FeedParams = serde_urlencoded::from_str(url.query().unwrap_or(""))
        .map_err(|e| Error::RustError(format!("Invalid query parameters: {e}")))?;

    let filter = ListFilter {
        source_kind: params.source_kind,
        source_id: params.source_id,
        limit: Some(params.limit.unwrap_or(20)),
        since: params.since,
        query: params.q,
    };

    let db = ctx.env.d1("DB")?;
    let items = query_items(&db, &filter).await?;

    let response = FeedResponse { items };
    Response::from_json(&response)
}

async fn handle_item(id: &str, ctx: &RouteContext<()>) -> Result<Response> {
    let db = ctx.env.d1("DB")?;
    let stmt = db.prepare("SELECT * FROM items WHERE id = ?1").bind(&[id.into()])?;

    let result = stmt.first::<Item>(None).await?;

    match result {
        Some(item) => Response::from_json(&item),
        None => Response::error("Item not found", 404),
    }
}

async fn query_items(db: &D1Database, filter: &ListFilter) -> Result<Vec<Item>> {
    let mut query = String::from(
        "SELECT id, source_kind, source_id, author, title, summary, url, content_html, published_at, created_at FROM items WHERE 1=1"
    );
    let mut bindings = vec![];

    if let Some(kind) = filter.source_kind {
        query.push_str(" AND source_kind = ?");
        bindings.push(kind.to_string().into());
    }

    if let Some(ref source_id) = filter.source_id {
        query.push_str(" AND source_id = ?");
        bindings.push(source_id.clone().into());
    }

    if let Some(ref since) = filter.since {
        query.push_str(" AND published_at >= ?");
        bindings.push(since.clone().into());
    }

    if let Some(ref q) = filter.query {
        query.push_str(" AND (title LIKE ? OR summary LIKE ?)");
        let pattern = format!("%{q}%");
        bindings.push(pattern.clone().into());
        bindings.push(pattern.into());
    }

    query.push_str(" ORDER BY published_at DESC");

    if let Some(limit) = filter.limit {
        query.push_str(" LIMIT ?");
        bindings.push((limit as i64).into());
    }

    let mut stmt = db.prepare(&query);
    for binding in bindings {
        stmt = stmt.bind(&[binding])?;
    }

    let results = stmt.all().await?;
    let items: Vec<Item> = results.results()?;

    Ok(items)
}

async fn run_sync(env: &Env) -> Result<()> {
    let config = load_sync_config(env)?;

    let db = env.d1("DB")?;
    let mut synced = 0;

    if let Some(substack_config) = config.substack {
        match sync_substack(&substack_config, &db).await {
            Ok(count) => {
                console_log!("Synced {} items from Substack", count);
                synced += count;
            }
            Err(e) => console_error!("Substack sync failed: {}", e),
        }
    }

    if let Some(bluesky_config) = config.bluesky {
        match sync_bluesky(&bluesky_config, &db).await {
            Ok(count) => {
                console_log!("Synced {} items from Bluesky", count);
                synced += count;
            }
            Err(e) => console_error!("Bluesky sync failed: {}", e),
        }
    }

    for leaflet_config in config.leaflet {
        match sync_leaflet(&leaflet_config, &db).await {
            Ok(count) => {
                console_log!("Synced {} items from Leaflet ({})", count, leaflet_config.id);
                synced += count;
            }
            Err(e) => console_error!("Leaflet sync failed for {}: {}", leaflet_config.id, e),
        }
    }

    console_log!("Sync completed: {} total items", synced);
    Ok(())
}

fn load_sync_config(env: &Env) -> Result<SyncConfig> {
    let substack = env
        .var("SUBSTACK_URL")
        .ok()
        .map(|url| SubstackConfig { base_url: url.to_string() });

    let bluesky = env
        .var("BLUESKY_HANDLE")
        .ok()
        .map(|handle| BlueskyConfig { handle: handle.to_string() });

    let leaflet = if let Ok(urls) = env.var("LEAFLET_URLS") {
        urls.to_string()
            .split(',')
            .filter_map(|entry| {
                let parts: Vec<&str> = entry.trim().splitn(2, ':').collect();
                if parts.len() == 2 {
                    Some(LeafletConfig { id: parts[0].to_string(), base_url: parts[1].to_string() })
                } else {
                    None
                }
            })
            .collect()
    } else {
        Vec::new()
    };

    Ok(SyncConfig { substack, bluesky, leaflet })
}

async fn sync_substack(config: &SubstackConfig, db: &D1Database) -> Result<usize> {
    let feed_url = format!("{}/feed", config.base_url);

    let mut req = Request::new(&feed_url, Method::Get)?;
    req.headers_mut()?.set("User-Agent", "pai-worker/0.1.0")?;

    let mut resp = Fetch::Request(req).send().await?;
    let body = resp.text().await?;

    let channel =
        rss::Channel::read_from(body.as_bytes()).map_err(|e| Error::RustError(format!("Failed to parse RSS: {e}")))?;

    let source_id = normalize_source_id(&config.base_url);
    let mut count = 0;

    for item in channel.items() {
        let id = item.guid().map(|g| g.value()).unwrap_or(item.link().unwrap_or(""));
        let url = item.link().unwrap_or(id);
        let title = item.title();
        let summary = item.description();
        let author = item.author();
        let content_html = item.content();

        let published_at = item
            .pub_date()
            .and_then(|s| chrono::DateTime::parse_from_rfc2822(s).ok())
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

        let created_at = chrono::Utc::now().to_rfc3339();

        let stmt = db.prepare(
            "INSERT OR REPLACE INTO items (id, source_kind, source_id, author, title, summary, url, content_html, published_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"
        );

        stmt.bind(&[
            id.into(),
            "substack".into(),
            source_id.clone().into(),
            author.map(|s| s.into()).unwrap_or(JsValue::NULL),
            title.map(|s| s.into()).unwrap_or(JsValue::NULL),
            summary.map(|s| s.into()).unwrap_or(JsValue::NULL),
            url.into(),
            content_html.map(|s| s.into()).unwrap_or(JsValue::NULL),
            published_at.into(),
            created_at.into(),
        ])?
        .run()
        .await?;

        count += 1;
    }

    Ok(count)
}

async fn sync_bluesky(config: &BlueskyConfig, db: &D1Database) -> Result<usize> {
    let api_url = format!(
        "https://public.api.bsky.app/xrpc/app.bsky.feed.getAuthorFeed?actor={}&limit=50",
        config.handle
    );

    let mut req = Request::new(&api_url, Method::Get)?;
    req.headers_mut()?.set("User-Agent", "pai-worker/0.1.0")?;

    let mut resp = Fetch::Request(req).send().await?;
    let json: serde_json::Value = resp.json().await?;

    let feed = json["feed"]
        .as_array()
        .ok_or_else(|| Error::RustError("Invalid Bluesky response".into()))?;

    let mut count = 0;

    for item in feed {
        let post = &item["post"];

        if item.get("reason").is_some() {
            continue;
        }

        let uri = post["uri"]
            .as_str()
            .ok_or_else(|| Error::RustError("Missing URI".into()))?;
        let record = &post["record"];
        let text = record["text"].as_str().unwrap_or("");

        let post_id = uri.split('/').next_back().unwrap_or("");
        let url = format!("https://bsky.app/profile/{}/post/{}", config.handle, post_id);

        let title = if text.len() > 100 { format!("{}...", &text[..97]) } else { text.to_string() };

        let published_at = record["createdAt"].as_str().unwrap_or("").to_string();
        let created_at = chrono::Utc::now().to_rfc3339();

        let stmt = db.prepare(
            "INSERT OR REPLACE INTO items (id, source_kind, source_id, author, title, summary, url, content_html, published_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"
        );

        stmt.bind(&[
            uri.into(),
            "bluesky".into(),
            config.handle.clone().into(),
            config.handle.clone().into(),
            title.into(),
            text.into(),
            url.into(),
            JsValue::NULL,
            published_at.into(),
            created_at.into(),
        ])?
        .run()
        .await?;

        count += 1;
    }

    Ok(count)
}

async fn sync_leaflet(config: &LeafletConfig, db: &D1Database) -> Result<usize> {
    let host = normalize_source_id(&config.base_url);
    let subdomain = host.split('.').next().unwrap_or(&host);
    let did = format!("{subdomain}.bsky.social");

    let api_url = format!(
        "https://public.api.bsky.app/xrpc/com.atproto.repo.listRecords?repo={did}&collection=pub.leaflet.post&limit=50"
    );

    let mut req = Request::new(&api_url, Method::Get)?;
    req.headers_mut()?.set("User-Agent", "pai-worker/0.1.0")?;

    let mut resp = Fetch::Request(req).send().await?;
    let json: serde_json::Value = resp.json().await?;

    let records = json["records"]
        .as_array()
        .ok_or_else(|| Error::RustError("Invalid Leaflet response".into()))?;

    let mut count = 0;

    for record in records {
        let uri = record["uri"]
            .as_str()
            .ok_or_else(|| Error::RustError("Missing URI".into()))?;
        let value = &record["value"];

        let title = value["title"].as_str().unwrap_or("Untitled");
        let summary = value["summary"].as_str().or(value["content"].as_str()).unwrap_or("");
        let slug = value["slug"].as_str().unwrap_or("");

        let url = if !slug.is_empty() {
            format!("{}/{}", config.base_url, slug)
        } else {
            format!("{}/post/{}", config.base_url, uri.split('/').next_back().unwrap_or(""))
        };

        let published_at = value["publishedAt"]
            .as_str()
            .or(value["createdAt"].as_str())
            .unwrap_or("")
            .to_string();

        let created_at = chrono::Utc::now().to_rfc3339();

        let stmt = db.prepare(
            "INSERT OR REPLACE INTO items (id, source_kind, source_id, author, title, summary, url, content_html, published_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"
        );

        stmt.bind(&[
            uri.into(),
            "leaflet".into(),
            config.id.clone().into(),
            JsValue::NULL,
            title.into(),
            summary.into(),
            url.into(),
            JsValue::NULL,
            published_at.into(),
            created_at.into(),
        ])?
        .run()
        .await?;

        count += 1;
    }

    Ok(count)
}

fn normalize_source_id(base_url: &str) -> String {
    base_url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_source_id_https() {
        assert_eq!(
            normalize_source_id("https://patternmatched.substack.com"),
            "patternmatched.substack.com"
        );
    }

    #[test]
    fn test_normalize_source_id_http() {
        assert_eq!(normalize_source_id("http://example.com/"), "example.com");
    }

    #[test]
    fn test_normalize_source_id_trailing_slash() {
        assert_eq!(normalize_source_id("https://test.leaflet.pub/"), "test.leaflet.pub");
    }

    #[test]
    fn test_normalize_source_id_no_protocol() {
        assert_eq!(normalize_source_id("example.com"), "example.com");
    }

    #[test]
    fn test_bluesky_title_truncation_short() {
        let text = "Short post";
        let title = if text.len() > 100 { format!("{}...", &text[..97]) } else { text.to_string() };
        assert_eq!(title, "Short post");
    }

    #[test]
    fn test_bluesky_title_truncation_long() {
        let text = "a".repeat(150);
        let title = if text.len() > 100 { format!("{}...", &text[..97]) } else { text.to_string() };
        assert_eq!(title.len(), 100);
        assert!(title.ends_with("..."));
    }

    #[test]
    fn test_bluesky_title_truncation_boundary() {
        let text = "a".repeat(100);
        let title = if text.len() > 100 { format!("{}...", &text[..97]) } else { text.to_string() };
        assert_eq!(title, text);
    }

    #[test]
    fn test_leaflet_url_with_slug() {
        let base_url = "https://test.leaflet.pub";
        let slug = "my-post";
        let url = if !slug.is_empty() {
            format!("{base_url}/{slug}")
        } else {
            format!("{}/post/{}", base_url, "fallback")
        };
        assert_eq!(url, "https://test.leaflet.pub/my-post");
    }

    #[test]
    fn test_leaflet_url_without_slug() {
        let base_url = "https://test.leaflet.pub";
        let slug = "";
        let uri = "at://did:plc:abc123/pub.leaflet.post/xyz789";
        let post_id = uri.split('/').next_back().unwrap_or("");
        let url = if !slug.is_empty() { format!("{base_url}/{slug}") } else { format!("{base_url}/post/{post_id}") };
        assert_eq!(url, "https://test.leaflet.pub/post/xyz789");
    }

    #[test]
    fn test_bluesky_post_id_extraction() {
        let uri = "at://did:plc:abc123/app.bsky.feed.post/3ld7xyqnvqk2a";
        let post_id = uri.split('/').next_back().unwrap_or("");
        assert_eq!(post_id, "3ld7xyqnvqk2a");
    }

    #[test]
    fn test_bluesky_url_construction() {
        let handle = "desertthunder.dev";
        let post_id = "3ld7xyqnvqk2a";
        let url = format!("https://bsky.app/profile/{handle}/post/{post_id}");
        assert_eq!(url, "https://bsky.app/profile/desertthunder.dev/post/3ld7xyqnvqk2a");
    }

    #[test]
    fn test_leaflet_config_parsing() {
        let entry = "desertthunder:https://desertthunder.leaflet.pub";
        let parts: Vec<&str> = entry.trim().splitn(2, ':').collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], "desertthunder");
        assert_eq!(parts[1], "https://desertthunder.leaflet.pub");
    }

    #[test]
    fn test_leaflet_config_parsing_invalid() {
        let entry = "invalid-entry-no-colon";
        let parts: Vec<&str> = entry.trim().splitn(2, ':').collect();
        assert_ne!(parts.len(), 2);
    }

    #[test]
    fn test_leaflet_config_parsing_multiple() {
        let urls = "id1:https://pub1.leaflet.pub,id2:https://pub2.leaflet.pub";
        let configs: Vec<_> = urls
            .split(',')
            .filter_map(|entry| {
                let parts: Vec<&str> = entry.trim().splitn(2, ':').collect();
                if parts.len() == 2 {
                    Some((parts[0].to_string(), parts[1].to_string()))
                } else {
                    None
                }
            })
            .collect();

        assert_eq!(configs.len(), 2);
        assert_eq!(configs[0].0, "id1");
        assert_eq!(configs[0].1, "https://pub1.leaflet.pub");
        assert_eq!(configs[1].0, "id2");
        assert_eq!(configs[1].1, "https://pub2.leaflet.pub");
    }

    #[test]
    fn test_substack_feed_url_construction() {
        let base_url = "https://patternmatched.substack.com";
        let feed_url = format!("{base_url}/feed");
        assert_eq!(feed_url, "https://patternmatched.substack.com/feed");
    }

    #[test]
    fn test_bluesky_api_url_construction() {
        let handle = "desertthunder.dev";
        let api_url = format!("https://public.api.bsky.app/xrpc/app.bsky.feed.getAuthorFeed?actor={handle}&limit=50");
        assert_eq!(
            api_url,
            "https://public.api.bsky.app/xrpc/app.bsky.feed.getAuthorFeed?actor=desertthunder.dev&limit=50"
        );
    }

    #[test]
    fn test_leaflet_did_construction() {
        let subdomain = "desertthunder";
        let did = format!("{subdomain}.bsky.social");
        assert_eq!(did, "desertthunder.bsky.social");
    }

    #[test]
    fn test_leaflet_api_url_construction() {
        let did = "desertthunder.bsky.social";
        let api_url = format!(
            "https://public.api.bsky.app/xrpc/com.atproto.repo.listRecords?repo={did}&collection=pub.leaflet.post&limit=50"
        );
        assert_eq!(
            api_url,
            "https://public.api.bsky.app/xrpc/com.atproto.repo.listRecords?repo=desertthunder.bsky.social&collection=pub.leaflet.post&limit=50"
        );
    }
}
