use crate::{BearBlogConfig, Item, PaiError, Result, SourceFetcher, SourceKind, Storage};
use chrono::Utc;
use feed_rs::parser;

/// Fetcher for BearBlog publications via RSS
///
/// Retrieves posts from BearBlog blogs by parsing their RSS feeds.
/// Each BearBlog provides an RSS feed at {slug}.bearblog.dev/feed/.
pub struct BearBlogFetcher {
    config: BearBlogConfig,
    client: reqwest::Client,
}

impl BearBlogFetcher {
    /// Creates a new BearBlog fetcher with the given configuration
    pub fn new(config: BearBlogConfig) -> Self {
        Self { config, client: reqwest::Client::new() }
    }

    /// Fetches and parses the RSS feed
    async fn fetch_feed(&self) -> Result<feed_rs::model::Feed> {
        let feed_url = format!("{}/feed/", self.config.base_url.trim_end_matches('/'));
        let response = self
            .client
            .get(&feed_url)
            .send()
            .await
            .map_err(|e| PaiError::Fetch(format!("Failed to fetch BearBlog RSS feed: {e}")))?;

        let body = response
            .text()
            .await
            .map_err(|e| PaiError::Fetch(format!("Failed to read response body: {e}")))?;

        parser::parse(body.as_bytes()).map_err(|e| PaiError::Parse(format!("Failed to parse RSS feed: {e}")))
    }
}

impl SourceFetcher for BearBlogFetcher {
    fn sync(&self, storage: &dyn Storage) -> Result<()> {
        let runtime =
            tokio::runtime::Runtime::new().map_err(|e| PaiError::Fetch(format!("Failed to create runtime: {e}")))?;

        runtime.block_on(async {
            let feed = self.fetch_feed().await?;

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
                    source_kind: SourceKind::BearBlog,
                    source_id: self.config.id.clone(),
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

    #[test]
    fn parse_valid_rss() {
        let rss = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
<channel>
    <title>Test BearBlog</title>
    <link>https://test.bearblog.dev</link>
    <description>Test blog</description>
    <item>
        <title>Test Post</title>
        <link>https://test.bearblog.dev/test-post</link>
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
    <title>Empty Feed</title>
</channel>
</rss>"#;

        let feed = parser::parse(rss.as_bytes()).unwrap();
        assert_eq!(feed.entries.len(), 0);
    }
}
