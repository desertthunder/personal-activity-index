#[cfg(not(target_arch = "wasm32"))]
mod fetchers;

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::{fmt, str::FromStr};
use thiserror::Error;

#[cfg(not(target_arch = "wasm32"))]
pub use fetchers::{BearBlogFetcher, BlueskyFetcher, LeafletFetcher, SubstackFetcher};

/// Errors that can occur in the Personal Activity Index
#[derive(Error, Debug)]
pub enum PaiError {
    #[error("Unknown source kind: {0}")]
    UnknownSourceKind(String),

    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Substack,
    Bluesky,
    Leaflet,
    BearBlog,
}

impl fmt::Display for SourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceKind::Substack => write!(f, "substack"),
            SourceKind::Bluesky => write!(f, "bluesky"),
            SourceKind::Leaflet => write!(f, "leaflet"),
            SourceKind::BearBlog => write!(f, "bearblog"),
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
            "bearblog" => Ok(SourceKind::BearBlog),
            _ => Err(PaiError::UnknownSourceKind(s.to_string())),
        }
    }
}

/// Represents a single content item from any source
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Configuration for Substack source
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SubstackConfig {
    #[serde(default)]
    pub enabled: bool,
    pub base_url: String,
}

/// Configuration for Bluesky source
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BlueskyConfig {
    #[serde(default)]
    pub enabled: bool,
    pub handle: String,
}

/// Configuration for a single Leaflet publication
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LeafletConfig {
    #[serde(default)]
    pub enabled: bool,
    pub id: String,
    pub base_url: String,
}

/// Configuration for a single BearBlog publication
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BearBlogConfig {
    #[serde(default)]
    pub enabled: bool,
    pub id: String,
    pub base_url: String,
}

/// Database configuration
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct DatabaseConfig {
    pub path: Option<String>,
}

/// Deployment mode configuration
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct DeploymentConfig {
    #[serde(default)]
    pub mode: String,
    pub cloudflare: Option<CloudflareConfig>,
}

/// Cloudflare deployment configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CloudflareConfig {
    pub worker_name: String,
    pub d1_binding: String,
    pub database_name: String,
}

/// Sources configuration section
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SourcesConfig {
    pub substack: Option<SubstackConfig>,
    pub bluesky: Option<BlueskyConfig>,
    #[serde(default)]
    pub leaflet: Vec<LeafletConfig>,
    #[serde(default)]
    pub bearblog: Vec<BearBlogConfig>,
}

/// CORS configuration for the HTTP server and Worker
///
/// Supports same-root-domain CORS (e.g., pai.desertthunder.dev from desertthunder.dev)
/// and local development with a dev key header.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct CorsConfig {
    /// List of allowed origins (exact match or same-root-domain)
    /// Example: ["https://desertthunder.dev", "http://localhost:4321"]
    #[serde(default)]
    pub allowed_origins: Vec<String>,

    /// Optional development key for local development
    /// When set, requests with X-LOCAL-DEV-KEY header matching this value are allowed
    pub dev_key: Option<String>,
}

impl CorsConfig {
    /// Check if an origin is allowed based on exact match or same-root-domain logic.
    ///
    /// Same-root-domain means extracting the root domain (last two parts) from both
    /// the origin and allowed origins, and checking for a match.
    ///
    /// Examples:
    /// - https://pai.desertthunder.dev is allowed if https://desertthunder.dev is in allowed_origins
    /// - http://localhost:4321 requires exact match
    pub fn is_origin_allowed(&self, origin: &str) -> bool {
        if self.allowed_origins.is_empty() {
            return false;
        }

        let origin_domain = extract_domain(origin);

        for allowed in &self.allowed_origins {
            if origin == allowed {
                return true;
            }

            let allowed_domain = extract_domain(allowed);
            if let (Some(origin_root), Some(allowed_root)) = (
                extract_root_domain(&origin_domain),
                extract_root_domain(&allowed_domain),
            ) {
                if origin_root == allowed_root {
                    return true;
                }
            }
        }

        false
    }

