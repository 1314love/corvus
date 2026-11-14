//! corvus: command line interface for the Corvus daemon.

use std::net::IpAddr;

use clap::{Parser, Subcommand};
use corvusd::{Daemon, DaemonConfig};

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
    },
    /// Run the daemon in the foreground
    Serve {
        #[arg(long, default_value_t = corvusd::DEFAULT_BIND)]
        bind: IpAddr,
        #[arg(long, default_value_t = corvusd::DEFAULT_WS_PORT)]
        ws_port: u16,
        #[arg(long, default_value_t = corvusd::DEFAULT_HTTP_PORT)]
        http_port: u16,
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Doctor {
            bind,
            ws_port,
            http_port,
        } => {
            let report = corvusd::doctor::inspect(bind, ws_port, http_port).await;
            print!("{}", corvusd::doctor::render(&report));
        }
        Command::Serve {
            bind,
            ws_port,
            http_port,
            stdio,
        } => {
            let daemon = Daemon::start(DaemonConfig {
                bind,
                ws_port,
                http_port,
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
