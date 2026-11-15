//! corvusd: the Corvus daemon binary.

use std::net::IpAddr;
use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use corvusd::home::HomeLayout;
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
        /// Corvus home directory
        #[arg(long)]
        home: Option<PathBuf>,
        /// Log file path override
        #[arg(long)]
        log_file: Option<PathBuf>,
        /// Disable the log file and keep logging on stderr
        #[arg(long, conflicts_with = "log_file")]
        no_log_file: bool,
        /// Chrome DevTools Protocol endpoint; discovered from the extension when omitted
        #[arg(long, env = "CORVUS_CDP_ENDPOINT")]
        cdp_endpoint: Option<String>,
        /// Disable CDP discovery and attachment
        #[arg(long)]
        no_cdp: bool,
        /// Browser channel: auto, cdp or extension
        #[arg(long, env = "CORVUS_CHANNEL", default_value = "auto", value_parser = ["auto", "cdp", "extension"])]
        channel: String,
        /// Also serve MCP over standard input and output
        #[arg(long)]
        stdio: bool,
    },
    /// Print version, ports, recording state and connected browsers
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
        /// Corvus home directory
        #[arg(long)]
        home: Option<PathBuf>,
        /// Chrome DevTools Protocol endpoint; discovered from the extension when omitted
        #[arg(long, env = "CORVUS_CDP_ENDPOINT")]
        cdp_endpoint: Option<String>,
        /// Disable CDP discovery and attachment
        #[arg(long)]
        no_cdp: bool,
        /// Browser channel: auto, cdp or extension
        #[arg(long, env = "CORVUS_CHANNEL", default_value = "auto", value_parser = ["auto", "cdp", "extension"])]
        channel: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Serve {
            bind,
            ws_port,
            http_port,
            db,
            home,
            cdp_endpoint,
            no_cdp,
            channel,
            log_file,
            no_log_file,
            stdio,
        } => {
            let channel = corvusd::channel::ChannelMode::parse(&channel)
                .ok_or_else(|| anyhow::anyhow!("unknown channel {channel}"))?;
            let layout = HomeLayout::resolve(home.clone())?;
            let log_target = if no_log_file {
                None
            } else {
                log_file.or_else(|| Some(layout.log_path()))
            };
            let _logging = corvusd::logging::init(log_target, layout.root());
            let daemon = Daemon::start(DaemonConfig {
                bind,
                ws_port,
                http_port,
                db_path: db,
                home,
                channel,
                cdp_endpoint,
                no_cdp,
                ..DaemonConfig::default()
            })
            .await?;
            if stdio {
                tokio::select! {
                    result = corvus_mcp::serve_stdio(std::sync::Arc::clone(daemon.mcp())) => {
                        result?;
                    }
                    result = corvusd::signals::shutdown() => {
                        result?;
                    }
                }
            } else {
                corvusd::signals::shutdown().await?;
            }
            daemon.shutdown().await;
        }
        Command::Doctor {
            bind,
            ws_port,
            http_port,
            db,
            home,
            cdp_endpoint: _,
            no_cdp: _,
            channel: _,
        } => {
            let report =
                corvusd::doctor::inspect_with_home(bind, ws_port, http_port, db, home).await;
            print!("{}", corvusd::doctor::render(&report));
        }
    }
    Ok(())
}
