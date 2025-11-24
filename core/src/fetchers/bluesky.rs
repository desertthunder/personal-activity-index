use crate::{BlueskyConfig, Item, PaiError, Result, SourceFetcher, SourceKind, Storage};
use chrono::Utc;
use serde::Deserialize;

const BLUESKY_API_BASE: &str = "https://public.api.bsky.app";

/// Response from app.bsky.feed.getAuthorFeed
#[derive(Debug, Deserialize)]
struct AuthorFeedResponse {
    feed: Vec<FeedViewPost>,
    #[allow(dead_code)]
    cursor: Option<String>,
}

/// A post in the author feed
#[derive(Debug, Deserialize)]
struct FeedViewPost {
    post: PostView,
    #[allow(dead_code)]
    reason: Option<serde_json::Value>,
}

/// Post view with metadata
#[derive(Debug, Deserialize)]
struct PostView {
    uri: String,
    #[allow(dead_code)]
    cid: String,
    author: Author,
    record: serde_json::Value,
    #[allow(dead_code)]
    #[serde(rename = "indexedAt")]
    indexed_at: String,
}

/// Author information
#[derive(Debug, Deserialize)]
struct Author {
    #[allow(dead_code)]
    did: String,
    handle: String,
    #[allow(dead_code)]
    #[serde(rename = "displayName")]
    display_name: Option<String>,
}

/// Fetcher for Bluesky posts via AT Protocol
///
/// Retrieves posts from a Bluesky user by querying the public API.
/// Filters out reposts and quotes to only include original posts.
pub struct BlueskyFetcher {
    config: BlueskyConfig,
    client: reqwest::Client,
}

impl BlueskyFetcher {
    /// Creates a new Bluesky fetcher with the given configuration
    pub fn new(config: BlueskyConfig) -> Self {
        Self { config, client: reqwest::Client::new() }
    }

    /// Fetches the author feed from the Bluesky public API
    async fn fetch_author_feed(&self) -> Result<AuthorFeedResponse> {
        let url = format!("{BLUESKY_API_BASE}/xrpc/app.bsky.feed.getAuthorFeed");

        let response = self
            .client
            .get(&url)
            .query(&[("actor", &self.config.handle), ("limit", &"50".to_string())])
            .send()
            .await
            .map_err(|e| PaiError::Fetch(format!("Failed to fetch Bluesky feed: {e}")))?;

        if !response.status().is_success() {
            return Err(PaiError::Fetch(format!("Bluesky API error: {}", response.status())));
        }

        response
            .json::<AuthorFeedResponse>()
            .await
            .map_err(|e| PaiError::Parse(format!("Failed to parse Bluesky response: {e}")))
    }

    /// Checks if a post is an original post (not a repost or quote)
    fn is_original_post(feed_post: &FeedViewPost) -> bool {
        feed_post.reason.is_none()
    }

    /// Converts an AT URI to a canonical Bluesky URL
    ///
    /// AT URI format: at://did:plc:xyz/app.bsky.feed.post/abc123
    /// URL format: https://bsky.app/profile/{handle}/post/{post_id}
    fn at_uri_to_url(uri: &str, handle: &str) -> Result<String> {
        let parts: Vec<&str> = uri.split('/').collect();
        if parts.len() >= 4 && parts[0] == "at:" {
            let post_id = parts[parts.len() - 1];
            Ok(format!("https://bsky.app/profile/{handle}/post/{post_id}"))
        } else {
            Err(PaiError::Parse(format!("Invalid AT URI: {uri}")))
        }
    }

    /// Extracts text content from the post record
    fn extract_text(record: &serde_json::Value) -> Option<String> {
        record.get("text").and_then(|v| v.as_str()).map(String::from)
    }

    /// Creates a title from the post text (truncated to 100 chars)
    fn create_title(text: &str) -> String {
        if text.len() <= 100 {
            text.to_string()
        } else {
            format!("{}...", &text[..97])
        }
    }
}