    /// Validate if a dev key matches the configured dev key
    pub fn is_dev_key_valid(&self, key: Option<&str>) -> bool {
        match (&self.dev_key, key) {
            (Some(config_key), Some(request_key)) => config_key == request_key,
            _ => false,
        }
    }
}

/// Extract domain from URL (removes protocol and path)
fn extract_domain(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_string()
}

/// Extract root domain (last two parts of domain)
/// Example: "pai.desertthunder.dev" -> Some("desertthunder.dev")
/// Example: "localhost" -> None (single part)
fn extract_root_domain(domain: &str) -> Option<String> {
    let parts: Vec<&str> = domain.split('.').collect();
    if parts.len() >= 2 {
        Some(format!("{}.{}", parts[parts.len() - 2], parts[parts.len() - 1]))
    } else {
        None
    }
}

/// Configuration for all sources
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Config {
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub deployment: DeploymentConfig,
    #[serde(default)]
    pub sources: SourcesConfig,
    #[serde(default)]
    pub cors: CorsConfig,
}

impl Config {
    /// Load configuration from a TOML file
    ///
    /// Reads and parses the config file, validating the structure and required fields.
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content =
            std::fs::read_to_string(path).map_err(|e| PaiError::Config(format!("Failed to read config file: {e}")))?;
        Self::from_str(&content)
    }
}

impl FromStr for Config {
    type Err = PaiError;

    fn from_str(s: &str) -> Result<Self> {
        toml::from_str(s).map_err(|e| PaiError::Config(format!("Failed to parse config: {e}")))
    }
}

