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

    #[tokio::test]
    async fn test_log_buffer_push_and_query() {
        let buffer = LogBuffer::new();
        buffer.push_stdout("api", "prod", "hello".to_string()).await;
        buffer.push_stderr("api", "prod", "error".to_string()).await;

        let results = buffer.query(&LogQuery::default()).await;
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_log_buffer_subscribe() {
        let buffer = LogBuffer::new();
        let mut rx = buffer.subscribe();

        buffer.push_stdout("api", "prod", "test".to_string()).await;

        let entry = rx.recv().await.unwrap();
        assert_eq!(entry.message, "test");
    }
}
