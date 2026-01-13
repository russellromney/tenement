---
title: Monitoring Setup
description: Prometheus, Grafana, and alerting for tenement
---

tenement exports Prometheus metrics at `/metrics`. This guide covers setting up monitoring and alerting.

## Quick Start

### View Raw Metrics

```bash
curl http://localhost:8080/metrics
```

### Key Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `instance_count` | gauge | Total running instances |
| `instance_status{process,instance}` | gauge | Instance health (1=healthy) |
| `instance_uptime_seconds{process,instance}` | gauge | Instance uptime |
| `instance_restarts{process,instance}` | counter | Restart count |
| `instance_memory_bytes{process,instance}` | gauge | Memory usage |
| `instance_storage_bytes{process,instance}` | gauge | Disk usage |
| `instance_storage_quota_bytes{process,instance}` | gauge | Storage limit |
| `http_requests_total{method,path,status}` | counter | Request count |
| `http_request_duration_seconds{method,path}` | histogram | Request latency |

## Prometheus Setup

### Installation

```bash
# Ubuntu/Debian
apt install prometheus

# Or Docker
docker run -d -p 9090:9090 -v /etc/prometheus:/etc/prometheus prom/prometheus
```

### Configuration

Add to `/etc/prometheus/prometheus.yml`:

```yaml
scrape_configs:
  - job_name: 'tenement'
    static_configs:
      - targets: ['localhost:8080']
    metrics_path: /metrics
    scrape_interval: 15s
```

### Verify

```bash
# Restart Prometheus
systemctl restart prometheus

# Check targets
curl http://localhost:9090/api/v1/targets
```

## Grafana Setup

### Installation

```bash
# Ubuntu/Debian
apt install grafana

# Or Docker
docker run -d -p 3000:3000 grafana/grafana
```

### Add Prometheus Data Source

1. Open Grafana (http://localhost:3000)
2. Configuration → Data Sources → Add
3. Select Prometheus
4. URL: `http://localhost:9090`
5. Save & Test

### Import Dashboard

Create a dashboard with these panels:

**Instance Count**
```promql
instance_count
```

**Instance Health**
```promql
instance_status
```

**Memory Usage by Instance**
```promql
instance_memory_bytes
```

**Storage Usage**
```promql
instance_storage_bytes / instance_storage_quota_bytes * 100
```

**Request Rate**
```promql
rate(http_requests_total[5m])
```

**Request Latency (p99)**
```promql
histogram_quantile(0.99, rate(http_request_duration_seconds_bucket[5m]))
```

**Restart Rate**
```promql
increase(instance_restarts[1h])
```

## Alerting

### Prometheus Alerting Rules

Create `/etc/prometheus/alerts/tenement.yml`:

```yaml
groups:
  - name: tenement
    rules:
      # Instance down
      - alert: InstanceDown
        expr: instance_status == 0
        for: 1m
        labels:
          severity: critical
        annotations:
          summary: "Instance {{ $labels.process }}:{{ $labels.instance }} is down"
          description: "Instance has been unhealthy for more than 1 minute"

      # High restart rate
      - alert: HighRestartRate
        expr: increase(instance_restarts[1h]) > 5
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Instance {{ $labels.process }}:{{ $labels.instance }} restarting frequently"
          description: "Instance has restarted {{ $value }} times in the last hour"

      # Storage near limit
      - alert: StorageNearLimit
        expr: instance_storage_bytes / instance_storage_quota_bytes > 0.9
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Instance {{ $labels.process }}:{{ $labels.instance }} storage > 90%"
          description: "Storage at {{ $value | humanizePercentage }}"

      # No instances running
      - alert: NoInstancesRunning
        expr: instance_count == 0
        for: 1m
        labels:
          severity: critical
        annotations:
          summary: "No tenement instances running"
          description: "All instances have stopped"

      # High latency
      - alert: HighLatency
        expr: histogram_quantile(0.99, rate(http_request_duration_seconds_bucket[5m])) > 1
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "High API latency"
          description: "p99 latency is {{ $value }}s"
```

### Enable Alerts

Add to `prometheus.yml`:

```yaml
rule_files:
  - /etc/prometheus/alerts/*.yml
```

### Alertmanager (Optional)

For Slack/PagerDuty/email alerts:

```yaml
# /etc/alertmanager/alertmanager.yml
route:
  receiver: 'slack'

receivers:
  - name: 'slack'
    slack_configs:
      - api_url: 'https://hooks.slack.com/services/...'
        channel: '#alerts'
```

## Quick Health Checks

### CLI Check

```bash
# Instance health
ten ps

# Specific instance
ten health api:prod
```

### HTTP Check

```bash
# Server health
curl http://localhost:8080/health

# All instances via API
curl -H "Authorization: Bearer $TOKEN" http://localhost:8080/api/instances
```

### Monitoring Script

```bash
#!/bin/bash
# check-tenement.sh

# Check server is up
if ! curl -sf http://localhost:8080/health > /dev/null; then
    echo "CRITICAL: tenement server down"
    exit 2
fi

# Check instance count
COUNT=$(curl -s http://localhost:8080/metrics | grep "^instance_count" | awk '{print $2}')
if [ "$COUNT" -eq 0 ]; then
    echo "WARNING: no instances running"
    exit 1
fi

echo "OK: $COUNT instances running"
exit 0
```

## Log Aggregation

tenement doesn't persist logs. Ship to external service:

### Using Vector

```toml
# /etc/vector/vector.toml
[sources.tenement_api]
type = "http_client"
endpoint = "http://localhost:8080/api/logs/stream"
headers.Authorization = "Bearer ${TENEMENT_TOKEN}"

[sinks.loki]
type = "loki"
inputs = ["tenement_api"]
endpoint = "http://loki:3100"
```

### Using Promtail

```yaml
# /etc/promtail/config.yml
scrape_configs:
  - job_name: tenement
    static_configs:
      - targets:
          - localhost
        labels:
          job: tenement
          __path__: /var/log/tenement/*.log
```

## Next Steps

- [Upgrading](/guides/07-upgrading) - Zero-downtime upgrades
- [Backup and Restore](/guides/08-backup) - Data preservation
- [Troubleshooting](/reference/troubleshooting) - Debugging issues
