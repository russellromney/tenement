//! SQLite storage for logs and config
//!
//! Persists logs with FTS5 full-text search and handles config storage.

use crate::logs::{LogEntry, LogLevel, LogQuery};
use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Pool, Row, Sqlite};
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{error, info};

/// SQLite connection pool
pub type DbPool = Pool<Sqlite>;

/// Initialize the database with required tables
pub async fn init_db(path: &Path) -> Result<DbPool> {
    // Create parent directories if needed
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let options = SqliteConnectOptions::from_str(&format!("sqlite:{}?mode=rwc", path.display()))?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .context("Failed to connect to SQLite database")?;

    // Create tables
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp TEXT NOT NULL,
            level TEXT NOT NULL,
            process TEXT NOT NULL,
            instance_id TEXT NOT NULL,
            message TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_logs_process ON logs(process);
        CREATE INDEX IF NOT EXISTS idx_logs_instance ON logs(instance_id);
        CREATE INDEX IF NOT EXISTS idx_logs_timestamp ON logs(timestamp DESC);
        "#,
    )
    .execute(&pool)
    .await
    .context("Failed to create logs table")?;

    // Create FTS5 virtual table for full-text search
    sqlx::query(
        r#"
        CREATE VIRTUAL TABLE IF NOT EXISTS logs_fts USING fts5(
            message,
            content='logs',
            content_rowid='id'
        );
        "#,
    )
    .execute(&pool)
    .await
    .context("Failed to create FTS5 table")?;

    // Create triggers to keep FTS in sync
    sqlx::query(
        r#"
        CREATE TRIGGER IF NOT EXISTS logs_ai AFTER INSERT ON logs BEGIN
            INSERT INTO logs_fts(rowid, message) VALUES (new.id, new.message);
        END;
        "#,
    )
    .execute(&pool)
    .await
    .context("Failed to create FTS insert trigger")?;

    sqlx::query(
        r#"
        CREATE TRIGGER IF NOT EXISTS logs_ad AFTER DELETE ON logs BEGIN
            INSERT INTO logs_fts(logs_fts, rowid, message) VALUES('delete', old.id, old.message);
        END;
        "#,
    )
    .execute(&pool)
    .await
    .context("Failed to create FTS delete trigger")?;

    // Create config table
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS config (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        "#,
    )
    .execute(&pool)
    .await
    .context("Failed to create config table")?;

    info!("Database initialized at {:?}", path);
    Ok(pool)
}

/// Log store with batch flushing
pub struct LogStore {
    pool: DbPool,
    tx: mpsc::Sender<LogEntry>,
}

impl LogStore {
    /// Create a new log store with batch flushing
    pub fn new(pool: DbPool) -> Arc<Self> {
        let (tx, rx) = mpsc::channel::<LogEntry>(10000);
        let store = Arc::new(Self { pool: pool.clone(), tx });

        // Spawn background batch flusher
        tokio::spawn(batch_flusher(pool, rx));

        store
    }

    /// Push a log entry (batched for efficiency)
    pub async fn push(&self, entry: LogEntry) {
        if let Err(e) = self.tx.send(entry).await {
            error!("Failed to queue log entry: {}", e);
        }
    }

    /// Query logs with filters
    pub async fn query(&self, query: &LogQuery) -> Result<Vec<LogEntry>> {
        let limit = query.limit.unwrap_or(100);

        // If search is provided, use FTS5
        if let Some(ref search) = query.search {
            return self.query_fts(query, search, limit).await;
        }

        // Build dynamic query
        let mut sql = String::from(
            "SELECT id, timestamp, level, process, instance_id, message FROM logs WHERE 1=1",
        );
        let mut params: Vec<String> = Vec::new();

        if let Some(ref process) = query.process {
            sql.push_str(" AND process = ?");
            params.push(process.clone());
        }

        if let Some(ref id) = query.instance_id {
            sql.push_str(" AND instance_id = ?");
            params.push(id.clone());
        }

        if let Some(level) = query.level {
            sql.push_str(" AND level = ?");
            params.push(level.to_string());
        }

        sql.push_str(" ORDER BY timestamp DESC LIMIT ?");

        // Execute query with dynamic binding
        let rows = match params.len() {
            0 => {
                sqlx::query(&sql)
                    .bind(limit as i64)
                    .fetch_all(&self.pool)
                    .await?
            }
            1 => {
                sqlx::query(&sql)
                    .bind(&params[0])
                    .bind(limit as i64)
                    .fetch_all(&self.pool)
                    .await?
            }
            2 => {
                sqlx::query(&sql)
                    .bind(&params[0])
                    .bind(&params[1])
                    .bind(limit as i64)
                    .fetch_all(&self.pool)
                    .await?
            }
            3 => {
                sqlx::query(&sql)
                    .bind(&params[0])
                    .bind(&params[1])
                    .bind(&params[2])
                    .bind(limit as i64)
                    .fetch_all(&self.pool)
                    .await?
            }
            _ => return Ok(Vec::new()),
        };

        Ok(rows
            .into_iter()
            .map(|row| {
                let timestamp_str: String = row.get("timestamp");
                LogEntry {
                    timestamp: iso8601_to_millis(&timestamp_str),
                    level: LogLevel::from_str(row.get::<&str, _>("level")),
                    process: row.get("process"),
                    instance_id: row.get("instance_id"),
                    message: row.get("message"),
                }
            })
            .collect())
    }

