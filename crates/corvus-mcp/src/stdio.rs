//! Standard input and output transport.
//!
//! One JSON-RPC message per line. Nothing except protocol messages may be
//! written to standard output, and logging therefore always goes to standard
//! error. Requests run concurrently so a cancellation notification can reach
//! the server while the request it names is still executing; answers are
//! serialized on standard output.

use std::io;
use std::sync::Arc;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;
use tokio::task::JoinSet;

use crate::protocol::McpServer;

/// Serve messages until standard input ends.
pub async fn serve_stdio(server: Arc<McpServer>) -> io::Result<()> {
    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();
    let stdout = Arc::new(Mutex::new(tokio::io::stdout()));
    let mut tasks: JoinSet<io::Result<()>> = JoinSet::new();
    tracing::info!("mcp stdio transport ready");
    let session = server.start_session("stdio", None);
    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if is_notification(line) {
            let _ = server.handle_message_with(line, session.as_ref()).await;
            continue;
        }
        let line = line.to_string();
        let server = Arc::clone(&server);
        let stdout = Arc::clone(&stdout);
        let session = session.clone();
        tasks.spawn(async move {
            if let Some(response) = server.handle_message_with(&line, session.as_ref()).await {
                let mut output = stdout.lock().await;
                output.write_all(response.as_bytes()).await?;
                output.write_all(b"\n").await?;
                output.flush().await?;
            }
            Ok(())
        });
    }
    let mut failure = None;
    while let Some(joined) = tasks.join_next().await {
        match joined {
            Ok(Ok(())) => {}
            Ok(Err(error)) => failure = Some(error),
            Err(error) => failure = Some(io::Error::other(error)),
        }
    }
    if let Some(session) = &session {
        server.finish_session(&session.session_id, "closed");
    }
    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn is_notification(line: &str) -> bool {
    serde_json::from_str::<Value>(line)
        .ok()
        .and_then(|value| value.as_object().map(|object| !object.contains_key("id")))
        .unwrap_or(false)
}
