//! Standard input and output transport.
//!
//! One JSON-RPC message per line. Nothing except protocol messages may be
//! written to standard output, and logging therefore always goes to standard
//! error.

use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::protocol::McpServer;

/// Serve messages until standard input ends.
pub async fn serve_stdio(server: Arc<McpServer>) -> std::io::Result<()> {
    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();
    let mut stdout = tokio::io::stdout();
    tracing::info!("mcp stdio transport ready");
    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(response) = server.handle_message(line).await {
            stdout.write_all(response.as_bytes()).await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
        }
    }
    Ok(())
}
