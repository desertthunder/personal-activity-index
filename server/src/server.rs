use crate::storage::SqliteStorage;

use axum::{
    extract::{Path, Query, Request, State},
    http::{header, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use chrono::DateTime;
use owo_colors::OwoColorize;
use pai_core::{Config, CorsConfig, Item, ListFilter, PaiError, SourceKind};
use rss::{Channel, ChannelBuilder, ItemBuilder};
use serde::{Deserialize, Serialize};
use std::{io, net::SocketAddr, path::PathBuf, sync::Arc, time::Instant};
use tokio::net::TcpListener;

const DEFAULT_LIMIT: usize = 20;
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Launches the HTTP server using the provided config and address.
pub fn serve(config: Config, db_path: PathBuf, address: &str) -> Result<(), PaiError> {
    let addr: SocketAddr = address
        .parse()
        .map_err(|e| PaiError::Config(format!("Invalid listen address '{address}': {e}")))?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(PaiError::Io)?;

    runtime.block_on(async move { run_server(config, db_path, addr).await })
}

async fn run_server(config: Config, db_path: PathBuf, addr: SocketAddr) -> Result<(), PaiError> {
    let storage = SqliteStorage::new(&db_path)?;
    storage.verify_schema()?;
    drop(storage);

    let state =
        AppState { db_path: Arc::new(db_path), start_time: Instant::now(), cors_config: Arc::new(config.cors.clone()) };

    let mut app = Router::new()
        .route("/api/feed", get(feed_handler))
        .route("/api/item/:id", get(item_handler))
        .route("/status", get(status_handler))
        .route("/rss.xml", get(rss_handler))
        .with_state(state.clone());

    if !config.cors.allowed_origins.is_empty() || config.cors.dev_key.is_some() {
        app = app.layer(middleware::from_fn_with_state(state.clone(), cors_middleware));
    }

    let listener = TcpListener::bind(addr).await.map_err(PaiError::Io)?;
    let local_addr = listener.local_addr().map_err(PaiError::Io)?;
    println!("{} Listening on http://{}", "Info:".cyan(), local_addr);

    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|e| io::Error::other(e).into())
}

/// CORS middleware that validates origins and dev keys
async fn cors_middleware(State(state): State<AppState>, request: Request, next: Next) -> Result<Response, StatusCode> {
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let dev_key = request
        .headers()
        .get("x-local-dev-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let method = request.method().clone();

    let is_authorized = if let Some(ref key) = dev_key {
        state.cors_config.is_dev_key_valid(Some(key))
    } else if let Some(ref origin_str) = origin {
        state.cors_config.is_origin_allowed(origin_str)
    } else {
        true
    };

    if method == Method::OPTIONS {
        if !is_authorized {
            return Err(StatusCode::FORBIDDEN);
        }

        let mut response = Response::new(String::new().into());
        if let Some(ref origin_str) = origin {
            response.headers_mut().insert(
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_str(origin_str).unwrap_or(HeaderValue::from_static("*")),
            );
        }
        response.headers_mut().insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, POST, OPTIONS"),
        );
        response.headers_mut().insert(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("Content-Type, X-Local-Dev-Key"),
        );
        response
            .headers_mut()
            .insert(header::ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("3600"));
        return Ok(response);
    }

    if origin.is_some() && !is_authorized {
        return Err(StatusCode::FORBIDDEN);
    }

    let mut response = next.run(request).await;

    if let Some(ref origin_str) = origin {
        if is_authorized {
            response.headers_mut().insert(
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_str(origin_str).unwrap_or(HeaderValue::from_static("*")),
            );
            response.headers_mut().insert(
                header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
                HeaderValue::from_static("true"),
            );
        }
    }

    Ok(response)
}

#[derive(Clone)]
struct AppState {
    db_path: Arc<PathBuf>,
    start_time: Instant,
    cors_config: Arc<CorsConfig>,
}

impl AppState {
    fn open_storage(&self) -> Result<SqliteStorage, PaiError> {
        SqliteStorage::new(self.db_path.as_ref())
    }

    fn status_snapshot(&self) -> Result<StatusResponse, PaiError> {
        let storage = self.open_storage()?;
        let total_items = storage.count_items()?;
        let sources = storage
            .get_stats()?
            .into_iter()
            .map(|(kind, count)| SourceStat { kind, count })
            .collect();

        Ok(StatusResponse {
            status: "ok",
            version: VERSION,
            uptime_seconds: self.start_time.elapsed().as_secs(),
            database_path: self.db_path.display().to_string(),
            total_items,
            sources,
        })
    }
}

#[derive(Debug, Default, Deserialize)]
struct FeedQuery {
    source_kind: Option<SourceKind>,
    source_id: Option<String>,
    limit: Option<usize>,
    since: Option<String>,
    q: Option<String>,
}

impl FeedQuery {
    fn into_filter(self) -> Result<ListFilter, PaiError> {
        let limit = match self.limit {
            Some(value) => ensure_positive_limit(value)?,
            None => DEFAULT_LIMIT,
        };

        Ok(ListFilter {
            source_kind: self.source_kind,
            source_id: normalize_optional_string(self.source_id),
            limit: Some(limit),
            since: normalize_optional_string(self.since),
            query: normalize_optional_string(self.q),
        })
    }
}

#[derive(Serialize)]
struct FeedResponse {
    count: usize,
    items: Vec<Item>,
}

#[derive(Serialize)]
struct StatusResponse {
    status: &'static str,
    version: &'static str,
    uptime_seconds: u64,
    database_path: String,
    total_items: usize,
    sources: Vec<SourceStat>,
}

#[derive(Serialize)]
struct SourceStat {
    kind: String,
    count: usize,
}

async fn feed_handler(
    State(state): State<AppState>, Query(query): Query<FeedQuery>,
) -> Result<Json<FeedResponse>, ApiError> {
    let filter = query.into_filter()?;
    let storage = state.open_storage()?;
    let items = pai_core::Storage::list_items(&storage, &filter)?;

    Ok(Json(FeedResponse { count: items.len(), items }))
}

async fn item_handler(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Item>, ApiError> {
    let storage = state.open_storage()?;
    let item = storage
        .get_item(&id)?
        .ok_or_else(|| ApiError::not_found(format!("Item '{id}' not found")))?;

    Ok(Json(item))
}

async fn status_handler(State(state): State<AppState>) -> Result<Json<StatusResponse>, ApiError> {
    let snapshot = state.status_snapshot()?;
    Ok(Json(snapshot))
}

async fn rss_handler(State(state): State<AppState>, Query(query): Query<FeedQuery>) -> Result<RssResponse, ApiError> {
    let filter = query.into_filter()?;
    let storage = state.open_storage()?;
    let items = pai_core::Storage::list_items(&storage, &filter)?;

    let channel = build_rss_channel(&items)?;
    Ok(RssResponse(channel))
}

fn build_rss_channel(items: &[Item]) -> Result<Channel, PaiError> {
    const TITLE: &str = "Personal Activity Index";
    const LINK: &str = "https://personal-activity-index.local/";
    const DESCRIPTION: &str = "Aggregated feed exported by the Personal Activity Index.";

    let rss_items: Vec<rss::Item> = items
        .iter()
        .map(|item| {
            let title = item
                .title
                .as_deref()
                .or(item.summary.as_deref())
                .unwrap_or(&item.url)
                .to_string();
            let description = item
                .summary
                .as_deref()
                .or(item.content_html.as_deref())
                .unwrap_or("")
                .to_string();
            let author = item.author.as_deref().unwrap_or("Unknown").to_string();
            let pub_date = format_rss_date(&item.published_at);

            ItemBuilder::default()
                .title(Some(title))
                .link(Some(item.url.clone()))
                .guid(Some(
                    rss::GuidBuilder::default().value(&item.id).permalink(false).build(),
                ))
                .pub_date(Some(pub_date))
                .author(Some(author))
                .description(Some(description))
                .categories(vec![rss::CategoryBuilder::default()
                    .name(item.source_kind.to_string())
                    .build()])
                .build()
        })
        .collect();

    let channel = ChannelBuilder::default()
        .title(TITLE)
        .link(LINK)
        .description(DESCRIPTION)
        .items(rss_items)
        .build();

    Ok(channel)
}

fn format_rss_date(value: &str) -> String {
    if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
        dt.to_rfc2822()
    } else if let Ok(dt) = DateTime::parse_from_rfc2822(value) {
        dt.to_rfc2822()
    } else {
        value.to_string()
    }
}

struct RssResponse(Channel);

impl IntoResponse for RssResponse {
    fn into_response(self) -> Response {
        let rss_string = self.0.to_string();
        (
            [(header::CONTENT_TYPE, "application/rss+xml; charset=utf-8")],
            rss_string,
        )
            .into_response()
    }
}

struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(msg: impl Into<String>) -> Self {
        Self { status: StatusCode::BAD_REQUEST, message: msg.into() }
    }

    fn not_found(msg: impl Into<String>) -> Self {
        Self { status: StatusCode::NOT_FOUND, message: msg.into() }
    }

    fn internal(msg: impl Into<String>) -> Self {
        Self { status: StatusCode::INTERNAL_SERVER_ERROR, message: msg.into() }
    }
}