impl SourceFetcher for BlueskyFetcher {
    fn sync(&self, storage: &dyn Storage) -> Result<()> {
        let runtime =
            tokio::runtime::Runtime::new().map_err(|e| PaiError::Fetch(format!("Failed to create runtime: {e}")))?;

        runtime.block_on(async {
            let response = self.fetch_author_feed().await?;

            for feed_post in response.feed {
                if !Self::is_original_post(&feed_post) {
                    continue;
                }

                let post = feed_post.post;
                let text = Self::extract_text(&post.record);

                let title = text.as_ref().map(|t| Self::create_title(t));
                let url = Self::at_uri_to_url(&post.uri, &post.author.handle)?;

                let published_at = post
                    .record
                    .get("createdAt")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .unwrap_or_else(|| Utc::now().to_rfc3339());

                let item = Item {
                    id: post.uri.clone(),
                    source_kind: SourceKind::Bluesky,
                    source_id: self.config.handle.clone(),
                    author: Some(post.author.handle.clone()),
                    title,
                    summary: text,
                    url,
                    content_html: None,
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
    fn at_uri_to_url_valid() {
        let uri = "at://did:plc:abc123/app.bsky.feed.post/xyz789";
        let url = BlueskyFetcher::at_uri_to_url(uri, "user.bsky.social").unwrap();
        assert_eq!(url, "https://bsky.app/profile/user.bsky.social/post/xyz789");
    }

    #[test]
    fn at_uri_to_url_invalid() {
        let uri = "invalid-uri";
        assert!(BlueskyFetcher::at_uri_to_url(uri, "user.bsky.social").is_err());
    }

    #[test]
    fn create_title_short_text() {
        let text = "Short post";
        assert_eq!(BlueskyFetcher::create_title(text), "Short post");
    }

    #[test]
    fn create_title_long_text() {
        let text = "This is a very long post that exceeds one hundred characters and should be truncated with ellipsis at the end";
        let title = BlueskyFetcher::create_title(text);
        assert!(title.ends_with("..."));
        assert_eq!(title.len(), 100);
    }

    #[test]
    fn extract_text_from_record() {
        let record = serde_json::json!({
            "text": "Hello world",
            "createdAt": "2024-01-01T12:00:00Z"
        });
        let text = BlueskyFetcher::extract_text(&record).unwrap();
        assert_eq!(text, "Hello world");
    }

    #[test]
    fn extract_text_missing() {
        let record = serde_json::json!({
            "createdAt": "2024-01-01T12:00:00Z"
        });
        assert!(BlueskyFetcher::extract_text(&record).is_none());
    }

    #[test]
    fn is_original_post_true() {
        let feed_post = FeedViewPost {
            post: PostView {
                uri: "at://test".to_string(),
                cid: "cid123".to_string(),
                author: Author {
                    did: "did:plc:test".to_string(),
                    handle: "test.bsky.social".to_string(),
                    display_name: None,
                },
                record: serde_json::json!({}),
                indexed_at: "2024-01-01T12:00:00Z".to_string(),
            },
            reason: None,
        };
        assert!(BlueskyFetcher::is_original_post(&feed_post));
    }

    #[test]
    fn is_original_post_false_repost() {
        let feed_post = FeedViewPost {
            post: PostView {
                uri: "at://test".to_string(),
                cid: "cid123".to_string(),
                author: Author {
                    did: "did:plc:test".to_string(),
                    handle: "test.bsky.social".to_string(),
                    display_name: None,
                },
                record: serde_json::json!({}),
                indexed_at: "2024-01-01T12:00:00Z".to_string(),
            },
            reason: Some(serde_json::json!({"$type": "app.bsky.feed.defs#reasonRepost"})),
        };
        assert!(!BlueskyFetcher::is_original_post(&feed_post));
    }
}
