//! corvusd: the Corvus daemon.
//!
//! M0 skeleton. Wires configuration and logging only; the MCP server,
//! transport hub and recorder land in M1.

use anyhow::Result;
use tracing_subscriber::EnvFilter;

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        protocol = corvus_proto::PROTOCOL_VERSION,
        "corvusd starting"
    );
    tracing::warn!("M0 skeleton: MCP server and transport hub are not wired yet");
    Ok(())
}
