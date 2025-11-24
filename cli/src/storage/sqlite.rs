use pai_core::{Item, ListFilter, PaiError, Result, SourceKind, Storage};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

const SCHEMA_VERSION: i32 = 1;

const INIT_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER PRIMARY KEY
);

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

CREATE INDEX IF NOT EXISTS idx_items_source_date
    ON items (source_kind, source_id, published_at DESC);
"#;

/// SQLite implementation of the Storage trait
///
/// Manages persistent storage of items in a local SQLite database.
/// Handles schema initialization and migrations automatically on first connection.
pub struct SqliteStorage {
    conn: Connection,
}

impl SqliteStorage {
    /// Opens or creates a SQLite database at the given path
    ///
    /// Initializes the schema if the database is new or runs migrations if needed.
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_ref = path.as_ref();

        if let Some(parent) = path_ref.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| PaiError::Storage(format!("Failed to create database directory: {e}")))?;
        }

        let conn = Connection::open(path).map_err(|e| PaiError::Storage(format!("Failed to open database: {e}")))?;

        let mut storage = Self { conn };
        storage.init_schema()?;
        Ok(storage)
    }

    /// Initializes the database schema
    ///
    /// Creates tables and indexes if they don't exist, and sets up version tracking.
    fn init_schema(&mut self) -> Result<()> {
        self.conn
            .execute_batch(INIT_SQL)
            .map_err(|e| PaiError::Storage(format!("Failed to initialize schema: {e}")))?;

        let version: Option<i32> = self
            .conn
            .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| row.get(0))
            .optional()
            .map_err(|e| PaiError::Storage(format!("Failed to check schema version: {e}")))?;

        match version {
            None => {
                self.conn
                    .execute(
                        "INSERT INTO schema_version (version) VALUES (?1)",
                        params![SCHEMA_VERSION],
                    )
                    .map_err(|e| PaiError::Storage(format!("Failed to set schema version: {e}")))?;
            }
            Some(v) if v < SCHEMA_VERSION => {
                return Err(PaiError::Storage(format!(
                    "Database migration needed: current={v}, required={SCHEMA_VERSION}"
                )));
            }
            _ => {}
        }

        Ok(())
    }

    /// Gets basic statistics about stored items
    pub fn get_stats(&self) -> Result<Vec<(String, usize)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT source_kind, COUNT(*) FROM items GROUP BY source_kind ORDER BY source_kind")
            .map_err(|e| PaiError::Storage(format!("Failed to prepare stats query: {e}")))?;

        let stats = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?)))
            .map_err(|e| PaiError::Storage(format!("Failed to query stats: {e}")))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| PaiError::Storage(format!("Failed to collect stats: {e}")))?;

        Ok(stats)
    }

    /// Gets total item count
    pub fn count_items(&self) -> Result<usize> {
        self.conn
            .query_row("SELECT COUNT(*) FROM items", [], |row| row.get(0))
            .map_err(|e| PaiError::Storage(format!("Failed to count items: {e}")))
    }

    /// Verifies schema integrity
    ///
    /// Checks that required tables and indexes exist.
    pub fn verify_schema(&self) -> Result<()> {
        let tables = vec!["schema_version", "items"];
        for table in tables {
            let exists: bool = self
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    params![table],
                    |row| {
                        let count: i32 = row.get(0)?;
                        Ok(count > 0)
                    },
                )
                .map_err(|e| PaiError::Storage(format!("Failed to verify table {table}: {e}")))?;

            if !exists {
                return Err(PaiError::Storage(format!("Missing table: {table}")));
            }
        }

        Ok(())
    }

    /// Fetches a single item by ID, if it exists
    pub fn get_item(&self, id: &str) -> Result<Option<Item>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, source_kind, source_id, author, title, summary, url, content_html, published_at, created_at
             FROM items WHERE id = ?1 LIMIT 1",
            )
            .map_err(|e| PaiError::Storage(format!("Failed to prepare get_item query: {e}")))?;

        stmt.query_row([id], |row| {
            let source_kind_str: String = row.get(1)?;
            let source_kind = source_kind_str
                .parse::<SourceKind>()
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e)))?;

            Ok(Item {
                id: row.get(0)?,
                source_kind,
                source_id: row.get(2)?,
                author: row.get(3)?,
                title: row.get(4)?,
                summary: row.get(5)?,
                url: row.get(6)?,
                content_html: row.get(7)?,
                published_at: row.get(8)?,
                created_at: row.get(9)?,
            })
        })
        .optional()
        .map_err(|e| PaiError::Storage(format!("Failed to fetch item by id: {e}")))
    }
}

