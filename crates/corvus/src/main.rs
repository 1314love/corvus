//! corvus: command line interface for the Corvus daemon.

use std::net::IpAddr;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use corvusd::{Daemon, DaemonConfig};
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "corvus",
    version,
    about = "Corvus - the browser hub built for AI agents"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print environment, ports and extension status
    Doctor {
        #[arg(long, default_value_t = corvusd::DEFAULT_BIND)]
        bind: IpAddr,
        #[arg(long, default_value_t = corvusd::DEFAULT_WS_PORT)]
        ws_port: u16,
        #[arg(long, default_value_t = corvusd::DEFAULT_HTTP_PORT)]
        http_port: u16,
        /// Recording database path to inspect
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// Run the daemon in the foreground
    Serve {
        #[arg(long, default_value_t = corvusd::DEFAULT_BIND)]
        bind: IpAddr,
        #[arg(long, default_value_t = corvusd::DEFAULT_WS_PORT)]
        ws_port: u16,
        #[arg(long, default_value_t = corvusd::DEFAULT_HTTP_PORT)]
        http_port: u16,
        /// Recording database path
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        stdio: bool,
    },
    /// Print a client configuration snippet
    Config {
        /// One of: stdio, http
        #[arg(default_value = "stdio")]
        transport: String,
    },
    /// Show daemon and protocol version
    Version,
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(corvusd::logs::global().clone())
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Doctor {
            bind,
            ws_port,
            http_port,
            db,
        } => {
            let report = corvusd::doctor::inspect_with_db(bind, ws_port, http_port, db).await;
            print!("{}", corvusd::doctor::render(&report));
        }
        Command::Serve {
            bind,
            ws_port,
            http_port,
            db,
            stdio,
        } => {
            init_tracing();
            let daemon = Daemon::start(DaemonConfig {
                bind,
                ws_port,
                http_port,
                db_path: db,
                ..DaemonConfig::default()
            })
            .await?;
            println!(
                "corvus serving websocket on {} and MCP on http://{}/mcp",
                daemon.ws_addr(),
                daemon.http_addr()
            );
            if stdio {
                tokio::select! {
                    result = corvus_mcp::serve_stdio(std::sync::Arc::clone(daemon.mcp())) => {
                        result?;
                    }
                    _ = tokio::signal::ctrl_c() => {}
                }
            } else {
                tokio::signal::ctrl_c().await?;
            }
            daemon.shutdown().await;
        }
        Command::Config { transport } => {
            let command = std::env::current_exe()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|_| "corvus".to_string());
            if transport == "stdio" {
                println!(
                    "{{\"mcpServers\":{{\"corvus\":{{\"command\":\"{command}\",\"args\":[\"serve\",\"--stdio\"]}}}}}}"
                );
            } else {
                println!(r#"{{"mcpServers":{{"corvus":{{"url":"http://127.0.0.1:7331/mcp"}}}}}}"#);
            }
        }
        Command::Version => println!(
            "corvus {} (protocol {})",
            env!("CARGO_PKG_VERSION"),
            corvus_proto::PROTOCOL_VERSION
        ),
    }
    Ok(())
}
