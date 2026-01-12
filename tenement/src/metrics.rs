//! Metrics collection and Prometheus export
//!
//! Simple in-memory metrics with Prometheus text format export.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

/// A counter metric (monotonically increasing)
#[derive(Debug, Default)]
pub struct Counter {
    value: AtomicU64,
}

impl Counter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn inc(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_by(&self, n: u64) {
        self.value.fetch_add(n, Ordering::Relaxed);
    }

    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }
}

/// A gauge metric (can go up or down)
#[derive(Debug, Default)]
pub struct Gauge {
    value: AtomicU64,
}

impl Gauge {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&self, v: u64) {
        self.value.store(v, Ordering::Relaxed);
    }

    pub fn inc(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    pub fn dec(&self) {
        self.value.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }
}

/// A histogram for tracking distributions (e.g., request latencies)
#[derive(Debug)]
pub struct Histogram {
    /// Bucket boundaries (upper bounds)
    buckets: Vec<f64>,
    /// Count per bucket
    bucket_counts: Vec<AtomicU64>,
    /// Total sum of all observations
    sum: AtomicU64,
    /// Total count of observations
    count: AtomicU64,
}

impl Histogram {
    /// Create a histogram with default latency buckets (in milliseconds)
    pub fn new() -> Self {
        Self::with_buckets(vec![1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 5000.0])
    }

    /// Create a histogram with custom bucket boundaries
    pub fn with_buckets(buckets: Vec<f64>) -> Self {
        let bucket_counts = buckets.iter().map(|_| AtomicU64::new(0)).collect();
        Self {
            buckets,
            bucket_counts,
            sum: AtomicU64::new(0),
            count: AtomicU64::new(0),
        }
    }

    /// Record an observation
    pub fn observe(&self, value: f64) {
        // Increment count
        self.count.fetch_add(1, Ordering::Relaxed);

        // Add to sum (store as bits for atomic operation)
        let value_bits = (value * 1000.0) as u64; // Store as micros for precision
        self.sum.fetch_add(value_bits, Ordering::Relaxed);

        // Find the first bucket that the value fits in and increment only that one
        // (we compute cumulative counts at export time)
        for (i, &bound) in self.buckets.iter().enumerate() {
            if value <= bound {
                self.bucket_counts[i].fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
        // Value exceeds all buckets - it will be counted in +Inf
    }

    pub fn get_count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    pub fn get_sum(&self) -> f64 {
        self.sum.load(Ordering::Relaxed) as f64 / 1000.0
    }

    pub fn get_bucket(&self, idx: usize) -> u64 {
        self.bucket_counts.get(idx).map(|c| c.load(Ordering::Relaxed)).unwrap_or(0)
    }

    pub fn buckets(&self) -> &[f64] {
        &self.buckets
    }
}

impl Default for Histogram {
    fn default() -> Self {
        Self::new()
    }
}

/// Labels for a metric
pub type Labels = HashMap<String, String>;

/// A labeled counter (counter per label combination)
#[derive(Debug, Default)]
pub struct LabeledCounter {
    counters: RwLock<HashMap<String, Arc<Counter>>>,
}

impl LabeledCounter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get or create a counter for the given labels
    pub async fn with_labels(&self, labels: &Labels) -> Arc<Counter> {
        let key = labels_to_key(labels);

        // Try read lock first
        {
            let counters = self.counters.read().await;
            if let Some(counter) = counters.get(&key) {
                return counter.clone();
            }
        }

        // Need to create
        let mut counters = self.counters.write().await;
        counters
            .entry(key)
            .or_insert_with(|| Arc::new(Counter::new()))
            .clone()
    }

    /// Get all counters with their label keys
    pub async fn all(&self) -> Vec<(String, u64)> {
        let counters = self.counters.read().await;
        counters
            .iter()
            .map(|(k, v)| (k.clone(), v.get()))
            .collect()
    }
}

/// A labeled gauge (gauge per label combination)
#[derive(Debug, Default)]
pub struct LabeledGauge {
    gauges: RwLock<HashMap<String, Arc<Gauge>>>,
}

impl LabeledGauge {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get or create a gauge for the given labels
    pub async fn with_labels(&self, labels: &Labels) -> Arc<Gauge> {
        let key = labels_to_key(labels);

        // Try read lock first
        {
            let gauges = self.gauges.read().await;
            if let Some(gauge) = gauges.get(&key) {
                return gauge.clone();
            }
        }

        // Need to create
        let mut gauges = self.gauges.write().await;
        gauges
            .entry(key)
            .or_insert_with(|| Arc::new(Gauge::new()))
            .clone()
    }

