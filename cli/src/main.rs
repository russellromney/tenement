use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tenement::{init_db, Config, ConfigStore, Hypervisor, TokenStore};

use tenement_cli::client::{self, ApiClient};
use tenement_cli::server;

mod install;
mod caddy;

#[derive(Parser)]
#[command(name = "tenement")]
#[command(author, version, about = "Hyperlightweight process hypervisor")]
struct Cli {
    /// Server URL for CLI commands (default: http://localhost:8080)
    #[arg(long, default_value = "http://localhost:8080", global = true)]
    server: String,

    /// API token (overrides TENEMENT_TOKEN env var and token file)
    #[arg(long, global = true)]
    token: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the HTTP server with dashboard and reverse proxy
    Serve {
        /// Port to listen on (used when TLS is disabled)
        #[arg(short, long, default_value = "8080")]
        port: u16,
        /// Domain for subdomain routing (e.g., example.com)
        #[arg(short, long, default_value = "localhost")]
        domain: String,
        /// Enable TLS with automatic Let's Encrypt certificates
        #[arg(long)]
        tls: bool,
        /// Email for Let's Encrypt registration (required with --tls)
        #[arg(long, requires = "tls")]
        email: Option<String>,
        /// Use Let's Encrypt staging environment (for testing, avoids rate limits)
        #[arg(long)]
        staging: bool,
    },
    /// Spawn a new process instance (e.g., ten spawn api:prod)
    Spawn {
        /// Instance identifier (process:id)
        instance: String,
    },
    /// Stop a running instance (e.g., ten stop api:prod)
    Stop {
        /// Instance identifier (process:id)
        instance: String,
    },
    /// Restart an instance (e.g., ten restart api:prod)
    Restart {
        /// Instance identifier (process:id)
        instance: String,
    },
    /// List running instances
    #[command(alias = "ls")]
    Ps,
    /// Check health of an instance (e.g., ten health api:prod)
    Health {
        /// Instance identifier (process:id)
        instance: String,
    },
    /// Set traffic weight for an instance (0-100)
    Weight {
        /// Instance identifier (process:id)
        instance: String,
        /// Traffic weight (0-100, default 100)
        weight: u8,
    },
    /// Deploy a new version and wait for it to be healthy
    Deploy {
        /// Instance identifier (process:version, e.g., api:v2)
        instance: String,
        /// Initial traffic weight (0-100, default 100)
        #[arg(long, short, default_value = "100")]
        weight: u8,
        /// Health check timeout in seconds (default 30)
        #[arg(long, default_value = "30")]
        timeout: u64,
    },
    /// Atomically swap traffic from one version to another (blue/green)
    Route {
        /// Process name (from tenement.toml)
        process: String,
        /// Source version (will be set to weight 0)
        #[arg(long)]
        from: String,
        /// Target version (will be set to weight 100)
        #[arg(long)]
        to: String,
    },
    /// Tail logs from running instances
    Logs {
        /// Instance identifier (process:id), e.g. api:prod. Omit for all instances.
        instance: Option<String>,
        /// Filter by log level (stdout or stderr)
        #[arg(long)]
        level: Option<String>,
        /// Search log messages
        #[arg(long)]
        search: Option<String>,
        /// Maximum number of entries (default 100)
        #[arg(long, default_value = "100")]
        limit: usize,
        /// Follow logs in real-time (stream new entries)
        #[arg(short, long)]
        follow: bool,
    },
    /// Initialize a new tenement project in the current directory
    Init {
        /// Service name (default: directory name)
        #[arg(long)]
        name: Option<String>,
        /// Command to run (auto-detected if possible)
        #[arg(long)]
        command: Option<String>,
    },
    /// Show config
    Config,
    /// Generate a new API token
    TokenGen,
    /// Install tenement as a systemd service
    Install {
        /// Domain for the service (e.g., example.com)
        #[arg(short, long)]
        domain: String,
        /// Port to listen on
        #[arg(short, long, default_value = "8080")]
        port: u16,
        /// Path to config file (default: current dir tenement.toml)
        #[arg(short, long)]
        config: Option<PathBuf>,
        /// Print generated files without installing
        #[arg(long)]
        dry_run: bool,
        /// Also install and configure Caddy as HTTPS reverse proxy (recommended for production)
        #[arg(long)]
        caddy: bool,
        /// DNS provider for wildcard certs (cloudflare, route53, digitalocean, etc.)
        /// Required for per-process wildcards like *.api.example.com
        #[arg(long)]
        dns_provider: Option<String>,
    },
    /// Uninstall tenement systemd service
    Uninstall,
    /// Generate Caddyfile for HTTPS reverse proxy
    Caddy {
        /// Domain for the service (e.g., example.com)
        #[arg(short, long)]
        domain: String,
        /// Port tenement is listening on
        #[arg(short, long, default_value = "8080")]
        port: u16,
        /// Output path for Caddyfile (default: stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Install Caddy via package manager
        #[arg(long)]
        install: bool,
        /// Enable Caddy as systemd service
        #[arg(long)]
        systemd: bool,
        /// DNS provider for wildcard certs (cloudflare, route53, digitalocean, etc.)
        /// Enables per-process wildcards like *.api.example.com
        #[arg(long)]
        dns_provider: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { port, domain, tls, email, staging } => {
            cmd_serve(port, domain, tls, email, staging).await?;
        }
        Commands::Spawn { instance } => {
            let (process, id) = parse_instance(&instance)?;
            let client = ApiClient::from_args(&cli.server, cli.token)?;
            let resp = client.spawn(&process, &id).await?;
            println!("Spawned {}", resp.instance);
            if let Some(port) = resp.port {
                println!("Listening on 127.0.0.1:{}", port);
            }
        }
        Commands::Stop { instance } => {
            let client = ApiClient::from_args(&cli.server, cli.token)?;
            client.stop(&instance).await?;
            println!("Stopped {}", instance);
        }
        Commands::Restart { instance } => {
            let client = ApiClient::from_args(&cli.server, cli.token)?;
            let resp = client.restart(&instance).await?;
            println!("Restarted {}", resp.instance);
        }
        Commands::Ps => {
            let client = ApiClient::from_args(&cli.server, cli.token)?;
            let instances = client.list().await?;
            if instances.is_empty() {
                println!("No running instances");
                println!("Server: {}", cli.server);
            } else {
                println!(
                    "{:<20} {:<20} {:<10} {:<10} {:<8} {:<6}",
                    "INSTANCE", "LISTEN", "UPTIME", "IDLE", "HEALTH", "WEIGHT"
                );
                for info in &instances {
                    let id = info["id"].as_str().unwrap_or("?");
                    let uptime = info["uptime_secs"].as_u64().unwrap_or(0);
                    let health = info["health"].as_str().unwrap_or("?");
                    let weight = info["weight"].as_u64().unwrap_or(0);
                    let idle = info["idle_secs"].as_u64().unwrap_or(0);
                    let listen = info["socket"].as_str().unwrap_or("?");

                    println!(
                        "{:<20} {:<20} {:<10} {:<10} {:<8} {:<6}",
                        id,
                        listen,
                        format_uptime(uptime),
                        format_uptime(idle),
                        health,
                        weight
                    );
                }
                println!();
                println!("{} instance(s) running on {}", instances.len(), cli.server);
            }
        }
        Commands::Health { instance } => {
            let client = ApiClient::from_args(&cli.server, cli.token)?;
            let resp = client.health(&instance).await?;
            let health = resp["health"].as_str().unwrap_or("unknown");
            println!("{}: {}", instance, health);
        }
        Commands::Weight { instance, weight } => {
            let client = ApiClient::from_args(&cli.server, cli.token)?;
            let resp = client.set_weight(&instance, weight).await?;
            println!("Set {} weight to {}", resp.instance, resp.weight);
        }
        Commands::Deploy { instance, weight, timeout } => {
            let (process, version) = parse_instance(&instance)?;
            let client = ApiClient::from_args(&cli.server, cli.token)?;
            println!("Deploying {}:{} with weight {}", process, version, weight);
            println!("Waiting for health check (timeout: {}s)...", timeout);

            let resp = client.deploy(&process, &version, weight, timeout).await?;

            println!("Deployed {}", resp.instance);
            println!("Weight: {}", resp.weight);
            println!("Status: {}", resp.status);
        }
        Commands::Route { process, from, to } => {
            let client = ApiClient::from_args(&cli.server, cli.token)?;
            let resp = client.route(&process, &from, &to).await?;

            println!("Routed traffic: {} -> {}", resp.from_instance, resp.to_instance);
            println!("  {} weight = {}", resp.from_instance, resp.from_weight);
            println!("  {} weight = {}", resp.to_instance, resp.to_weight);
        }
        Commands::Logs { instance, level, search, limit, follow } => {
            let client = ApiClient::from_args(&cli.server, cli.token)?;
            let (process, id) = match &instance {
                Some(inst) => {
                    let (p, i) = parse_instance(inst)?;
                    (Some(p), Some(i))
                }
                None => (None, None),
            };

            if follow {
                // Stream mode: connect to SSE endpoint
                client
                    .stream_logs(process.as_deref(), id.as_deref(), level.as_deref())
                    .await?;
            } else {
                // One-shot query
                let entries = client
                    .query_logs(process.as_deref(), id.as_deref(), level.as_deref(), search.as_deref(), limit)
                    .await?;

                for entry in entries {
                    let ts = entry["timestamp"].as_u64().unwrap_or(0);
                    let lvl = entry["level"].as_str().unwrap_or("?");
                    let proc = entry["process"].as_str().unwrap_or("?");
                    let inst = entry["instance_id"].as_str().unwrap_or("?");
                    let msg = entry["message"].as_str().unwrap_or("");

                    // Format timestamp as HH:MM:SS
                    let secs = ts / 1000;
                    let hours = (secs % 86400) / 3600;
                    let mins = (secs % 3600) / 60;
                    let s = secs % 60;

                    let level_marker = if lvl == "stderr" { "ERR" } else { "OUT" };
                    println!(
                        "{:02}:{:02}:{:02} [{}] {}:{} {}",
                        hours, mins, s, level_marker, proc, inst, msg
                    );
                }
            }
        }
        Commands::Init { name, command } => {
            cmd_init(name, command)?;
        }
        Commands::Config => {
            let config = Config::load()?;
            println!("Data dir: {:?}", config.settings.data_dir);
            println!("Health interval: {}s", config.settings.health_check_interval);
            println!("\nServices:");
            for (name, svc) in &config.service {
                println!("  [{}]", name);
                println!("    command: {}", svc.command);
                println!("    isolation: {}", svc.isolation);
                if let Some(health) = &svc.health {
                    println!("    health: {}", health);
                }
                if let Some(idle) = svc.idle_timeout {
                    println!("    idle_timeout: {}s", idle);
                }
            }
        }
        Commands::TokenGen => {
            let config = Config::load()?;
            let data_dir = PathBuf::from(&config.settings.data_dir);
            let db_path = data_dir.join("tenement.db");
            let pool = init_db(&db_path).await?;
            let config_store = ConfigStore::new(pool);
            let token_store = TokenStore::new(&config_store);

            let token = token_store.generate_and_store().await?;

            // Save plaintext token to file for CLI auto-read
            client::save_token_file(&data_dir, &token)?;

            println!("Generated new API token:");
            println!();
            println!("  {}", token);
            println!();
            println!("Token saved to {}/api_token", data_dir.display());
            println!("Use it in the Authorization header: Bearer {}", token);
        }
        Commands::Install { domain, port, config, dry_run, caddy: with_caddy, dns_provider } => {
            install::install(domain, port, config, dry_run, with_caddy, dns_provider)?;
        }
        Commands::Uninstall => {
            install::uninstall()?;
        }
        Commands::Caddy { domain, port, output, install: do_install, systemd, dns_provider } => {
            caddy::run(domain, port, output, do_install, systemd, dns_provider)?;
        }
    }