    /// Query using FTS5 full-text search
    async fn query_fts(&self, query: &LogQuery, search: &str, limit: usize) -> Result<Vec<LogEntry>> {
        let mut sql = String::from(
            r#"
            SELECT l.id, l.timestamp, l.level, l.process, l.instance_id, l.message
            FROM logs l
            JOIN logs_fts f ON l.id = f.rowid
            WHERE logs_fts MATCH ?
            "#,
        );
        let mut params: Vec<String> = vec![format!("\"{}\"", search.replace('"', "\"\""))];

        if let Some(ref process) = query.process {
            sql.push_str(" AND l.process = ?");
            params.push(process.clone());
        }

        if let Some(ref id) = query.instance_id {
            sql.push_str(" AND l.instance_id = ?");
            params.push(id.clone());
        }

        if let Some(level) = query.level {
            sql.push_str(" AND l.level = ?");
            params.push(level.to_string());
        }

        sql.push_str(" ORDER BY l.timestamp DESC LIMIT ?");

        let rows = match params.len() {
            1 => {
                sqlx::query(&sql)
                    .bind(&params[0])
                    .bind(limit as i64)
                    .fetch_all(&self.pool)
                    .await?
            }
            2 => {
                sqlx::query(&sql)
                    .bind(&params[0])
                    .bind(&params[1])
                    .bind(limit as i64)
                    .fetch_all(&self.pool)
                    .await?
            }
            3 => {
                sqlx::query(&sql)
                    .bind(&params[0])
                    .bind(&params[1])
                    .bind(&params[2])
                    .bind(limit as i64)
                    .fetch_all(&self.pool)
                    .await?
            }
            4 => {
                sqlx::query(&sql)
                    .bind(&params[0])
                    .bind(&params[1])
                    .bind(&params[2])
                    .bind(&params[3])
                    .bind(limit as i64)
                    .fetch_all(&self.pool)
                    .await?
            }
            _ => return Ok(Vec::new()),
        };

        Ok(rows
            .into_iter()
            .map(|row| {
                let timestamp_str: String = row.get("timestamp");
                LogEntry {
                    timestamp: iso8601_to_millis(&timestamp_str),
                    level: LogLevel::from_str(row.get::<&str, _>("level")),
                    process: row.get("process"),
                    instance_id: row.get("instance_id"),
                    message: row.get("message"),
                }
            })
            .collect())
    }

    /// Rotate logs - delete entries older than the given duration
    pub async fn rotate(&self, max_age: Duration) -> Result<u64> {
        let cutoff = chrono_cutoff(max_age);
        let result = sqlx::query("DELETE FROM logs WHERE timestamp < ?")
            .bind(&cutoff)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    /// Get total log count
    pub async fn count(&self) -> Result<i64> {
        let row = sqlx::query("SELECT COUNT(*) as count FROM logs")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.get("count"))
    }
}

/// Background task that batches log entries and flushes to SQLite
async fn batch_flusher(pool: DbPool, mut rx: mpsc::Receiver<LogEntry>) {
    let flush_interval = Duration::from_millis(250);
    let mut batch: Vec<LogEntry> = Vec::with_capacity(1000);

    loop {
        // Wait for entries or timeout
        let deadline = tokio::time::sleep(flush_interval);
        tokio::pin!(deadline);

        loop {
            tokio::select! {
                entry = rx.recv() => {
                    match entry {
                        Some(e) => {
                            batch.push(e);
                            if batch.len() >= 1000 {
                                break; // Flush when batch is full
                            }
                        }
                        None => {
                            // Channel closed, flush remaining and exit
                            if !batch.is_empty() {
                                if let Err(e) = flush_batch(&pool, &batch).await {
                                    error!("Failed to flush final batch: {}", e);
                                }
                            }
                            return;
                        }
                    }
                }
                _ = &mut deadline => {
                    break; // Flush on timeout
                }
            }
        }

        // Flush batch
        if !batch.is_empty() {
            if let Err(e) = flush_batch(&pool, &batch).await {
                error!("Failed to flush log batch: {}", e);
            }
            batch.clear();
        }
    }
}