impl From<PaiError> for ApiError {
    fn from(err: PaiError) -> Self {
        match err {
            PaiError::InvalidArgument(msg) => Self::bad_request(msg),
            other => Self::internal(other.to_string()),
        }
    }
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(ErrorBody { error: self.message })).into_response()
    }
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

fn ensure_positive_limit(limit: usize) -> Result<usize, PaiError> {
    if limit == 0 {
        return Err(PaiError::InvalidArgument("Limit must be greater than zero".to_string()));
    }
    Ok(limit)
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value.and_then(|input| {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use pai_core::Storage;
    use tempfile::tempdir;

    #[test]
    fn feed_query_defaults() {
        let filter = FeedQuery::default().into_filter().unwrap();
        assert_eq!(filter.limit, Some(DEFAULT_LIMIT));
        assert!(filter.source_kind.is_none());
        assert!(filter.source_id.is_none());
    }

    #[test]
    fn feed_query_respects_parameters() {
        let query = FeedQuery {
            source_kind: Some(SourceKind::Bluesky),
            source_id: Some(" desertthunder.dev ".to_string()),
            limit: Some(5),
            since: Some("2024-01-01T00:00:00Z".to_string()),
            q: Some(" rust ".to_string()),
        };

        let filter = query.into_filter().unwrap();
        assert_eq!(filter.limit, Some(5));
        assert_eq!(filter.source_kind, Some(SourceKind::Bluesky));
        assert_eq!(filter.source_id.as_deref(), Some("desertthunder.dev"));
        assert_eq!(filter.query.as_deref(), Some("rust"));
        assert_eq!(filter.since.as_deref(), Some("2024-01-01T00:00:00Z"));
    }

    #[test]
    fn feed_query_rejects_zero_limit() {
        let err = FeedQuery { limit: Some(0), ..Default::default() }
            .into_filter()
            .unwrap_err();
        assert!(matches!(err, PaiError::InvalidArgument(_)));
    }

    #[test]
    fn api_error_into_response_sets_status() {
        let resp = ApiError::bad_request("oops").into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn status_snapshot_reports_counts() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("status.db");
        let state = AppState {
            db_path: Arc::new(db_path),
            start_time: Instant::now(),
            cors_config: Arc::new(pai_core::CorsConfig::default()),
        };

        let storage = state.open_storage().unwrap();
        let now = Utc::now().to_rfc3339();
        let item = Item {
            id: "status-test".to_string(),
            source_kind: SourceKind::Substack,
            source_id: "status.substack.com".to_string(),
            author: None,
            title: Some("Status".to_string()),
            summary: None,
            url: "https://example.com/status".to_string(),
            content_html: None,
            published_at: now.clone(),
            created_at: now,
        };
        storage.insert_or_replace_item(&item).unwrap();

        let snapshot = state.status_snapshot().unwrap();
        assert_eq!(snapshot.status, "ok");
        assert_eq!(snapshot.version, VERSION);
        assert!(snapshot.uptime_seconds < 5);
        assert_eq!(snapshot.total_items, 1);
        assert_eq!(snapshot.sources.len(), 1);
        assert_eq!(snapshot.sources[0].kind, "substack");
    }
}
