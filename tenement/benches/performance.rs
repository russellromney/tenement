//! Performance Benchmarks
//!
//! Criterion benchmarks to establish performance baselines.
//! Part of Session 7 of the E2E Testing Plan.

use criterion::{criterion_group, criterion_main, Criterion};
use std::collections::HashMap;
use tenement::runtime::RuntimeType;
use tenement::{Config, Hypervisor, LogQuery};
use tenement::config::ProcessConfig;
use tokio::runtime::Runtime;

/// Create a test config with a process
fn test_config_with_process(name: &str, command: &str) -> Config {
    let mut config = Config::default();
    config.settings.data_dir = std::env::temp_dir().join("tenement-bench");
    config.settings.backoff_base_ms = 0;

    let process = ProcessConfig {
        command: command.to_string(),
        args: vec![],
        socket: "/tmp/{name}-{id}.sock".to_string(),
        isolation: RuntimeType::Process,
        health: None,
        env: HashMap::new(),
        workdir: None,
        restart: "on-failure".to_string(),
        idle_timeout: None,
        startup_timeout: 5,
        memory_limit_mb: None,
        cpu_shares: None,
        kernel: None,
        rootfs: None,
        memory_mb: 256,
        vcpus: 1,
        vsock_port: 5000,
    };

    config.service.insert(name.to_string(), process);
    config
}

/// Benchmark log buffer push throughput
fn bench_log_buffer_push(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let config = Config::default();
    let hypervisor = rt.block_on(async { Hypervisor::new(config) });
    let log_buffer = hypervisor.log_buffer();

    c.bench_function("log_buffer_push", |b| {
        b.to_async(&rt).iter(|| async {
            log_buffer
                .push_stdout("api", "bench", "test message for benchmarking".to_string())
                .await;
        })
    });
}

/// Benchmark log buffer query
fn bench_log_buffer_query(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let config = Config::default();
    let hypervisor = rt.block_on(async { Hypervisor::new(config) });
    let log_buffer = hypervisor.log_buffer();

    // Pre-populate buffer with entries
    rt.block_on(async {
        for i in 0..1000 {
            log_buffer
                .push_stdout("api", "bench", format!("log entry {}", i))
                .await;
        }
    });

    let query = LogQuery {
        process: Some("api".to_string()),
        instance_id: Some("bench".to_string()),
        level: None,
        search: None,
        limit: Some(100),
    };

    c.bench_function("log_buffer_query_100", |b| {
        b.to_async(&rt).iter(|| async {
            log_buffer.query(&query).await
        })
    });
}

/// Benchmark FTS search on log buffer
fn bench_fts_search(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let config = Config::default();
    let hypervisor = rt.block_on(async { Hypervisor::new(config) });
    let log_buffer = hypervisor.log_buffer();

    // Pre-populate buffer with varied entries
    rt.block_on(async {
        for i in 0..10000 {
            let msg = if i % 10 == 0 {
                format!("error occurred at step {}", i)
            } else if i % 5 == 0 {
                format!("warning: check value {}", i)
            } else {
                format!("info: processing item {}", i)
            };
            log_buffer.push_stdout("api", "search", msg).await;
        }
    });

    let query = LogQuery {
        process: Some("api".to_string()),
        instance_id: Some("search".to_string()),
        level: None,
        search: Some("error".to_string()),
        limit: Some(100),
    };

    c.bench_function("fts_search_10k_entries", |b| {
        b.to_async(&rt).iter(|| async {
            log_buffer.query(&query).await
        })
    });
}

/// Benchmark metrics formatting
fn bench_metrics_format(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let config = Config::default();
    let hypervisor = rt.block_on(async { Hypervisor::new(config) });
    let metrics = hypervisor.metrics();

    // Set some gauge values for realistic benchmark
    metrics.instances_up.set(10);

    c.bench_function("metrics_format_prometheus", |b| {
        b.to_async(&rt).iter(|| async {
            metrics.format_prometheus().await
        })
    });
}

/// Benchmark config parsing from TOML string
fn bench_config_parse(c: &mut Criterion) {
    let config_str = r#"
[settings]
data_dir = "/var/lib/tenement"
health_check_interval = 30
max_restarts = 5
backoff_base_ms = 100
backoff_max_ms = 30000

[service.api]
command = "/usr/bin/python"
args = ["-m", "http.server", "8000"]
socket = "/tmp/api-{id}.sock"
isolation = "process"
restart = "on-failure"
startup_timeout = 30
idle_timeout = 300
memory_limit_mb = 512
cpu_shares = 1024

[service.worker]
command = "/usr/bin/node"
args = ["worker.js"]
socket = "/tmp/worker-{id}.sock"
isolation = "process"
restart = "always"
"#;

    c.bench_function("config_parse_toml", |b| {
        b.iter(|| {
            let _config: Config = toml::from_str(config_str).unwrap();
        })
    });
}

/// Benchmark health check latency (without actual process)
fn bench_health_check_latency(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let config = test_config_with_process("api", "sleep");
    let hypervisor = rt.block_on(async { Hypervisor::new(config) });

    // Health check on non-existent instance (fast path - no socket)
    c.bench_function("health_check_nonexistent", |b| {
        b.to_async(&rt).iter(|| async {
            hypervisor.check_health("api", "nonexistent").await
        })
    });
}

/// Benchmark hypervisor list operation
fn bench_hypervisor_list(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let config = Config::default();
    let hypervisor = rt.block_on(async { Hypervisor::new(config) });

    c.bench_function("hypervisor_list_empty", |b| {
        b.to_async(&rt).iter(|| async {
            hypervisor.list().await
        })
    });
}

/// Benchmark instance lookup
fn bench_instance_get(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let config = Config::default();
    let hypervisor = rt.block_on(async { Hypervisor::new(config) });

    c.bench_function("instance_get_nonexistent", |b| {
        b.to_async(&rt).iter(|| async {
            hypervisor.get("api", "nonexistent").await
        })
    });
}

criterion_group!(
    benches,
    bench_log_buffer_push,
    bench_log_buffer_query,
    bench_fts_search,
    bench_metrics_format,
    bench_config_parse,
    bench_health_check_latency,
    bench_hypervisor_list,
    bench_instance_get,
);
criterion_main!(benches);