    Ok(())
}

/// Start the server (this is the only command that creates a Hypervisor directly)
async fn cmd_serve(
    port: u16,
    domain: String,
    tls: bool,
    email: Option<String>,
    staging: bool,
) -> Result<()> {
    let config = Config::load()?;
    let db_path = PathBuf::from(&config.settings.data_dir).join("tenement.db");
    let pool = init_db(&db_path).await?;
    let config_store = std::sync::Arc::new(ConfigStore::new(pool));

    let tls_options = if tls {
        let acme_email = email
            .or_else(|| config.settings.tls.acme_email.clone())
            .ok_or_else(|| anyhow::anyhow!(
                "TLS enabled but no email provided.\n\
                Use --email <your@email.com> for Let's Encrypt registration."
            ))?;

        validate_acme_email(&acme_email)?;

        if domain == "localhost" {
            anyhow::bail!(
                "TLS cannot be used with localhost.\n\
                Provide a real domain with --domain <your-domain.com>"
            );
        }

        let cache_dir = config.settings.tls.cache_dir
            .clone()
            .unwrap_or_else(|| config.settings.data_dir.join("acme"));

        Some(server::TlsOptions {
            enabled: true,
            email: acme_email,
            domain: domain.clone(),
            cache_dir,
            staging: staging || config.settings.tls.staging,
            https_port: config.settings.tls.https_port,
            http_port: config.settings.tls.http_port,
        })
    } else if config.settings.tls.enabled {
        let acme_email = config.settings.tls.acme_email.clone()
            .ok_or_else(|| anyhow::anyhow!(
                "TLS enabled in config but acme_email not set.\n\
                Add acme_email to [settings.tls] in tenement.toml"
            ))?;

        validate_acme_email(&acme_email)?;

        let tls_domain = config.settings.tls.domain.clone()
            .unwrap_or_else(|| domain.clone());

        if tls_domain == "localhost" {
            anyhow::bail!(
                "TLS cannot be used with localhost.\n\
                Set domain in [settings.tls] in tenement.toml"
            );
        }

        let cache_dir = config.settings.tls.cache_dir
            .clone()
            .unwrap_or_else(|| config.settings.data_dir.join("acme"));

        Some(server::TlsOptions {
            enabled: true,
            email: acme_email,
            domain: tls_domain,
            cache_dir,
            staging: staging || config.settings.tls.staging,
            https_port: config.settings.tls.https_port,
            http_port: config.settings.tls.http_port,
        })
    } else {
        None
    };

    if let Some(ref tls_opts) = tls_options {
        if tls_opts.http_port == tls_opts.https_port {
            anyhow::bail!(
                "HTTP port ({}) and HTTPS port ({}) cannot be the same.\n\
                The HTTP port is used for redirects to HTTPS.",
                tls_opts.http_port,
                tls_opts.https_port
            );
        }
    }

    let hypervisor = Hypervisor::new(config);
    server::serve(hypervisor, domain, port, config_store, tls_options).await?;
    Ok(())
}

