use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tenement::{init_db, Config, ConfigStore, Hypervisor, TokenStore};

use tenement_cli::server;

mod install;
mod caddy;

#[derive(Parser)]
#[command(name = "tenement")]
#[command(author, version, about = "Hyperlightweight process hypervisor")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the HTTP server with dashboard and reverse proxy
    Serve {
        /// Port to listen on
        #[arg(short, long, default_value = "8080")]
        port: u16,
        /// Domain for subdomain routing (e.g., example.com)
        #[arg(short, long, default_value = "localhost")]
        domain: String,
    },
    /// Spawn a new process instance
    Spawn {
        /// Process name (from tenement.toml)
        process: String,
        /// Instance ID
        #[arg(long, short)]
        id: String,
    },
    /// Stop a running instance
    Stop {
        /// Instance identifier (process:id)
        instance: String,
    },
    /// Restart an instance
    Restart {
        /// Instance identifier (process:id)
        instance: String,
    },
    /// List running instances
    #[command(alias = "ls")]
    Ps,
    /// Check health of an instance
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
        /// Process name (from tenement.toml)
        process: String,
        /// Version to deploy (becomes instance ID, e.g., "v2")
        #[arg(long, short)]
        version: String,
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
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { port, domain } => {
            let config = Config::load()?;
            let db_path = PathBuf::from(&config.settings.data_dir).join("tenement.db");
            let pool = init_db(&db_path).await?;
            let config_store = std::sync::Arc::new(ConfigStore::new(pool));

            let hypervisor = Hypervisor::new(config);
            server::serve(hypervisor, domain, port, config_store).await?;
        }
        Commands::Spawn { process, id } => {
            let hypervisor = Hypervisor::from_config_file()?;
            let socket = hypervisor.spawn(&process, &id).await?;
            println!("Spawned {}:{}", process, id);
            println!("Socket: {}", socket.display());
        }
        Commands::Stop { instance } => {
            let (process, id) = parse_instance(&instance)?;
            let hypervisor = Hypervisor::from_config_file()?;
            hypervisor.stop(&process, &id).await?;
            println!("Stopped {}", instance);
        }
        Commands::Restart { instance } => {
            let (process, id) = parse_instance(&instance)?;
            let hypervisor = Hypervisor::from_config_file()?;
            let socket = hypervisor.restart(&process, &id).await?;
            println!("Restarted {}", instance);
            println!("Socket: {}", socket.display());
        }
        Commands::Ps => {
            let hypervisor = Hypervisor::from_config_file()?;
            let instances = hypervisor.list().await;
            if instances.is_empty() {
                println!("No running instances");
            } else {
                println!(
                    "{:<20} {:<30} {:<10} {:<10} {:<6}",
                    "INSTANCE", "SOCKET", "UPTIME", "HEALTH", "WEIGHT"
                );
                for info in instances {
                    println!(
                        "{:<20} {:<30} {:<10} {:<10} {:<6}",
                        info.id.to_string(),
                        info.socket.display(),
                        format_uptime(info.uptime_secs),
                        info.health.to_string(),
                        info.weight
                    );
                }
            }
        }
        Commands::Health { instance } => {
            let (process, id) = parse_instance(&instance)?;
            let hypervisor = Hypervisor::from_config_file()?;
            let status = hypervisor.check_health(&process, &id).await;
            println!("{}: {}", instance, status);
        }
        Commands::Weight { instance, weight } => {
            let (process, id) = parse_instance(&instance)?;
            let hypervisor = Hypervisor::from_config_file()?;
            // Need to spawn first if not running
            if !hypervisor.is_running(&process, &id).await {
                anyhow::bail!("Instance {} is not running", instance);
            }
            hypervisor.set_weight(&process, &id, weight).await?;
            println!("Set {} weight to {}", instance, weight);
        }
        Commands::Deploy { process, version, weight, timeout } => {
            let hypervisor = Hypervisor::from_config_file()?;
            println!("Deploying {}:{} with weight {}", process, version, weight);
            println!("Waiting for health check (timeout: {}s)...", timeout);

            let socket = hypervisor.deploy_and_wait_healthy(&process, &version, weight, timeout).await?;

            println!("Deployed {}:{}", process, version);
            println!("Socket: {}", socket.display());
            println!("Weight: {}", weight);
            println!("Status: healthy");
        }
        Commands::Route { process, from, to } => {
            let hypervisor = Hypervisor::from_config_file()?;

            hypervisor.route_swap(&process, &from, &to).await?;

            println!("Routed traffic: {}:{} -> {}:{}", process, from, process, to);
            println!("  {}:{} weight = 0", process, from);
            println!("  {}:{} weight = 100", process, to);
        }
        Commands::Config => {
            let config = Config::load()?;
            println!("Data dir: {:?}", config.settings.data_dir);
            println!("Health interval: {}s", config.settings.health_check_interval);
            println!("\nServices:");
            for (name, svc) in &config.service {
                println!("  [{}]", name);
                println!("    command: {}", svc.command);
                println!("    socket: {}", svc.socket);
                println!("    isolation: {}", svc.isolation);
                if let Some(health) = &svc.health {
                    println!("    health: {}", health);
                }
            }
        }
        Commands::TokenGen => {
            let config = Config::load()?;
            let db_path = PathBuf::from(&config.settings.data_dir).join("tenement.db");
            let pool = init_db(&db_path).await?;
            let config_store = ConfigStore::new(pool);
            let token_store = TokenStore::new(&config_store);

            let token = token_store.generate_and_store().await?;
            println!("Generated new API token:");
            println!();
            println!("  {}", token);
            println!();
            println!("Store this token securely - it cannot be recovered.");
            println!("Use it in the Authorization header: Bearer {}", token);
        }
        Commands::Install { domain, port, config, dry_run } => {
            install::install(domain, port, config, dry_run)?;
        }
        Commands::Uninstall => {
            install::uninstall()?;
        }
        Commands::Caddy { domain, port, output, install: do_install, systemd } => {
            caddy::run(domain, port, output, do_install, systemd)?;
        }
    }

    Ok(())
}

fn parse_instance(s: &str) -> Result<(String, String)> {
    let parts: Vec<&str> = s.splitn(2, ':').collect();
    if parts.len() != 2 {
        anyhow::bail!("Invalid instance format. Use 'process:id'");
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
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