    /// Remove a gauge for the given labels (e.g., when instance stops)
    pub async fn remove(&self, labels: &Labels) {
        let key = labels_to_key(labels);
        let mut gauges = self.gauges.write().await;
        gauges.remove(&key);
    }

    /// Get all gauges with their label keys
    pub async fn all(&self) -> Vec<(String, u64)> {
        let gauges = self.gauges.read().await;
        gauges
            .iter()
            .map(|(k, v)| (k.clone(), v.get()))
            .collect()
    }
}

/// A labeled histogram
#[derive(Debug, Default)]
pub struct LabeledHistogram {
    histograms: RwLock<HashMap<String, Arc<Histogram>>>,
}

impl LabeledHistogram {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn with_labels(&self, labels: &Labels) -> Arc<Histogram> {
        let key = labels_to_key(labels);

        {
            let histograms = self.histograms.read().await;
            if let Some(histogram) = histograms.get(&key) {
                return histogram.clone();
            }
        }

        let mut histograms = self.histograms.write().await;
        histograms
            .entry(key)
            .or_insert_with(|| Arc::new(Histogram::new()))
            .clone()
    }

    pub async fn all(&self) -> Vec<(String, Arc<Histogram>)> {
        let histograms = self.histograms.read().await;
        histograms
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}

/// Convert labels to a stable string key
fn labels_to_key(labels: &Labels) -> String {
    let mut pairs: Vec<_> = labels.iter().collect();
    pairs.sort_by_key(|(k, _)| *k);
    pairs
        .iter()
        .map(|(k, v)| format!("{}=\"{}\"", k, v))
        .collect::<Vec<_>>()
        .join(",")
}

/// Parse a label key back to labels
fn key_to_labels(key: &str) -> Labels {
    if key.is_empty() {
        return HashMap::new();
    }
    key.split(',')
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            let k = parts.next()?;
            let v = parts.next()?.trim_matches('"');
            Some((k.to_string(), v.to_string()))
        })
        .collect()
}

/// Metrics registry
pub struct Metrics {
    /// Total HTTP requests
    pub requests_total: LabeledCounter,
    /// Request duration in milliseconds
    pub request_duration_ms: LabeledHistogram,
    /// Number of running instances
    pub instances_up: Gauge,
    /// Total instance restarts
    pub instance_restarts: LabeledCounter,
    /// Current storage usage in bytes per instance
    pub instance_storage_bytes: LabeledGauge,
    /// Configured storage quota in bytes per instance (0 = unlimited)
    pub instance_storage_quota_bytes: LabeledGauge,
    /// Storage usage ratio (0-10000, divide by 10000 to get 0.0-1.0)
    /// E.g., 2500 = 0.25 = 25% usage
    pub instance_storage_usage_ratio: LabeledGauge,
}

