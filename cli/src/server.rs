use crate::storage::SqliteStorage;
use crate::{ensure_positive_limit, normalize_optional_string, normalize_since_input};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use owo_colors::OwoColorize;
use pai_core::{Item, ListFilter, PaiError, SourceKind};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use tokio::net::TcpListener;

const DEFAULT_LIMIT: usize = 20;

/// Launches the HTTP server using the provided SQLite database path and address.
pub(crate) fn serve(db_path: PathBuf, address: String) -> Result<(), PaiError> {
    let addr: SocketAddr = address
        .parse()
        .map_err(|e| PaiError::Config(format!("Invalid listen address '{address}': {e}")))?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(PaiError::Io)?;

    runtime.block_on(async move { run_server(db_path, addr).await })
}

async fn run_server(db_path: PathBuf, addr: SocketAddr) -> Result<(), PaiError> {
    // Ensure the database exists and schema is ready before serving requests.
    let storage = SqliteStorage::new(&db_path)?;
    storage.verify_schema()?;
    drop(storage);

    let state = AppState { db_path: Arc::new(db_path) };

    let app = Router::new()
        .route("/api/feed", get(feed_handler))
        .route("/api/item/:id", get(item_handler))
        .with_state(state);

    let listener = TcpListener::bind(addr).await.map_err(PaiError::Io)?;
    let local_addr = listener.local_addr().map_err(PaiError::Io)?;
    println!("{} Listening on http://{}", "Info:".cyan(), local_addr);

    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|err| PaiError::Io(std::io::Error::new(std::io::ErrorKind::Other, err)))
}

#[derive(Clone)]
struct AppState {
    db_path: Arc<PathBuf>,
}

impl AppState {
    fn open_storage(&self) -> Result<SqliteStorage, PaiError> {
        SqliteStorage::new(self.db_path.as_ref())
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
            since: normalize_since_input(self.since)?,
            query: normalize_optional_string(self.q),
        })
    }
}

#[derive(Serialize)]
struct FeedResponse {
    count: usize,
    items: Vec<Item>,
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(filter.source_id.unwrap(), "desertthunder.dev");
        assert_eq!(filter.query.unwrap(), "rust");
        assert_eq!(filter.since.unwrap(), "2024-01-01T00:00:00+00:00");
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
}