/// Initialize a new tenement project
fn cmd_init(name: Option<String>, command: Option<String>) -> Result<()> {
    let config_path = std::path::Path::new("tenement.toml");
    if config_path.exists() {
        anyhow::bail!("tenement.toml already exists in this directory");
    }

    let dir_name = std::env::current_dir()?
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "app".to_string());

    let service_name = name.unwrap_or(dir_name);

    let detected_command = command.unwrap_or_else(|| detect_framework_command());

    let config = format!(
        r#"[service.{name}]
command = "{command}"
health = "/health"
idle_timeout = 300
storage_persist = true
isolation = "process"

[instances]
{name} = ["default"]
"#,
        name = service_name,
        command = detected_command,
    );

    std::fs::write(config_path, &config)?;
    println!("Created tenement.toml");
    println!();
    println!("  Service: {}", service_name);
    println!("  Command: {}", detected_command);
    println!();
    println!("Next steps:");
    println!("  1. ten token-gen    # Create API token");
    println!("  2. ten serve        # Start server");
    println!("  3. ten ps           # Check running instances");

    Ok(())
}

/// Detect the likely run command based on files in the current directory
fn detect_framework_command() -> String {
    let cwd = std::path::Path::new(".");

    // Python
    if cwd.join("pyproject.toml").exists() || cwd.join("requirements.txt").exists() {
        if cwd.join("app.py").exists() {
            return "uv run python app.py".to_string();
        }
        if cwd.join("main.py").exists() {
            return "uv run python main.py".to_string();
        }
        return "uv run python app.py".to_string();
    }

    // Node
    if cwd.join("package.json").exists() {
        return "npm start".to_string();
    }

    // Go
    if cwd.join("go.mod").exists() {
        return "go run .".to_string();
    }

    // Rust
    if cwd.join("Cargo.toml").exists() {
        return "cargo run --release".to_string();
    }

    "./app".to_string()
}

fn parse_instance(s: &str) -> Result<(String, String)> {
    let parts: Vec<&str> = s.splitn(2, ':').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        anyhow::bail!(
            "Invalid instance format '{}'. Use 'process:id' (e.g., api:prod)",
            s
        );
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
}

fn validate_acme_email(email: &str) -> Result<()> {
    if email.is_empty() {
        anyhow::bail!(
            "ACME email cannot be empty.\n\
            Provide a valid email for Let's Encrypt registration."
        );
    }
    if !email.contains('@') {
        anyhow::bail!(
            "Invalid email format: '{}'\n\
            Email must contain '@' for Let's Encrypt registration.",
            email
        );
    }
    let parts: Vec<&str> = email.split('@').collect();
    if parts.len() != 2 || parts[1].is_empty() || !parts[1].contains('.') {
        anyhow::bail!(
            "Invalid email format: '{}'\n\
            Email must have a valid domain (e.g., user@example.com).",
            email
        );
    }
    Ok(())
}

fn format_uptime(secs: u64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86400)
    }
}
