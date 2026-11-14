//! corvusd: the Corvus daemon binary.

use std::net::IpAddr;
use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

use corvusd::{Daemon, DaemonConfig};

#[derive(Parser)]
#[command(
    name = "corvusd",
    version,
    about = "Corvus daemon: browser hub for AI agents"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the WebSocket transport and the MCP server in one runtime
    Serve {
        /// Address to bind both listeners to
        #[arg(long, default_value_t = corvusd::DEFAULT_BIND)]
        bind: IpAddr,
        /// WebSocket port used by the browser extension
        #[arg(long, default_value_t = corvusd::DEFAULT_WS_PORT)]
        ws_port: u16,
        /// MCP streamable HTTP port
        #[arg(long, default_value_t = corvusd::DEFAULT_HTTP_PORT)]
        http_port: u16,
        /// Recording database path
        #[arg(long)]
        db: Option<PathBuf>,
        /// Also serve MCP over standard input and output
        #[arg(long)]
        stdio: bool,
    },
    /// Print version, ports and connected browsers
    Doctor {
        /// Address of the daemon to inspect
        #[arg(long, default_value_t = corvusd::DEFAULT_BIND)]
        bind: IpAddr,
        /// WebSocket port to probe
        #[arg(long, default_value_t = corvusd::DEFAULT_WS_PORT)]
        ws_port: u16,
        /// MCP streamable HTTP port to probe
        #[arg(long, default_value_t = corvusd::DEFAULT_HTTP_PORT)]
        http_port: u16,
        /// Recording database path to inspect
        #[arg(long)]
        db: Option<PathBuf>,
    },
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
async fn main() -> Result<()> {
    init_tracing();
    let cli = Cli::parse();
    match cli.command {
        Command::Serve {
            bind,
            ws_port,
            http_port,
            db,
            stdio,
        } => {
            let daemon = Daemon::start(DaemonConfig {
                bind,
                ws_port,
                http_port,
                db_path: db,
                ..DaemonConfig::default()
            })
            .await?;
            tracing::info!(
                ws = %daemon.ws_addr(),
                mcp = %format!("http://{}/mcp", daemon.http_addr()),
                "corvusd ready"
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
        Command::Doctor {
            bind,
            ws_port,
            http_port,
            db,
        } => {
            let report = corvusd::doctor::inspect_with_db(bind, ws_port, http_port, db).await;
            print!("{}", corvusd::doctor::render(&report));
        }
    }
    Ok(())
}
