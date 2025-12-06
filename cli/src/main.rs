use anyhow::Result;
use clap::{Parser, Subcommand};
use tenement::{Config, Hypervisor};

#[derive(Parser)]
#[command(name = "tenement")]
#[command(author, version, about = "Hyperlightweight process hypervisor")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
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
    /// Show config
    Config,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
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
                    "{:<20} {:<30} {:<10} {:<10}",
                    "INSTANCE", "SOCKET", "UPTIME", "HEALTH"
                );
                for info in instances {
                    println!(
                        "{:<20} {:<30} {:<10} {:<10}",
                        info.id.to_string(),
                        info.socket.display(),
                        format_uptime(info.uptime_secs),
                        info.health.to_string()
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
        Commands::Config => {
            let config = Config::load()?;
            println!("Data dir: {:?}", config.settings.data_dir);
            println!("Health interval: {}s", config.settings.health_check_interval);
            println!("\nProcesses:");
            for (name, process) in &config.process {
                println!("  [{}]", name);
                println!("    command: {}", process.command);
                println!("    socket: {}", process.socket);
                if let Some(health) = &process.health {
                    println!("    health: {}", health);
                }
            }
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
