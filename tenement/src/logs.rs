//! Log capture and storage
//!
//! Captures stdout/stderr from spawned processes and stores them in a ring buffer.
//! Provides real-time streaming via broadcast channel.

use serde::Serialize;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, RwLock};

/// Default capacity for the ring buffer (per instance)
const DEFAULT_BUFFER_CAPACITY: usize = 10_000;

/// Log level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Stdout,
    Stderr,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Stdout => write!(f, "stdout"),
            LogLevel::Stderr => write!(f, "stderr"),
        }
    }
}

/// A single log entry
#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    /// Unix timestamp in milliseconds
    pub timestamp: u64,
    /// Log level (stdout or stderr)
    pub level: LogLevel,
    /// Process name
    pub process: String,
    /// Instance ID
    pub instance_id: String,
    /// Log message
    pub message: String,
}

impl LogEntry {
    /// Create a new log entry with current timestamp
    pub fn new(process: &str, instance_id: &str, level: LogLevel, message: String) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Self {
            timestamp,
            level,
            process: process.to_string(),
            instance_id: instance_id.to_string(),
            message,
        }
    }
}

/// Query parameters for filtering logs
#[derive(Debug, Default, Clone)]
pub struct LogQuery {
    /// Filter by process name
    pub process: Option<String>,
    /// Filter by instance ID
    pub instance_id: Option<String>,
    /// Filter by log level
    pub level: Option<LogLevel>,
    /// Maximum number of entries to return
    pub limit: Option<usize>,
    /// Text search (simple substring match)
    pub search: Option<String>,
}

/// Ring buffer for log entries
#[derive(Debug)]
struct RingBuffer {
    entries: VecDeque<LogEntry>,
    capacity: usize,
}

