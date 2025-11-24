use crate::{Item, PaiError, Result, SourceFetcher, SourceKind, Storage, SubstackConfig};
use chrono::Utc;
use feed_rs::parser;
use tokio::runtime::Runtime;

/// Fetcher for Substack RSS feeds
///
/// Retrieves posts from a Substack publication by parsing its RSS feed.
/// Maps RSS items to the standardized Item struct for storage.
pub struct SubstackFetcher {
    config: SubstackConfig,
    client: reqwest::Client,
}

impl SubstackFetcher {
    /// Creates a new Substack fetcher with the given configuration
    pub fn new(config: SubstackConfig) -> Self {
        Self { config, client: reqwest::Client::new() }
    }

    /// Fetches and parses the RSS feed
    async fn fetch_feed(&self) -> Result<feed_rs::model::Feed> {
        let feed_url = format!("{}/feed", self.config.base_url);
        let response = self
            .client
            .get(&feed_url)
            .send()
            .await
            .map_err(|e| PaiError::Fetch(format!("Failed to fetch RSS feed: {e}")))?;

        let body = response
            .text()
            .await
            .map_err(|e| PaiError::Fetch(format!("Failed to read response body: {e}")))?;

        parser::parse(body.as_bytes()).map_err(|e| PaiError::Parse(format!("Failed to parse RSS feed: {e}")))
    }

    /// Extracts the source ID from the base URL (e.g., "patternmatched.substack.com")
    fn extract_source_id(&self) -> String {
        Self::normalize_source_id(&self.config.base_url)
    }

    pub(crate) fn normalize_source_id(base_url: &str) -> String {
        base_url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_string()
    }
}

impl SourceFetcher for SubstackFetcher {
    fn sync(&self, storage: &dyn Storage) -> Result<()> {
        let runtime = Runtime::new().map_err(|e| PaiError::Fetch(format!("Failed to create runtime: {e}")))?;

        runtime.block_on(async {
            let feed = self.fetch_feed().await?;
            let source_id = self.extract_source_id();

            for entry in feed.entries {
                let id = entry.id.clone();
                let url = entry
                    .links
                    .first()
                    .map(|link| link.href.clone())
                    .unwrap_or_else(|| id.clone());

                let title = entry.title.as_ref().map(|t| t.content.clone());
                let summary = entry.summary.as_ref().map(|s| s.content.clone());
                let author = entry.authors.first().map(|a| a.name.clone());
                let content_html = entry.content.and_then(|c| c.body);

                let published_at = entry
                    .published
                    .or(entry.updated)
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_else(|| Utc::now().to_rfc3339());

                let item = Item {
                    id,
                    source_kind: SourceKind::Substack,
                    source_id: source_id.clone(),
                    author,
                    title,
                    summary,
                    url,
                    content_html,
                    published_at,
                    created_at: Utc::now().to_rfc3339(),
                };

                storage.insert_or_replace_item(&item)?;
            }

            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ListFilter;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    #[allow(dead_code)]
    struct MockStorage {
        items: Arc<Mutex<Vec<Item>>>,
    }

    #[allow(dead_code)]
    impl MockStorage {
        fn new() -> Self {
            Self { items: Arc::new(Mutex::new(Vec::new())) }
        }

        fn get_items(&self) -> Vec<Item> {
            self.items.lock().unwrap().clone()
        }
    }

    impl Storage for MockStorage {
        fn insert_or_replace_item(&self, item: &Item) -> Result<()> {
            self.items.lock().unwrap().push(item.clone());
            Ok(())
        }

        fn list_items(&self, _filter: &ListFilter) -> Result<Vec<Item>> {
            Ok(self.items.lock().unwrap().clone())
        }
    }

    #[test]
    fn extract_source_id_https() {
        assert_eq!(
            SubstackFetcher::normalize_source_id("https://patternmatched.substack.com"),
            "patternmatched.substack.com"
        );
    }

    #[test]
    fn extract_source_id_http() {
        assert_eq!(
            SubstackFetcher::normalize_source_id("http://test.substack.com/"),
            "test.substack.com"
        );
    }

    #[test]
    fn parse_valid_rss() {
        let rss = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
<channel>
    <title>Test Feed</title>
    <link>https://test.substack.com</link>
    <description>Test</description>
    <item>
        <title>Test Post</title>
        <link>https://test.substack.com/p/test-post</link>
        <guid>test-guid</guid>
        <pubDate>Mon, 01 Jan 2024 12:00:00 +0000</pubDate>
        <description>Test summary</description>
    </item>
</channel>
</rss>"#;

        let feed = parser::parse(rss.as_bytes()).unwrap();
        assert_eq!(feed.entries.len(), 1);
        assert_eq!(feed.entries[0].title.as_ref().unwrap().content, "Test Post");
    }

    #[test]
    fn parse_invalid_rss() {
        let invalid_rss = "this is not valid XML";
        let result = parser::parse(invalid_rss.as_bytes());
        assert!(result.is_err());
    }

    #[test]
    fn parse_empty_rss() {
        let rss = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
<channel>
    <title>Test Feed</title>
</channel>
</rss>"#;

        let feed = parser::parse(rss.as_bytes()).unwrap();
        assert_eq!(feed.entries.len(), 0);
    }
}