impl Storage for SqliteStorage {
    fn insert_or_replace_item(&self, item: &Item) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO items
             (id, source_kind, source_id, author, title, summary, url, content_html, published_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    item.id,
                    item.source_kind.to_string(),
                    item.source_id,
                    item.author,
                    item.title,
                    item.summary,
                    item.url,
                    item.content_html,
                    item.published_at,
                    item.created_at,
                ],
            )
            .map_err(|e| PaiError::Storage(format!("Failed to insert item: {e}")))?;

        Ok(())
    }

    fn list_items(&self, filter: &ListFilter) -> Result<Vec<Item>> {
        let mut sql = String::from("SELECT id, source_kind, source_id, author, title, summary, url, content_html, published_at, created_at FROM items WHERE 1=1");
        let mut conditions = Vec::new();

        if filter.source_kind.is_some() {
            sql.push_str(" AND source_kind = ?");
            conditions.push(filter.source_kind.unwrap().to_string());
        }

        if let Some(ref source_id) = filter.source_id {
            sql.push_str(" AND source_id = ?");
            conditions.push(source_id.clone());
        }

        if let Some(ref since) = filter.since {
            sql.push_str(" AND published_at >= ?");
            conditions.push(since.clone());
        }

        if let Some(ref query) = filter.query {
            sql.push_str(" AND (title LIKE ? OR summary LIKE ?)");
            let pattern = format!("%{query}%");
            conditions.push(pattern.clone());
            conditions.push(pattern);
        }

        sql.push_str(" ORDER BY published_at DESC");

        if let Some(limit) = filter.limit {
            sql.push_str(&format!(" LIMIT {limit}"));
        }

        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|e| PaiError::Storage(format!("Failed to prepare query: {e}")))?;

        let params_refs: Vec<&dyn rusqlite::ToSql> = conditions.iter().map(|s| s as &dyn rusqlite::ToSql).collect();

        let items = stmt
            .query_map(params_refs.as_slice(), |row| {
                let source_kind_str: String = row.get(1)?;
                let source_kind = source_kind_str.parse::<SourceKind>().map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e))
                })?;

                Ok(Item {
                    id: row.get(0)?,
                    source_kind,
                    source_id: row.get(2)?,
                    author: row.get(3)?,
                    title: row.get(4)?,
                    summary: row.get(5)?,
                    url: row.get(6)?,
                    content_html: row.get(7)?,
                    published_at: row.get(8)?,
                    created_at: row.get(9)?,
                })
            })
            .map_err(|e| PaiError::Storage(format!("Failed to query items: {e}")))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| PaiError::Storage(format!("Failed to collect items: {e}")))?;

        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn create_test_storage() -> SqliteStorage {
        SqliteStorage::new(":memory:").expect("Failed to create in-memory database")
    }

    fn create_test_item(id: &str, source_kind: SourceKind, source_id: &str) -> Item {
        Item {
            id: id.to_string(),
            source_kind,
            source_id: source_id.to_string(),
            author: Some("Test Author".to_string()),
            title: Some("Test Title".to_string()),
            summary: Some("Test summary".to_string()),
            url: format!("https://example.com/{id}"),
            content_html: Some("<p>Test content</p>".to_string()),
            published_at: Utc::now().to_rfc3339(),
            created_at: Utc::now().to_rfc3339(),
        }
    }

    #[test]
    fn new_database_initializes_schema() {
        let storage = create_test_storage();
        assert!(storage.verify_schema().is_ok());
    }

    #[test]
    fn insert_and_retrieve_item() {
        let storage = create_test_storage();
        let item = create_test_item("test-1", SourceKind::Substack, "test.substack.com");

        storage.insert_or_replace_item(&item).expect("Failed to insert item");

        let filter = ListFilter::default();
        let items = storage.list_items(&filter).expect("Failed to list items");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "test-1");
        assert_eq!(items[0].source_kind, SourceKind::Substack);
    }

    #[test]
    fn insert_replaces_existing_item() {
        let storage = create_test_storage();
        let mut item = create_test_item("test-1", SourceKind::Substack, "test.substack.com");

        storage.insert_or_replace_item(&item).expect("Failed to insert item");

        item.title = Some("Updated Title".to_string());
        storage.insert_or_replace_item(&item).expect("Failed to replace item");

        let filter = ListFilter::default();
        let items = storage.list_items(&filter).expect("Failed to list items");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, Some("Updated Title".to_string()));
    }

    #[test]
    fn filter_by_source_kind() {
        let storage = create_test_storage();

        storage
            .insert_or_replace_item(&create_test_item("test-1", SourceKind::Substack, "test.substack.com"))
            .expect("Failed to insert");
        storage
            .insert_or_replace_item(&create_test_item("test-2", SourceKind::Bluesky, "test.bsky.social"))
            .expect("Failed to insert");

        let filter = ListFilter { source_kind: Some(SourceKind::Substack), ..Default::default() };
        let items = storage.list_items(&filter).expect("Failed to list items");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].source_kind, SourceKind::Substack);
    }

    #[test]
    fn filter_by_source_id() {
        let storage = create_test_storage();

        storage
            .insert_or_replace_item(&create_test_item("test-1", SourceKind::Leaflet, "source1.leaflet.pub"))
            .expect("Failed to insert");
        storage
            .insert_or_replace_item(&create_test_item("test-2", SourceKind::Leaflet, "source2.leaflet.pub"))
            .expect("Failed to insert");

        let filter = ListFilter { source_id: Some("source1.leaflet.pub".to_string()), ..Default::default() };
        let items = storage.list_items(&filter).expect("Failed to list items");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].source_id, "source1.leaflet.pub");
    }

    #[test]
    fn filter_with_limit() {
        let storage = create_test_storage();

        for i in 0..5 {
            storage
                .insert_or_replace_item(&create_test_item(
                    &format!("test-{i}"),
                    SourceKind::Substack,
                    "test.substack.com",
                ))
                .expect("Failed to insert");
        }

        let filter = ListFilter { limit: Some(3), ..Default::default() };
        let items = storage.list_items(&filter).expect("Failed to list items");

        assert_eq!(items.len(), 3);
    }

    #[test]
    fn filter_by_query() {
        let storage = create_test_storage();

        let mut item1 = create_test_item("test-1", SourceKind::Substack, "test.substack.com");
        item1.title = Some("Rust Programming".to_string());
        storage.insert_or_replace_item(&item1).expect("Failed to insert");

        let mut item2 = create_test_item("test-2", SourceKind::Substack, "test.substack.com");
        item2.title = Some("Python Tutorial".to_string());
        storage.insert_or_replace_item(&item2).expect("Failed to insert");

        let filter = ListFilter { query: Some("Rust".to_string()), ..Default::default() };
        let items = storage.list_items(&filter).expect("Failed to list items");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "test-1");
    }

    #[test]
    fn get_stats_returns_counts_by_source() {
        let storage = create_test_storage();

        storage
            .insert_or_replace_item(&create_test_item("test-1", SourceKind::Substack, "test.substack.com"))
            .expect("Failed to insert");
        storage
            .insert_or_replace_item(&create_test_item("test-2", SourceKind::Substack, "test.substack.com"))
            .expect("Failed to insert");
        storage
            .insert_or_replace_item(&create_test_item("test-3", SourceKind::Bluesky, "test.bsky.social"))
            .expect("Failed to insert");

        let stats = storage.get_stats().expect("Failed to get stats");

        assert_eq!(stats.len(), 2);
        assert!(stats.iter().any(|(k, v)| k == "bluesky" && *v == 1));
        assert!(stats.iter().any(|(k, v)| k == "substack" && *v == 2));
    }

    #[test]
    fn count_items_returns_total() {
        let storage = create_test_storage();

        for i in 0..3 {
            storage
                .insert_or_replace_item(&create_test_item(
                    &format!("test-{i}"),
                    SourceKind::Substack,
                    "test.substack.com",
                ))
                .expect("Failed to insert");
        }

        let count = storage.count_items().expect("Failed to count items");
        assert_eq!(count, 3);
    }

    #[test]
    fn get_item_returns_record() {
        let storage = create_test_storage();
        let item = create_test_item("test-1", SourceKind::Substack, "test.substack.com");
        storage.insert_or_replace_item(&item).expect("Failed to insert");

        let fetched = storage.get_item("test-1").expect("query failed").unwrap();
        assert_eq!(fetched.id, "test-1");
        assert_eq!(fetched.source_kind, SourceKind::Substack);
    }

    #[test]
    fn get_item_returns_none_for_missing() {
        let storage = create_test_storage();
        let result = storage.get_item("nope").expect("query failed");
        assert!(result.is_none());
    }
}