impl RingBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    fn push(&mut self, entry: LogEntry) {
        if self.entries.len() >= self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    fn query(&self, query: &LogQuery) -> Vec<LogEntry> {
        let mut results: Vec<LogEntry> = self
            .entries
            .iter()
            .filter(|e| {
                // Filter by process
                if let Some(ref p) = query.process {
                    if &e.process != p {
                        return false;
                    }
                }
                // Filter by instance_id
                if let Some(ref id) = query.instance_id {
                    if &e.instance_id != id {
                        return false;
                    }
                }
                // Filter by level
                if let Some(level) = query.level {
                    if e.level != level {
                        return false;
                    }
                }
                // Filter by search text
                if let Some(ref search) = query.search {
                    if !e.message.contains(search) {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect();

        // Apply limit (take from end - most recent)
        if let Some(limit) = query.limit {
            if results.len() > limit {
                results = results.split_off(results.len() - limit);
            }
        }

        results
    }

    fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Log buffer with broadcast channel for streaming
pub struct LogBuffer {
    buffer: RwLock<RingBuffer>,
    sender: broadcast::Sender<LogEntry>,
}

impl LogBuffer {
    /// Create a new log buffer with default capacity
    pub fn new() -> Arc<Self> {
        Self::with_capacity(DEFAULT_BUFFER_CAPACITY)
    }

    /// Create a new log buffer with specified capacity
    pub fn with_capacity(capacity: usize) -> Arc<Self> {
        let (sender, _) = broadcast::channel(1024);
        Arc::new(Self {
            buffer: RwLock::new(RingBuffer::new(capacity)),
            sender,
        })
    }

    /// Push a log entry to the buffer and broadcast it
    pub async fn push(&self, entry: LogEntry) {
        // Store in ring buffer
        {
            let mut buffer = self.buffer.write().await;
            buffer.push(entry.clone());
        }

        // Broadcast to subscribers (ignore if no receivers)
        let _ = self.sender.send(entry);
    }

    /// Push a stdout log entry
    pub async fn push_stdout(&self, process: &str, instance_id: &str, message: String) {
        let entry = LogEntry::new(process, instance_id, LogLevel::Stdout, message);
        self.push(entry).await;
    }

    /// Push a stderr log entry
    pub async fn push_stderr(&self, process: &str, instance_id: &str, message: String) {
        let entry = LogEntry::new(process, instance_id, LogLevel::Stderr, message);
        self.push(entry).await;
    }

    /// Query logs with filters
    pub async fn query(&self, query: &LogQuery) -> Vec<LogEntry> {
        let buffer = self.buffer.read().await;
        buffer.query(query)
    }

    /// Get the number of entries in the buffer
    pub async fn len(&self) -> usize {
        let buffer = self.buffer.read().await;
        buffer.len()
    }

    /// Check if the buffer is empty
    pub async fn is_empty(&self) -> bool {
        self.len().await == 0
    }

    /// Subscribe to the log stream
    pub fn subscribe(&self) -> broadcast::Receiver<LogEntry> {
        self.sender.subscribe()
    }
}

impl Default for LogBuffer {
    fn default() -> Self {
        let (sender, _) = broadcast::channel(1024);
        Self {
            buffer: RwLock::new(RingBuffer::new(DEFAULT_BUFFER_CAPACITY)),
            sender,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===================
    // LOG LEVEL TESTS
    // ===================

    #[test]
    fn test_log_level_display() {
        assert_eq!(LogLevel::Stdout.to_string(), "stdout");
        assert_eq!(LogLevel::Stderr.to_string(), "stderr");
    }

    #[test]
    fn test_log_level_equality() {
        assert_eq!(LogLevel::Stdout, LogLevel::Stdout);
        assert_ne!(LogLevel::Stdout, LogLevel::Stderr);
    }

    #[test]
    fn test_log_level_clone() {
        let level = LogLevel::Stdout;
        let cloned = level.clone();
        assert_eq!(level, cloned);
    }

    #[test]
    fn test_log_level_copy() {
        let level = LogLevel::Stderr;
        let copied: LogLevel = level;
        assert_eq!(level, copied);
    }

    #[test]
    fn test_log_level_serialize() {
        let stdout = LogLevel::Stdout;
        let json = serde_json::to_string(&stdout).unwrap();
        assert_eq!(json, "\"stdout\"");

        let stderr = LogLevel::Stderr;
        let json = serde_json::to_string(&stderr).unwrap();
        assert_eq!(json, "\"stderr\"");
    }

    // ===================
    // LOG ENTRY TESTS
    // ===================

    #[test]
    fn test_log_entry_new() {
        let entry = LogEntry::new("api", "prod", LogLevel::Stdout, "hello".to_string());
        assert_eq!(entry.process, "api");
        assert_eq!(entry.instance_id, "prod");
        assert_eq!(entry.level, LogLevel::Stdout);
        assert_eq!(entry.message, "hello");
        assert!(entry.timestamp > 0);
    }

    #[test]
    fn test_log_entry_timestamp_increases() {
        let entry1 = LogEntry::new("api", "prod", LogLevel::Stdout, "first".to_string());
        std::thread::sleep(std::time::Duration::from_millis(5));
        let entry2 = LogEntry::new("api", "prod", LogLevel::Stdout, "second".to_string());

        assert!(entry2.timestamp >= entry1.timestamp);
    }

    #[test]
    fn test_log_entry_clone() {
        let entry = LogEntry::new("api", "prod", LogLevel::Stderr, "error".to_string());
        let cloned = entry.clone();

        assert_eq!(entry.timestamp, cloned.timestamp);
        assert_eq!(entry.process, cloned.process);
        assert_eq!(entry.message, cloned.message);
    }

    #[test]
    fn test_log_entry_serialize() {
        let entry = LogEntry::new("api", "prod", LogLevel::Stdout, "test message".to_string());
        let json = serde_json::to_string(&entry).unwrap();

        assert!(json.contains("api"));
        assert!(json.contains("prod"));
        assert!(json.contains("stdout"));
        assert!(json.contains("test message"));
    }

    #[test]
    fn test_log_entry_empty_message() {
        let entry = LogEntry::new("api", "prod", LogLevel::Stdout, "".to_string());
        assert_eq!(entry.message, "");
    }

    #[test]
    fn test_log_entry_long_message() {
        let long_message = "x".repeat(10000);
        let entry = LogEntry::new("api", "prod", LogLevel::Stdout, long_message.clone());
        assert_eq!(entry.message.len(), 10000);
    }

    #[test]
    fn test_log_entry_special_chars() {
        let entry = LogEntry::new(
            "api",
            "prod",
            LogLevel::Stdout,
            "message with\nnewline\tand\ttabs".to_string(),
        );
        assert!(entry.message.contains('\n'));
        assert!(entry.message.contains('\t'));
    }

    // ===================
    // RING BUFFER TESTS
    // ===================

    #[test]
    fn test_ring_buffer_push() {
        let mut buffer = RingBuffer::new(10);
        buffer.push(LogEntry::new("api", "prod", LogLevel::Stdout, "msg1".to_string()));
        buffer.push(LogEntry::new("api", "prod", LogLevel::Stdout, "msg2".to_string()));
        assert_eq!(buffer.len(), 2);
    }

    #[test]
    fn test_ring_buffer_eviction() {
        let mut buffer = RingBuffer::new(3);
        buffer.push(LogEntry::new("api", "prod", LogLevel::Stdout, "msg1".to_string()));
        buffer.push(LogEntry::new("api", "prod", LogLevel::Stdout, "msg2".to_string()));
        buffer.push(LogEntry::new("api", "prod", LogLevel::Stdout, "msg3".to_string()));
        buffer.push(LogEntry::new("api", "prod", LogLevel::Stdout, "msg4".to_string()));

        assert_eq!(buffer.len(), 3);

        let query = LogQuery::default();
        let results = buffer.query(&query);
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].message, "msg2"); // msg1 was evicted
        assert_eq!(results[1].message, "msg3");
        assert_eq!(results[2].message, "msg4");
    }

    #[test]
    fn test_ring_buffer_at_exact_capacity() {
        let mut buffer = RingBuffer::new(3);
        buffer.push(LogEntry::new("api", "prod", LogLevel::Stdout, "msg1".to_string()));
        buffer.push(LogEntry::new("api", "prod", LogLevel::Stdout, "msg2".to_string()));
        buffer.push(LogEntry::new("api", "prod", LogLevel::Stdout, "msg3".to_string()));

        assert_eq!(buffer.len(), 3);
        assert_eq!(buffer.capacity, 3);

        let results = buffer.query(&LogQuery::default());
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].message, "msg1");
    }

    #[test]
    fn test_ring_buffer_single_capacity() {
        let mut buffer = RingBuffer::new(1);
        buffer.push(LogEntry::new("api", "prod", LogLevel::Stdout, "msg1".to_string()));
        assert_eq!(buffer.len(), 1);

        buffer.push(LogEntry::new("api", "prod", LogLevel::Stdout, "msg2".to_string()));
        assert_eq!(buffer.len(), 1);

        let results = buffer.query(&LogQuery::default());
        assert_eq!(results[0].message, "msg2");
    }

    #[test]
    fn test_ring_buffer_empty() {
        let buffer = RingBuffer::new(10);
        assert_eq!(buffer.len(), 0);

        let results = buffer.query(&LogQuery::default());
        assert!(results.is_empty());
    }

    // ===================
    // QUERY FILTER TESTS
    // ===================

    #[test]
    fn test_ring_buffer_query_filter_process() {
        let mut buffer = RingBuffer::new(10);
        buffer.push(LogEntry::new("api", "prod", LogLevel::Stdout, "api msg".to_string()));
        buffer.push(LogEntry::new("web", "prod", LogLevel::Stdout, "web msg".to_string()));

        let query = LogQuery {
            process: Some("api".to_string()),
            ..Default::default()
        };
        let results = buffer.query(&query);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].process, "api");
    }

    #[test]
    fn test_ring_buffer_query_filter_instance() {
        let mut buffer = RingBuffer::new(10);
        buffer.push(LogEntry::new("api", "prod", LogLevel::Stdout, "prod msg".to_string()));
        buffer.push(LogEntry::new("api", "staging", LogLevel::Stdout, "staging msg".to_string()));

        let query = LogQuery {
            instance_id: Some("prod".to_string()),
            ..Default::default()
        };
        let results = buffer.query(&query);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].instance_id, "prod");
    }

    #[test]
    fn test_ring_buffer_query_filter_level() {
        let mut buffer = RingBuffer::new(10);
        buffer.push(LogEntry::new("api", "prod", LogLevel::Stdout, "out".to_string()));
        buffer.push(LogEntry::new("api", "prod", LogLevel::Stderr, "err".to_string()));

        let query = LogQuery {
            level: Some(LogLevel::Stderr),
            ..Default::default()
        };
        let results = buffer.query(&query);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].level, LogLevel::Stderr);
    }

    #[test]
    fn test_ring_buffer_query_combined_filters() {
        let mut buffer = RingBuffer::new(10);
        buffer.push(LogEntry::new("api", "prod", LogLevel::Stdout, "api prod out".to_string()));
        buffer.push(LogEntry::new("api", "prod", LogLevel::Stderr, "api prod err".to_string()));
        buffer.push(LogEntry::new("api", "staging", LogLevel::Stderr, "api staging err".to_string()));
        buffer.push(LogEntry::new("web", "prod", LogLevel::Stderr, "web prod err".to_string()));

        let query = LogQuery {
            process: Some("api".to_string()),
            instance_id: Some("prod".to_string()),
            level: Some(LogLevel::Stderr),
            ..Default::default()
        };
        let results = buffer.query(&query);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].message, "api prod err");
    }

    #[test]
    fn test_ring_buffer_query_no_match() {
        let mut buffer = RingBuffer::new(10);
        buffer.push(LogEntry::new("api", "prod", LogLevel::Stdout, "msg".to_string()));

        let query = LogQuery {
            process: Some("nonexistent".to_string()),
            ..Default::default()
        };
        let results = buffer.query(&query);
        assert!(results.is_empty());
    }

    // ===================
    // QUERY LIMIT TESTS
    // ===================

    #[test]
    fn test_ring_buffer_query_limit() {
        let mut buffer = RingBuffer::new(10);
        for i in 0..5 {
            buffer.push(LogEntry::new(
                "api",
                "prod",
                LogLevel::Stdout,
                format!("msg{}", i),
            ));
        }

        let query = LogQuery {
            limit: Some(2),
            ..Default::default()
        };
        let results = buffer.query(&query);
        assert_eq!(results.len(), 2);
        // Should return the most recent 2
        assert_eq!(results[0].message, "msg3");
        assert_eq!(results[1].message, "msg4");
    }

    #[test]
    fn test_ring_buffer_query_limit_larger_than_buffer() {
        let mut buffer = RingBuffer::new(10);
        buffer.push(LogEntry::new("api", "prod", LogLevel::Stdout, "msg1".to_string()));
        buffer.push(LogEntry::new("api", "prod", LogLevel::Stdout, "msg2".to_string()));

        let query = LogQuery {
            limit: Some(100),
            ..Default::default()
        };
        let results = buffer.query(&query);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_ring_buffer_query_limit_zero() {
        let mut buffer = RingBuffer::new(10);
        buffer.push(LogEntry::new("api", "prod", LogLevel::Stdout, "msg".to_string()));

        let query = LogQuery {
            limit: Some(0),
            ..Default::default()
        };
        let results = buffer.query(&query);
        assert!(results.is_empty());
    }

    // ===================
    // SEARCH TESTS
    // ===================

    #[test]
    fn test_ring_buffer_query_search() {
        let mut buffer = RingBuffer::new(10);
        buffer.push(LogEntry::new("api", "prod", LogLevel::Stdout, "hello world".to_string()));
        buffer.push(LogEntry::new("api", "prod", LogLevel::Stdout, "goodbye".to_string()));
        buffer.push(LogEntry::new("api", "prod", LogLevel::Stderr, "error: world".to_string()));

        let query = LogQuery {
            search: Some("world".to_string()),
            ..Default::default()
        };
        let results = buffer.query(&query);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_ring_buffer_query_search_case_sensitive() {
        let mut buffer = RingBuffer::new(10);
        buffer.push(LogEntry::new("api", "prod", LogLevel::Stdout, "Hello World".to_string()));
        buffer.push(LogEntry::new("api", "prod", LogLevel::Stdout, "hello world".to_string()));

        let query = LogQuery {
            search: Some("Hello".to_string()),
            ..Default::default()
        };
        let results = buffer.query(&query);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_ring_buffer_query_search_no_match() {
        let mut buffer = RingBuffer::new(10);
        buffer.push(LogEntry::new("api", "prod", LogLevel::Stdout, "hello world".to_string()));

        let query = LogQuery {
            search: Some("xyz".to_string()),
            ..Default::default()
        };
        let results = buffer.query(&query);
        assert!(results.is_empty());
    }

    #[test]
    fn test_ring_buffer_query_search_empty_string() {
        let mut buffer = RingBuffer::new(10);
        buffer.push(LogEntry::new("api", "prod", LogLevel::Stdout, "hello".to_string()));
        buffer.push(LogEntry::new("api", "prod", LogLevel::Stdout, "world".to_string()));

        let query = LogQuery {
            search: Some("".to_string()),
            ..Default::default()
        };
        let results = buffer.query(&query);
        // Empty string matches everything
        assert_eq!(results.len(), 2);
    }

    // ===================
    // LOG BUFFER ASYNC TESTS
    // ===================

    #[tokio::test]
    async fn test_log_buffer_push_and_query() {
        let buffer = LogBuffer::new();
        buffer.push_stdout("api", "prod", "hello".to_string()).await;
        buffer.push_stderr("api", "prod", "error".to_string()).await;

        let results = buffer.query(&LogQuery::default()).await;
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_log_buffer_len() {
        let buffer = LogBuffer::new();
        assert_eq!(buffer.len().await, 0);

        buffer.push_stdout("api", "prod", "msg".to_string()).await;
        assert_eq!(buffer.len().await, 1);

        buffer.push_stderr("api", "prod", "err".to_string()).await;
        assert_eq!(buffer.len().await, 2);
    }

    #[tokio::test]
    async fn test_log_buffer_is_empty() {
        let buffer = LogBuffer::new();
        assert!(buffer.is_empty().await);

        buffer.push_stdout("api", "prod", "msg".to_string()).await;
        assert!(!buffer.is_empty().await);
    }

    #[tokio::test]
    async fn test_log_buffer_subscribe() {
        let buffer = LogBuffer::new();
        let mut rx = buffer.subscribe();

        buffer.push_stdout("api", "prod", "test".to_string()).await;

        let entry = rx.recv().await.unwrap();
        assert_eq!(entry.message, "test");
    }

    #[tokio::test]
    async fn test_log_buffer_multiple_subscribers() {
        let buffer = LogBuffer::new();
        let mut rx1 = buffer.subscribe();
        let mut rx2 = buffer.subscribe();

        buffer.push_stdout("api", "prod", "broadcast".to_string()).await;

        let entry1 = rx1.recv().await.unwrap();
        let entry2 = rx2.recv().await.unwrap();

        assert_eq!(entry1.message, "broadcast");
        assert_eq!(entry2.message, "broadcast");
    }

    #[tokio::test]
    async fn test_log_buffer_with_capacity() {
        let buffer = LogBuffer::with_capacity(5);

        for i in 0..10 {
            buffer.push_stdout("api", "prod", format!("msg{}", i)).await;
        }

        let results = buffer.query(&LogQuery::default()).await;
        assert_eq!(results.len(), 5);
        // Should have the 5 most recent
        assert_eq!(results[0].message, "msg5");
        assert_eq!(results[4].message, "msg9");
    }

    #[tokio::test]
    async fn test_log_buffer_push_stdout() {
        let buffer = LogBuffer::new();
        buffer.push_stdout("api", "prod", "stdout msg".to_string()).await;

        let results = buffer.query(&LogQuery {
            level: Some(LogLevel::Stdout),
            ..Default::default()
        }).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].level, LogLevel::Stdout);
    }

    #[tokio::test]
    async fn test_log_buffer_push_stderr() {
        let buffer = LogBuffer::new();
        buffer.push_stderr("api", "prod", "stderr msg".to_string()).await;

        let results = buffer.query(&LogQuery {
            level: Some(LogLevel::Stderr),
            ..Default::default()
        }).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].level, LogLevel::Stderr);
    }

    // ===================
    // LOG QUERY TESTS
    // ===================

    #[test]
    fn test_log_query_default() {
        let query = LogQuery::default();
        assert!(query.process.is_none());
        assert!(query.instance_id.is_none());
        assert!(query.level.is_none());
        assert!(query.limit.is_none());
        assert!(query.search.is_none());
    }

    #[test]
    fn test_log_query_clone() {
        let query = LogQuery {
            process: Some("api".to_string()),
            instance_id: Some("prod".to_string()),
            level: Some(LogLevel::Stderr),
            limit: Some(100),
            search: Some("error".to_string()),
        };
        let cloned = query.clone();

        assert_eq!(query.process, cloned.process);
        assert_eq!(query.instance_id, cloned.instance_id);
        assert_eq!(query.level, cloned.level);
        assert_eq!(query.limit, cloned.limit);
        assert_eq!(query.search, cloned.search);
    }

    #[test]
    fn test_log_query_debug() {
        let query = LogQuery {
            process: Some("api".to_string()),
            ..Default::default()
        };
        let debug = format!("{:?}", query);
        assert!(debug.contains("api"));
    }
}