/// Flush a batch of log entries to SQLite
async fn flush_batch(pool: &DbPool, entries: &[LogEntry]) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }

    let mut tx = pool.begin().await?;

    for entry in entries {
        // Convert millis timestamp to ISO8601 string
        let timestamp = millis_to_iso8601(entry.timestamp);
        sqlx::query(
            "INSERT INTO logs (timestamp, level, process, instance_id, message) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&timestamp)
        .bind(entry.level.to_string())
        .bind(&entry.process)
        .bind(&entry.instance_id)
        .bind(&entry.message)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

/// Convert milliseconds since epoch to ISO8601 timestamp string
fn millis_to_iso8601(millis: u64) -> String {
    use chrono::{DateTime, Utc};
    use std::time::{Duration, UNIX_EPOCH};

    let datetime = UNIX_EPOCH + Duration::from_millis(millis);
    let datetime: DateTime<Utc> = datetime.into();
    datetime.to_rfc3339()
}

/// Convert ISO8601 timestamp string back to milliseconds
fn iso8601_to_millis(s: &str) -> u64 {
    use chrono::DateTime;

    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.timestamp_millis() as u64)
        .unwrap_or(0)
}

/// Calculate cutoff timestamp for log rotation
fn chrono_cutoff(max_age: Duration) -> String {
    use std::time::SystemTime;
    let cutoff = SystemTime::now()
        .checked_sub(max_age)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let datetime: chrono::DateTime<chrono::Utc> = cutoff.into();
    datetime.to_rfc3339()
}

/// Config store for key-value settings
pub struct ConfigStore {
    pool: DbPool,
}

impl ConfigStore {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    /// Get a config value
    pub async fn get(&self, key: &str) -> Result<Option<String>> {
        let row = sqlx::query("SELECT value FROM config WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.get("value")))
    }

    /// Set a config value
    pub async fn set(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query("INSERT OR REPLACE INTO config (key, value) VALUES (?, ?)")
            .bind(key)
            .bind(value)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Delete a config value
    pub async fn delete(&self, key: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM config WHERE key = ?")
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}

// Helper to parse LogLevel from string
impl LogLevel {
    fn from_str(s: &str) -> Self {
        match s {
            "stdout" => LogLevel::Stdout,
            "stderr" => LogLevel::Stderr,
            _ => LogLevel::Stdout,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn create_test_db() -> (DbPool, TempDir) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");
        let pool = init_db(&path).await.unwrap();
        (pool, dir)
    }

    #[tokio::test]
    async fn test_init_db() {
        let (pool, _dir) = create_test_db().await;

        // Verify tables exist
        let result = sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name='logs'")
            .fetch_optional(&pool)
            .await
            .unwrap();
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn test_log_store_insert_and_query() {
        let (pool, _dir) = create_test_db().await;
        let store = LogStore::new(pool);

        // Insert entries
        let entry = LogEntry::new("api", "prod", LogLevel::Stdout, "hello world".to_string());
        store.push(entry).await;

        // Wait for batch flush
        tokio::time::sleep(Duration::from_millis(300)).await;

        // Query
        let query = LogQuery::default();
        let results = store.query(&query).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].message, "hello world");
    }

    #[tokio::test]
    async fn test_log_store_query_filter_process() {
        let (pool, _dir) = create_test_db().await;
        let store = LogStore::new(pool);

        store.push(LogEntry::new("api", "prod", LogLevel::Stdout, "api msg".to_string())).await;
        store.push(LogEntry::new("web", "prod", LogLevel::Stdout, "web msg".to_string())).await;

        tokio::time::sleep(Duration::from_millis(300)).await;

        let query = LogQuery {
            process: Some("api".to_string()),
            ..Default::default()
        };
        let results = store.query(&query).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].process, "api");
    }

    #[tokio::test]
    async fn test_log_store_fts_search() {
        let (pool, _dir) = create_test_db().await;
        let store = LogStore::new(pool);

        store.push(LogEntry::new("api", "prod", LogLevel::Stdout, "hello world".to_string())).await;
        store.push(LogEntry::new("api", "prod", LogLevel::Stdout, "goodbye world".to_string())).await;
        store.push(LogEntry::new("api", "prod", LogLevel::Stdout, "hello there".to_string())).await;

        tokio::time::sleep(Duration::from_millis(300)).await;

        let query = LogQuery {
            search: Some("hello".to_string()),
            ..Default::default()
        };
        let results = store.query(&query).await.unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_log_store_rotate() {
        let (pool, _dir) = create_test_db().await;
        let store = LogStore::new(pool);

        store.push(LogEntry::new("api", "prod", LogLevel::Stdout, "old msg".to_string())).await;
        tokio::time::sleep(Duration::from_millis(300)).await;

        // Rotate with 0 duration should delete all
        let deleted = store.rotate(Duration::from_secs(0)).await.unwrap();
        assert!(deleted >= 1);

        let count = store.count().await.unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_config_store_get_set() {
        let (pool, _dir) = create_test_db().await;
        let store = ConfigStore::new(pool);

        // Initially empty
        assert!(store.get("test_key").await.unwrap().is_none());

        // Set value
        store.set("test_key", "test_value").await.unwrap();
        assert_eq!(store.get("test_key").await.unwrap(), Some("test_value".to_string()));

        // Update value
        store.set("test_key", "new_value").await.unwrap();
        assert_eq!(store.get("test_key").await.unwrap(), Some("new_value".to_string()));

        // Delete value
        assert!(store.delete("test_key").await.unwrap());
        assert!(store.get("test_key").await.unwrap().is_none());
    }
}