/// Synchronize all enabled sources
///
/// Calls each configured source fetcher to retrieve and store content.
/// Returns the number of sources successfully synced.
///
/// Filters sources based on optional kind and source_id parameters.
#[cfg(not(target_arch = "wasm32"))]
pub fn sync_all_sources(
    config: &Config, storage: &dyn Storage, kind: Option<SourceKind>, source_id: Option<&str>,
) -> Result<usize> {
    let mut synced_count = 0;

    if let Some(ref substack_config) = config.sources.substack {
        let should_sync = substack_config.enabled
            && match (kind, source_id) {
                (Some(k), _) if k != SourceKind::Substack => false,
                (_, Some(sid)) => {
                    let substack_id = substack_config
                        .base_url
                        .trim_start_matches("https://")
                        .trim_start_matches("http://")
                        .trim_end_matches('/');
                    substack_id == sid
                }
                _ => true,
            };

        if should_sync {
            let fetcher = SubstackFetcher::new(substack_config.clone());
            fetcher.sync(storage)?;
            synced_count += 1;
        }
    }

    if let Some(ref bluesky_config) = config.sources.bluesky {
        let should_sync = bluesky_config.enabled
            && match (kind, source_id) {
                (Some(k), _) if k != SourceKind::Bluesky => false,
                (_, Some(sid)) => bluesky_config.handle == sid,
                _ => true,
            };

        if should_sync {
            let fetcher = BlueskyFetcher::new(bluesky_config.clone());
            fetcher.sync(storage)?;
            synced_count += 1;
        }
    }

    for leaflet_config in &config.sources.leaflet {
        if !leaflet_config.enabled {
            continue;
        }

        let should_sync = match (kind, source_id) {
            (Some(k), _) if k != SourceKind::Leaflet => false,
            (_, Some(sid)) => leaflet_config.id == sid,
            _ => true,
        };

        if should_sync {
            let fetcher = LeafletFetcher::new(leaflet_config.clone());
            fetcher.sync(storage)?;
            synced_count += 1;
        }
    }

    for bearblog_config in &config.sources.bearblog {
        if !bearblog_config.enabled {
            continue;
        }

        let should_sync = match (kind, source_id) {
            (Some(k), _) if k != SourceKind::BearBlog => false,
            (_, Some(sid)) => bearblog_config.id == sid,
            _ => true,
        };

        if should_sync {
            let fetcher = BearBlogFetcher::new(bearblog_config.clone());
            fetcher.sync(storage)?;
            synced_count += 1;
        }
    }

    Ok(synced_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_kind_display() {
        assert_eq!(SourceKind::Substack.to_string(), "substack");
        assert_eq!(SourceKind::Bluesky.to_string(), "bluesky");
        assert_eq!(SourceKind::Leaflet.to_string(), "leaflet");
        assert_eq!(SourceKind::BearBlog.to_string(), "bearblog");
    }

    #[test]
    fn source_kind_parse() {
        assert_eq!("substack".parse::<SourceKind>().unwrap(), SourceKind::Substack);
        assert_eq!("BLUESKY".parse::<SourceKind>().unwrap(), SourceKind::Bluesky);
        assert_eq!("Leaflet".parse::<SourceKind>().unwrap(), SourceKind::Leaflet);
        assert_eq!("bearblog".parse::<SourceKind>().unwrap(), SourceKind::BearBlog);
        assert_eq!("BEARBLOG".parse::<SourceKind>().unwrap(), SourceKind::BearBlog);
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

    #[test]
    fn config_parse_empty() {
        let config = Config::from_str("").unwrap();
        assert!(config.sources.substack.is_none());
        assert!(config.sources.bluesky.is_none());
        assert!(config.sources.leaflet.is_empty());
    }

    #[test]
    fn config_parse_substack() {
        let toml = r#"
[sources.substack]
enabled = true
base_url = "https://patternmatched.substack.com"
"#;
        let config = Config::from_str(toml).unwrap();
        let substack = config.sources.substack.as_ref().unwrap();
        assert!(substack.enabled);
        assert_eq!(substack.base_url, "https://patternmatched.substack.com");
    }

    #[test]
    fn config_parse_bluesky() {
        let toml = r#"
[sources.bluesky]
enabled = true
handle = "desertthunder.dev"
"#;
        let config = Config::from_str(toml).unwrap();
        let bluesky = config.sources.bluesky.as_ref().unwrap();
        assert!(bluesky.enabled);
        assert_eq!(bluesky.handle, "desertthunder.dev");
    }

    #[test]
    fn config_parse_leaflet_multiple() {
        let toml = r#"
[[sources.leaflet]]
enabled = true
id = "desertthunder"
base_url = "https://desertthunder.leaflet.pub"

[[sources.leaflet]]
enabled = true
id = "stormlightlabs"
base_url = "https://stormlightlabs.leaflet.pub"
"#;
        let config = Config::from_str(toml).unwrap();
        assert_eq!(config.sources.leaflet.len(), 2);
        assert_eq!(config.sources.leaflet[0].id, "desertthunder");
        assert_eq!(config.sources.leaflet[1].id, "stormlightlabs");
    }

    #[test]
    fn config_parse_all_sources() {
        let toml = r#"
[database]
path = "/tmp/test.db"

[deployment]
mode = "sqlite"

[sources.substack]
enabled = true
base_url = "https://test.substack.com"

[sources.bluesky]
enabled = false
handle = "test.bsky.social"

[[sources.leaflet]]
enabled = true
id = "test"
base_url = "https://test.leaflet.pub"
"#;
        let config = Config::from_str(toml).unwrap();
        assert_eq!(config.database.path, Some("/tmp/test.db".to_string()));
        assert_eq!(config.deployment.mode, "sqlite");
        assert!(config.sources.substack.is_some());
        assert!(config.sources.bluesky.is_some());
        assert_eq!(config.sources.leaflet.len(), 1);
    }

    #[test]
    fn config_parse_invalid_toml() {
        let toml = "this is not valid toml {{{";
        assert!(Config::from_str(toml).is_err());
    }

    #[test]
    fn config_parse_missing_required_field() {
        let toml = r#"
[sources.substack]
enabled = true
"#;
        let result = Config::from_str(toml);
        assert!(result.is_err());
    }

    #[test]
    fn config_default_enabled_false() {
        let toml = r#"
[sources.substack]
base_url = "https://test.substack.com"
"#;
        let config = Config::from_str(toml).unwrap();
        let substack = config.sources.substack.as_ref().unwrap();
        assert!(!substack.enabled);
    }

    #[test]
    fn cors_config_exact_match() {
        let cors = CorsConfig {
            allowed_origins: vec![
                "https://desertthunder.dev".to_string(),
                "http://localhost:4321".to_string(),
            ],
            dev_key: None,
        };
        assert!(cors.is_origin_allowed("https://desertthunder.dev"));
        assert!(cors.is_origin_allowed("http://localhost:4321"));
        assert!(!cors.is_origin_allowed("https://evil.com"));
    }

    #[test]
    fn cors_config_same_root_domain() {
        let cors = CorsConfig { allowed_origins: vec!["https://desertthunder.dev".to_string()], dev_key: None };
        assert!(cors.is_origin_allowed("https://pai.desertthunder.dev"));
        assert!(cors.is_origin_allowed("https://api.desertthunder.dev"));
        assert!(cors.is_origin_allowed("https://desertthunder.dev"));
        assert!(!cors.is_origin_allowed("https://evil.dev"));
    }

    #[test]
    fn cors_config_localhost_requires_exact_match() {
        let cors = CorsConfig { allowed_origins: vec!["http://localhost:4321".to_string()], dev_key: None };
        assert!(cors.is_origin_allowed("http://localhost:4321"));
        assert!(!cors.is_origin_allowed("http://localhost:3000"));
    }

    #[test]
    fn cors_config_empty_origins_denies_all() {
        let cors = CorsConfig { allowed_origins: vec![], dev_key: None };
        assert!(!cors.is_origin_allowed("https://desertthunder.dev"));
        assert!(!cors.is_origin_allowed("http://localhost:4321"));
    }

    #[test]
    fn cors_config_dev_key_valid() {
        let cors = CorsConfig { allowed_origins: vec![], dev_key: Some("secret-dev-key".to_string()) };
        assert!(cors.is_dev_key_valid(Some("secret-dev-key")));
        assert!(!cors.is_dev_key_valid(Some("wrong-key")));
        assert!(!cors.is_dev_key_valid(None));
    }

    #[test]
    fn cors_config_dev_key_none() {
        let cors = CorsConfig { allowed_origins: vec![], dev_key: None };
        assert!(!cors.is_dev_key_valid(Some("any-key")));
        assert!(!cors.is_dev_key_valid(None));
    }

    #[test]
    fn extract_domain_https() {
        assert_eq!(
            super::extract_domain("https://desertthunder.dev/path"),
            "desertthunder.dev"
        );
        assert_eq!(
            super::extract_domain("https://pai.desertthunder.dev"),
            "pai.desertthunder.dev"
        );
    }

    #[test]
    fn extract_domain_http() {
        assert_eq!(super::extract_domain("http://localhost:4321/api"), "localhost");
        assert_eq!(super::extract_domain("http://example.com"), "example.com");
    }

    #[test]
    fn extract_root_domain_multi_level() {
        assert_eq!(
            super::extract_root_domain("pai.desertthunder.dev"),
            Some("desertthunder.dev".to_string())
        );
        assert_eq!(
            super::extract_root_domain("api.example.com"),
            Some("example.com".to_string())
        );
        assert_eq!(
            super::extract_root_domain("a.b.c.example.org"),
            Some("example.org".to_string())
        );
    }

    #[test]
    fn extract_root_domain_single_part() {
        assert_eq!(super::extract_root_domain("localhost"), None);
    }

    #[test]
    fn extract_root_domain_two_parts() {
        assert_eq!(
            super::extract_root_domain("example.com"),
            Some("example.com".to_string())
        );
    }

    #[test]
    fn config_parse_cors() {
        let toml = r#"
[cors]
allowed_origins = ["https://desertthunder.dev", "http://localhost:4321"]
dev_key = "my-dev-key"
"#;
        let config = Config::from_str(toml).unwrap();
        assert_eq!(config.cors.allowed_origins.len(), 2);
        assert_eq!(config.cors.allowed_origins[0], "https://desertthunder.dev");
        assert_eq!(config.cors.dev_key, Some("my-dev-key".to_string()));
    }
}
