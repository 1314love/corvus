//! corvus: command line interface for the Corvus daemon.

use clap::{Parser, Subcommand};

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
    /// Print environment, ports, extension status
    Doctor,
    /// Run the daemon in the foreground
    Serve,
    /// Print client configuration snippets
    Config {
        /// one of: claude, cursor, dsh, cline, copilot
        client: String,
    },
    /// Show daemon and protocol version
    Version,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Doctor => println!("corvus doctor: not implemented in M0"),
        Command::Serve => println!("corvus serve: start corvusd instead (not implemented in M0)"),
        Command::Config { client } => println!("corvus config {client}: not implemented in M0"),
        Command::Version => println!(
            "corvus {} (protocol {})",
            env!("CARGO_PKG_VERSION"),
            corvus_proto::PROTOCOL_VERSION
        ),
    }
    Ok(())
}