impl Metrics {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            requests_total: LabeledCounter::new(),
            request_duration_ms: LabeledHistogram::new(),
            instances_up: Gauge::new(),
            instance_restarts: LabeledCounter::new(),
            instance_storage_bytes: LabeledGauge::new(),
            instance_storage_quota_bytes: LabeledGauge::new(),
            instance_storage_usage_ratio: LabeledGauge::new(),
        })
    }

    /// Format metrics in Prometheus text format
    pub async fn format_prometheus(&self) -> String {
        let mut output = String::new();

        // tenement_requests_total
        output.push_str("# HELP tenement_requests_total Total number of HTTP requests\n");
        output.push_str("# TYPE tenement_requests_total counter\n");
        for (labels, value) in self.requests_total.all().await {
            if labels.is_empty() {
                output.push_str(&format!("tenement_requests_total {}\n", value));
            } else {
                output.push_str(&format!("tenement_requests_total{{{}}} {}\n", labels, value));
            }
        }

        // tenement_request_duration_ms
        output.push_str("\n# HELP tenement_request_duration_ms Request duration in milliseconds\n");
        output.push_str("# TYPE tenement_request_duration_ms histogram\n");
        for (labels, histogram) in self.request_duration_ms.all().await {
            let label_str = if labels.is_empty() {
                String::new()
            } else {
                format!("{},", labels)
            };

            // Bucket counts (cumulative)
            let mut cumulative = 0u64;
            for (i, &bound) in histogram.buckets().iter().enumerate() {
                cumulative += histogram.get_bucket(i);
                output.push_str(&format!(
                    "tenement_request_duration_ms_bucket{{{}le=\"{}\"}} {}\n",
                    label_str, bound, cumulative
                ));
            }
            output.push_str(&format!(
                "tenement_request_duration_ms_bucket{{{}le=\"+Inf\"}} {}\n",
                label_str,
                histogram.get_count()
            ));
            output.push_str(&format!(
                "tenement_request_duration_ms_sum{{{}}} {}\n",
                label_str.trim_end_matches(','),
                histogram.get_sum()
            ));
            output.push_str(&format!(
                "tenement_request_duration_ms_count{{{}}} {}\n",
                label_str.trim_end_matches(','),
                histogram.get_count()
            ));
        }

        // tenement_instances_up
        output.push_str("\n# HELP tenement_instances_up Number of running instances\n");
        output.push_str("# TYPE tenement_instances_up gauge\n");
        output.push_str(&format!("tenement_instances_up {}\n", self.instances_up.get()));

        // tenement_instance_restarts_total
        output.push_str("\n# HELP tenement_instance_restarts_total Total instance restarts\n");
        output.push_str("# TYPE tenement_instance_restarts_total counter\n");
        for (labels, value) in self.instance_restarts.all().await {
            if labels.is_empty() {
                output.push_str(&format!("tenement_instance_restarts_total {}\n", value));
            } else {
                output.push_str(&format!(
                    "tenement_instance_restarts_total{{{}}} {}\n",
                    labels, value
                ));
            }
        }

        // tenement_instance_storage_bytes
        output.push_str("\n# HELP tenement_instance_storage_bytes Current storage usage in bytes\n");
        output.push_str("# TYPE tenement_instance_storage_bytes gauge\n");
        for (labels, value) in self.instance_storage_bytes.all().await {
            if labels.is_empty() {
                output.push_str(&format!("tenement_instance_storage_bytes {}\n", value));
            } else {
                output.push_str(&format!(
                    "tenement_instance_storage_bytes{{{}}} {}\n",
                    labels, value
                ));
            }
        }

        // tenement_instance_storage_quota_bytes
        output.push_str("\n# HELP tenement_instance_storage_quota_bytes Configured storage quota in bytes (0 = unlimited)\n");
        output.push_str("# TYPE tenement_instance_storage_quota_bytes gauge\n");
        for (labels, value) in self.instance_storage_quota_bytes.all().await {
            if labels.is_empty() {
                output.push_str(&format!("tenement_instance_storage_quota_bytes {}\n", value));
            } else {
                output.push_str(&format!(
                    "tenement_instance_storage_quota_bytes{{{}}} {}\n",
                    labels, value
                ));
            }
        }

        // tenement_instance_storage_usage_ratio
        output.push_str("\n# HELP tenement_instance_storage_usage_ratio Storage usage ratio (0.0 to 1.0+)\n");
        output.push_str("# TYPE tenement_instance_storage_usage_ratio gauge\n");
        for (labels, value) in self.instance_storage_usage_ratio.all().await {
            // Value is stored as ratio * 10000, convert back to decimal
            let ratio = value as f64 / 10000.0;
            if labels.is_empty() {
                output.push_str(&format!("tenement_instance_storage_usage_ratio {:.4}\n", ratio));
            } else {
                output.push_str(&format!(
                    "tenement_instance_storage_usage_ratio{{{}}} {:.4}\n",
                    labels, ratio
                ));
            }
        }

        output
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            requests_total: LabeledCounter::new(),
            request_duration_ms: LabeledHistogram::new(),
            instances_up: Gauge::new(),
            instance_restarts: LabeledCounter::new(),
            instance_storage_bytes: LabeledGauge::new(),
            instance_storage_quota_bytes: LabeledGauge::new(),
            instance_storage_usage_ratio: LabeledGauge::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter_inc() {
        let counter = Counter::new();
        assert_eq!(counter.get(), 0);
        counter.inc();
        assert_eq!(counter.get(), 1);
        counter.inc();
        assert_eq!(counter.get(), 2);
    }

    #[test]
    fn test_counter_inc_by() {
        let counter = Counter::new();
        counter.inc_by(5);
        assert_eq!(counter.get(), 5);
        counter.inc_by(3);
        assert_eq!(counter.get(), 8);
    }

    #[test]
    fn test_gauge_set() {
        let gauge = Gauge::new();
        gauge.set(10);
        assert_eq!(gauge.get(), 10);
        gauge.set(5);
        assert_eq!(gauge.get(), 5);
    }

    #[test]
    fn test_gauge_inc_dec() {
        let gauge = Gauge::new();
        gauge.inc();
        gauge.inc();
        assert_eq!(gauge.get(), 2);
        gauge.dec();
        assert_eq!(gauge.get(), 1);
    }

    #[test]
    fn test_histogram_observe() {
        let histogram = Histogram::with_buckets(vec![10.0, 50.0, 100.0]);
        histogram.observe(5.0);   // -> bucket 0 (<=10)
        histogram.observe(25.0);  // -> bucket 1 (<=50)
        histogram.observe(75.0);  // -> bucket 2 (<=100)
        histogram.observe(8.0);   // -> bucket 0 (<=10)

        assert_eq!(histogram.get_count(), 4);
        // Two values (5, 8) fall into bucket 0 (<=10)
        assert_eq!(histogram.get_bucket(0), 2);
        // One value (25) falls into bucket 1 (<=50)
        assert_eq!(histogram.get_bucket(1), 1);
        // One value (75) falls into bucket 2 (<=100)
        assert_eq!(histogram.get_bucket(2), 1);
    }

    #[test]
    fn test_labels_to_key() {
        let mut labels = HashMap::new();
        labels.insert("process".to_string(), "api".to_string());
        labels.insert("id".to_string(), "prod".to_string());

        let key = labels_to_key(&labels);
        // Should be sorted alphabetically
        assert_eq!(key, "id=\"prod\",process=\"api\"");
    }

    #[test]
    fn test_key_to_labels() {
        let key = "id=\"prod\",process=\"api\"";
        let labels = key_to_labels(key);
        assert_eq!(labels.get("id"), Some(&"prod".to_string()));
        assert_eq!(labels.get("process"), Some(&"api".to_string()));
    }

    #[tokio::test]
    async fn test_labeled_counter() {
        let labeled = LabeledCounter::new();

        let mut labels1 = HashMap::new();
        labels1.insert("status".to_string(), "200".to_string());

        let mut labels2 = HashMap::new();
        labels2.insert("status".to_string(), "500".to_string());

        let c1 = labeled.with_labels(&labels1).await;
        c1.inc();
        c1.inc();

        let c2 = labeled.with_labels(&labels2).await;
        c2.inc();

        // Same labels should return same counter
        let c1_again = labeled.with_labels(&labels1).await;
        assert_eq!(c1_again.get(), 2);

        let all = labeled.all().await;
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn test_metrics_format_prometheus() {
        let metrics = Metrics::new();

        // Record some data
        let mut labels = HashMap::new();
        labels.insert("status".to_string(), "200".to_string());

        let counter = metrics.requests_total.with_labels(&labels).await;
        counter.inc();
        counter.inc();

        metrics.instances_up.set(3);

        let output = metrics.format_prometheus().await;

        assert!(output.contains("tenement_requests_total"));
        assert!(output.contains("status=\"200\""));
        assert!(output.contains("tenement_instances_up 3"));
    }
}
