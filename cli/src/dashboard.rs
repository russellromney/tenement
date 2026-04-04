//! Dashboard template rendering using askama
//!
//! Server-rendered HTML templates with HTMX for interactivity.

use askama::Template;

#[derive(Template)]
#[template(path = "base.html")]
struct BaseTemplate<'a> {
    auth_token: &'a str,
    summary: Option<SummaryData>,
    active_tab: &'a str,
}

#[derive(Template)]
#[template(path = "login.html")]
pub struct LoginTemplate {
    pub error: Option<String>,
}

#[derive(Template)]
#[template(path = "overview.html")]
pub struct OverviewTemplate<'a> {
    pub auth_token: &'a str,
    pub summary: Option<SummaryData>,
    pub active_tab: &'a str,
    pub instances: Vec<InstanceRow>,
    pub error: Option<String>,
}

#[derive(Template)]
#[template(path = "instances.html")]
pub struct InstancesTemplate<'a> {
    pub auth_token: &'a str,
    pub summary: Option<SummaryData>,
    pub active_tab: &'a str,
    pub instances: Vec<InstanceRow>,
    pub error: Option<String>,
}

#[derive(Template)]
#[template(path = "logs.html")]
pub struct LogsTemplate<'a> {
    pub auth_token: &'a str,
    pub summary: Option<SummaryData>,
    pub active_tab: &'a str,
    pub logs: Vec<LogEntry>,
    pub processes: Vec<String>,
    pub filter_process: String,
    pub filter_level: String,
    pub search: String,
    pub error: Option<String>,
}

// Content-only templates (no header/nav, for HTMX partial swaps)
#[derive(Template)]
#[template(path = "overview_content.html")]
pub struct OverviewContentTemplate<'a> {
    pub auth_token: &'a str,
    pub summary: Option<SummaryData>,
    pub active_tab: &'a str,
    pub instances: Vec<InstanceRow>,
    pub error: Option<String>,
}

#[derive(Template)]
#[template(path = "instances_content.html")]
pub struct InstancesContentTemplate<'a> {
    pub auth_token: &'a str,
    pub summary: Option<SummaryData>,
    pub active_tab: &'a str,
    pub instances: Vec<InstanceRow>,
    pub error: Option<String>,
}

#[derive(Template)]
#[template(path = "logs_content.html")]
pub struct LogsContentTemplate<'a> {
    pub auth_token: &'a str,
    pub summary: Option<SummaryData>,
    pub active_tab: &'a str,
    pub logs: Vec<LogEntry>,
    pub processes: Vec<String>,
    pub filter_process: String,
    pub filter_level: String,
    pub search: String,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SummaryData {
    pub total_instances: usize,
    pub healthy_instances: usize,
    pub total_requests: u64,
}

#[derive(Clone, Debug)]
pub struct InstanceRow {
    pub id: String,
    pub health: String,
    pub health_badge: String,
    pub health_color: String,
    pub requests_total: String,
    pub avg_latency_ms: String,
    pub uptime: String,
    pub idle: String,
    pub restarts: String,
    pub weight: String,
    pub storage: String,
}

#[derive(Clone, Debug)]
pub struct LogEntry {
    pub time: String,
    pub process: String,
    pub level_label: String,
    pub is_error: bool,
    pub message: String,
}

/// Format seconds into human-readable duration
pub fn format_duration(secs: u64) -> String {
    if secs == 0 {
        return "0s".to_string();
    }
    if secs < 60 {
        return format!("{secs}s");
    }
    if secs < 3600 {
        return format!("{}m", secs / 60);
    }
    if secs < 86400 {
        return format!("{}h", secs / 3600);
    }
    format!("{}d", secs / 86400)
}

/// Format bytes into human-readable size
pub fn format_bytes(bytes: u64) -> String {
    let kb = 1024u64;
    let mb = kb * 1024;
    let gb = mb * 1024;
    if bytes >= gb {
        return format!("{:.1}GB", bytes as f64 / gb as f64);
    }
    if bytes >= mb {
        return format!("{}MB", bytes / mb);
    }
    if bytes >= kb {
        return format!("{}KB", bytes / kb);
    }
    format!("{bytes}B")
}

/// Get health badge class
pub fn health_badge(status: &str) -> &'static str {
    match status {
        "healthy" => "green",
        "degraded" => "yellow",
        "unhealthy" | "failed" => "red",
        _ => "gray",
    }
}

/// Get health indicator color
pub fn health_color(status: &str) -> &'static str {
    match status {
        "healthy" => "#22c55e",
        "degraded" => "#eab308",
        "unhealthy" | "failed" => "#ef4444",
        _ => "#6b7280",
    }
}
