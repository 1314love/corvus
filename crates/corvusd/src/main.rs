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
        /// Authentication mode: off or local
        #[arg(long, env = "CORVUS_AUTH", default_value = "off", value_parser = ["off", "local"])]
        auth: String,
        /// HS256 signing secret, required when auth is local (32+ bytes)
        #[arg(long, env = "CORVUS_AUTH_SECRET", hide_env_values = true)]
        auth_secret: Option<String>,
        /// Bootstrap administrator username
        #[arg(long, env = "CORVUS_AUTH_USERNAME")]
        auth_username: Option<String>,
        /// Bootstrap administrator password
        #[arg(long, env = "CORVUS_AUTH_PASSWORD", hide_env_values = true)]
        auth_password: Option<String>,
        /// Allow a non-loopback bind without authentication (unsafe)
        #[arg(
            long = "allow-no-auth",
            env = "CORVUS_ALLOW_NO_AUTH",
            action = clap::ArgAction::Set,
            num_args = 0..=1,
            default_missing_value = "true",
            default_value_t = false,
            value_parser = parse_allow_no_auth
        )]
        allow_no_auth: bool,
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
            auth,
            auth_secret,
            auth_username,
            auth_password,
            allow_no_auth,
            stdio,
        } => {
            let channel = corvusd::channel::ChannelMode::parse(&channel)
                .ok_or_else(|| anyhow::anyhow!("unknown channel {channel}"))?;
            let auth = match auth.as_str() {
                "off" => corvusd::AuthSettings::default(),
                "local" => {
                    let secret = auth_secret.ok_or_else(|| {
                        anyhow::anyhow!("--auth-secret is required when auth is local")
                    })?;
                    corvusd::AuthSettings {
                        config: corvusd::AuthConfig::local(secret),
                        username: auth_username,
                        password: auth_password,
                    }
                }
                other => anyhow::bail!("unknown auth mode {other}"),
            };
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
                allow_no_auth,
                auth,
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

fn parse_allow_no_auth(value: &str) -> Result<bool, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" | "y" => Ok(true),
        "0" | "false" | "no" | "off" | "n" | "" => Ok(false),
        other => Err(format!("expected a boolean value, got {other}")),
    }
}

#[cfg(test)]
mod cli_tests {
    use super::parse_allow_no_auth;

    #[test]
    fn allow_no_auth_accepts_truthy_and_falsy_values() {
        for value in ["1", "true", "TRUE", "yes", "on", "y"] {
            assert!(parse_allow_no_auth(value).expect(value));
        }
        for value in ["0", "false", "FALSE", "no", "off", "n", ""] {
            assert!(!parse_allow_no_auth(value).expect(value));
        }
        assert!(parse_allow_no_auth("maybe").is_err());
    }
}
