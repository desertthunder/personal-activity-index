use std::fmt;
use thiserror::Error;

/// Errors that can occur in the Personal Activity Index
#[derive(Error, Debug)]
pub enum PaiError {
    #[error("Unknown source kind: {0}")]
    UnknownSourceKind(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Fetch error: {0}")]
    Fetch(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, PaiError>;

/// Represents the different source types supported by the indexer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceKind {
    Substack,
    Bluesky,
    Leaflet,
}

impl fmt::Display for SourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceKind::Substack => write!(f, "substack"),
            SourceKind::Bluesky => write!(f, "bluesky"),
            SourceKind::Leaflet => write!(f, "leaflet"),
        }
    }
}

impl std::str::FromStr for SourceKind {
    type Err = PaiError;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "substack" => Ok(SourceKind::Substack),
            "bluesky" => Ok(SourceKind::Bluesky),
            "leaflet" => Ok(SourceKind::Leaflet),
            _ => Err(PaiError::UnknownSourceKind(s.to_string())),
        }
    }
}

/// Represents a single content item from any source
#[derive(Debug, Clone)]
pub struct Item {
    /// Unique identifier for the item
    pub id: String,
    /// The source type this item came from
    pub source_kind: SourceKind,
    /// The specific source instance identifier (e.g., domain or handle)
    pub source_id: String,
    /// Author of the content
    pub author: Option<String>,
    /// Title of the content
    pub title: Option<String>,
    /// Summary or excerpt of the content
    pub summary: Option<String>,
    /// Canonical URL for the content
    pub url: String,
    /// Full HTML content
    pub content_html: Option<String>,
    /// When the content was published (ISO 8601)
    pub published_at: String,
    /// When this item was created in our database (ISO 8601)
    pub created_at: String,
}

/// Filter criteria for listing items
#[derive(Debug, Default, Clone)]
pub struct ListFilter {
    /// Filter by source kind
    pub source_kind: Option<SourceKind>,
    /// Filter by specific source ID
    pub source_id: Option<String>,
    /// Maximum number of items to return
    pub limit: Option<usize>,
    /// Only items published at or after this time (ISO 8601)
    pub since: Option<String>,
    /// Substring search on title/summary
    pub query: Option<String>,
}

/// Storage trait for persisting and retrieving items
pub trait Storage {
    /// Insert or replace an item in storage
    fn insert_or_replace_item(&self, item: &Item) -> Result<()>;

    /// List items matching the given filter
    fn list_items(&self, filter: &ListFilter) -> Result<Vec<Item>>;
}

/// Trait for fetching content from a specific source
pub trait SourceFetcher {
    /// Synchronize content from this source into storage
    fn sync(&self, storage: &dyn Storage) -> Result<()>;
}

/// Configuration for all sources
#[derive(Debug, Default)]
pub struct Config {}

/// Synchronize all enabled sources
///
/// Calls each configured source fetcher to retrieve and store content.
pub fn sync_all_sources(_config: &Config, _storage: &dyn Storage) -> Result<usize> {
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_kind_display() {
        assert_eq!(SourceKind::Substack.to_string(), "substack");
        assert_eq!(SourceKind::Bluesky.to_string(), "bluesky");
        assert_eq!(SourceKind::Leaflet.to_string(), "leaflet");
    }

    #[test]
    fn source_kind_parse() {
        assert_eq!("substack".parse::<SourceKind>().unwrap(), SourceKind::Substack);
        assert_eq!("BLUESKY".parse::<SourceKind>().unwrap(), SourceKind::Bluesky);
        assert_eq!("Leaflet".parse::<SourceKind>().unwrap(), SourceKind::Leaflet);
        assert!("invalid".parse::<SourceKind>().is_err());
    }

    #[test]
    fn error_unknown_source_kind() {
        let err = "unknown".parse::<SourceKind>().unwrap_err();
        assert!(matches!(err, PaiError::UnknownSourceKind(_)));
        assert_eq!(err.to_string(), "Unknown source kind: unknown");
    }

    #[test]
    fn list_filter_default() {
        let filter = ListFilter::default();
        assert!(filter.source_kind.is_none());
        assert!(filter.source_id.is_none());
        assert!(filter.limit.is_none());
        assert!(filter.since.is_none());
        assert!(filter.query.is_none());
    }
}
