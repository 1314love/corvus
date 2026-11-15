//! corvus: command line interface for the Corvus daemon.
#![warn(missing_docs)]

use std::net::IpAddr;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand, ValueEnum};
use corvusd::home::HomeLayout;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Client {
    Claude,
    Cursor,
    Dsh,
    Cline,
    Generic,
}

impl Client {
    fn location_hint(self) -> Option<&'static str> {
        match self {
            Client::Claude => Some(
                "// Claude Code: add this entry to ~/.claude.json (user scope) or .mcp.json (project scope).",
            ),
            Client::Cursor => Some(
                "// Cursor: add this entry to ~/.cursor/mcp.json (global) or .cursor/mcp.json (project).",
            ),
            Client::Dsh => Some(
                "// DSH: add this mcpServers entry to the DSH MCP server settings.",
            ),
            Client::Cline => Some(
                "// Cline: add this entry to cline_mcp_settings.json or use the MCP Servers panel.",
            ),
            Client::Generic => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Transport {
    Stdio,
    Http,
}

/// Build the mcpServers JSON document for one transport.
fn config_snippet(transport: Transport, command: &str, home: Option<&Path>) -> String {
    let entry = match transport {
        Transport::Stdio => {
            let mut args = vec!["serve".to_string(), "--stdio".to_string()];
            if let Some(home) = home.filter(|path| !path.as_os_str().is_empty()) {
                args.push("--home".to_string());
                args.push(home.display().to_string());
            }
            serde_json::json!({
                "command": command,
                "args": args
            })
        }
        Transport::Http => serde_json::json!({
            "url": "http://127.0.0.1:7331/mcp"
        }),
    };
    let document = serde_json::json!({ "mcpServers": { "corvus": entry } });
    serde_json::to_string_pretty(&document).expect("the config snippet serializes")
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
        #[arg(long)]
        stdio: bool,
    },
    /// Print a client configuration snippet (JSON on stdout, location hint on stderr)
    Config {
        /// Target client; every format uses the mcpServers schema
        #[arg(long, value_enum, default_value_t = Client::Generic)]
        client: Client,
        /// MCP transport
        #[arg(long, value_enum, default_value_t = Transport::Stdio)]
        transport: Transport,
        /// Positional transport kept for compatibility
        #[arg(value_enum, hide = true)]
        legacy_transport: Option<Transport>,
        /// Corvus home directory to pin in the snippet
        #[arg(long)]
        home: Option<PathBuf>,
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
        Command::Serve {
            bind,
            ws_port,
            http_port,
            db,
            home,
            cdp_endpoint,
            no_cdp,
            channel,
            stdio,
        } => {
            let channel = corvusd::channel::ChannelMode::parse(&channel)
                .ok_or_else(|| anyhow::anyhow!("unknown channel {channel}"))?;
            let layout = HomeLayout::resolve(home.clone())?;
            let _logging = corvusd::logging::init(Some(layout.log_path()), layout.root());
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
            eprintln!(
                "corvus serving websocket on {} and MCP on http://{}/mcp",
                daemon.ws_addr(),
                daemon.http_addr()
            );
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
        Command::Config {
            client,
            transport,
            legacy_transport,
            home,
        } => {
            let command = std::env::current_exe()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|_| "corvus".to_string());
            let transport = legacy_transport.unwrap_or(transport);
            if let Some(hint) = client.location_hint() {
                eprintln!("{hint}");
            }
            println!("{}", config_snippet(transport, &command, home.as_deref()));
        }
        Command::Version => println!(
            "corvus {} (protocol {})",
            env!("CARGO_PKG_VERSION"),
            corvus_proto::PROTOCOL_VERSION
        ),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const CLIENTS: [(&str, Client); 5] = [
        ("claude", Client::Claude),
        ("cursor", Client::Cursor),
        ("dsh", Client::Dsh),
        ("cline", Client::Cline),
        ("generic", Client::Generic),
    ];

    const TRANSPORTS: [(&str, Transport); 2] =
        [("stdio", Transport::Stdio), ("http", Transport::Http)];

    fn object_keys(value: &Value) -> Vec<&str> {
        let mut keys: Vec<&str> = value
            .as_object()
            .expect("value is a JSON object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        keys
    }

    fn snippet_value(transport: Transport) -> Value {
        let snippet = config_snippet(transport, "/opt/corvus", None);
        serde_json::from_str(&snippet).expect("stdout is a single JSON document")
    }

    #[test]
    fn config_parses_every_client_and_transport() {
        for (client_name, expected_client) in CLIENTS {
            for (transport_name, expected_transport) in TRANSPORTS {
                let cli = Cli::try_parse_from([
                    "corvus",
                    "config",
                    "--client",
                    client_name,
                    "--transport",
                    transport_name,
                ])
                .expect("flags parse");
                match cli.command {
                    Command::Config {
                        client,
                        transport,
                        legacy_transport,
                        home,
                    } => {
                        assert_eq!(client, expected_client);
                        assert_eq!(transport, expected_transport);
                        assert_eq!(legacy_transport, None);
                        assert_eq!(home, None);
                    }
                    _ => panic!("expected the config subcommand"),
                }
            }
        }
    }

    #[test]
    fn config_defaults_are_generic_and_stdio() {
        let cli = Cli::try_parse_from(["corvus", "config"]).expect("defaults parse");
        match cli.command {
            Command::Config {
                client,
                transport,
                legacy_transport,
                home,
            } => {
                assert_eq!(client, Client::Generic);
                assert_eq!(transport, Transport::Stdio);
                assert_eq!(legacy_transport, None);
                assert_eq!(home, None);
            }
            _ => panic!("expected the config subcommand"),
        }
    }

    #[test]
    fn config_accepts_a_legacy_positional_transport() {
        let cli = Cli::try_parse_from(["corvus", "config", "http"]).expect("positional parses");
        match cli.command {
            Command::Config {
                client,
                transport,
                legacy_transport,
                home,
            } => {
                assert_eq!(client, Client::Generic);
                assert_eq!(transport, Transport::Stdio);
                assert_eq!(legacy_transport, Some(Transport::Http));
                assert_eq!(home, None);
            }
            _ => panic!("expected the config subcommand"),
        }
    }

    #[test]
    fn snippets_have_exact_keys_for_every_transport() {
        for (_, transport) in TRANSPORTS {
            let value = snippet_value(transport);
            assert_eq!(object_keys(&value), vec!["mcpServers"]);
            let servers = &value["mcpServers"];
            assert_eq!(object_keys(servers), vec!["corvus"]);
            let entry = &servers["corvus"];
            match transport {
                Transport::Stdio => {
                    assert_eq!(object_keys(entry), vec!["args", "command"]);
                    assert_eq!(entry["command"], "/opt/corvus");
                    assert_eq!(entry["args"], serde_json::json!(["serve", "--stdio"]));
                }
                Transport::Http => {
                    assert_eq!(object_keys(entry), vec!["url"]);
                    assert_eq!(entry["url"], "http://127.0.0.1:7331/mcp");
                }
            }
        }
    }

    #[test]
    fn stdio_snippet_pins_the_supplied_home_and_http_stays_unchanged() {
        let home = std::env::temp_dir().join("corvus-home-snippet");
        let snippet = config_snippet(Transport::Stdio, "/opt/corvus", Some(home.as_path()));
        let value: Value =
            serde_json::from_str(&snippet).expect("stdout is a single JSON document");
        assert_eq!(
            value["mcpServers"]["corvus"]["args"],
            serde_json::json!(["serve", "--stdio", "--home", home.display().to_string()])
        );
        let http = config_snippet(Transport::Http, "/opt/corvus", Some(home.as_path()));
        let value: Value = serde_json::from_str(&http).expect("stdout is a single JSON document");
        assert_eq!(object_keys(&value["mcpServers"]["corvus"]), vec!["url"]);
        assert_eq!(
            value["mcpServers"]["corvus"]["url"],
            "http://127.0.0.1:7331/mcp"
        );
    }

    #[test]
    fn only_named_clients_have_a_location_hint() {
        for (_, client) in CLIENTS {
            match client.location_hint() {
                Some(hint) => assert!(hint.starts_with("// ")),
                None => assert_eq!(client, Client::Generic),
            }
        }
        assert_eq!(Client::Generic.location_hint(), None);
    }
}
