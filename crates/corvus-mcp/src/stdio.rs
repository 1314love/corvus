//! Standard input and output transport.
//!
//! One JSON-RPC message per line. Nothing except protocol messages may be
//! written to standard output, and logging therefore always goes to standard
//! error. Requests run concurrently so a cancellation notification can reach
//! the server while the request it names is still executing; answers are
//! serialized on standard output.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;
use tokio::task::JoinSet;

use crate::protocol::McpServer;

/// Serve messages until standard input ends.
pub async fn serve_stdio(server: Arc<McpServer>) -> io::Result<()> {
    serve_stdio_on(server, tokio::io::stdin(), tokio::io::stdout()).await
}

/// Serve the same protocol over arbitrary streams. Tests use this seam to
/// drive the transport without touching the process standard streams.
pub async fn serve_stdio_on<R, W>(server: Arc<McpServer>, input: R, output: W) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let mut lines = BufReader::new(input).lines();
    let stdout = Arc::new(Mutex::new(output));
    let mut tasks: JoinSet<io::Result<()>> = JoinSet::new();
    tracing::info!("mcp stdio transport ready");
    let session = server.start_session("stdio", None);
    let mut signature = server.surface_signature();
    let mut surface_tick = tokio::time::interval(Duration::from_millis(200));
    surface_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        let line = tokio::select! {
            line = lines.next_line() => line?,
            _ = surface_tick.tick() => {
                let next = server.surface_signature();
                if next != signature {
                    signature = next;
                    if signature.is_some() {
                        write_line(
                            &stdout,
                            &json!({
                                "jsonrpc": "2.0",
                                "method": "notifications/tools/list_changed",
                            })
                            .to_string(),
                        )
                        .await?;
                    }
                }
                continue;
            }
        };
        let Some(line) = line else {
            break;
        };
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
                write_line(&stdout, &response).await?;
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

async fn write_line<W>(stdout: &Arc<Mutex<W>>, line: &str) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let mut output = stdout.lock().await;
    output.write_all(line.as_bytes()).await?;
    output.write_all(b"\n").await?;
    output.flush().await
}

fn is_notification(line: &str) -> bool {
    serde_json::from_str::<Value>(line)
        .ok()
        .and_then(|value| value.as_object().map(|object| !object.contains_key("id")))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::backend::{BackendFuture, ToolBackend, ToolCall};
    use crate::surface::ToolSurface;
    use corvus_application::ports::SessionRecording;
    use corvus_application::StepRecord;
    use corvus_domain::recording::SessionRow;
    use corvus_proto::{ErrorBody, ErrorCode};
    use serde_json::json;

    #[derive(Default)]
    struct Store {
        sessions: std::sync::Mutex<std::collections::HashSet<String>>,
    }

    impl SessionRecording for Store {
        fn start_session(
            &self,
            id: &str,
            _transport: &str,
            _agent: Option<&str>,
            _browser: Option<&str>,
        ) -> Result<(), String> {
            self.sessions
                .lock()
                .expect("sessions")
                .insert(id.to_string());
            Ok(())
        }

        fn session_exists(&self, id: &str) -> Result<bool, String> {
            Ok(self.sessions.lock().expect("sessions").contains(id))
        }

        fn finish_session(&self, _id: &str, _status: &str) -> Result<(), String> {
            Ok(())
        }

        fn list_sessions(
            &self,
            _limit: u32,
            _since_ms: Option<i64>,
        ) -> Result<Vec<SessionRow>, String> {
            Ok(Vec::new())
        }

        fn record_step(&self, _step: StepRecord) -> Result<(), String> {
            Ok(())
        }
    }

    struct NoBackend;

    impl ToolBackend for NoBackend {
        fn call(&self, _call: ToolCall) -> BackendFuture {
            Box::pin(async { Ok(json!({})) })
        }
    }

    struct DynamicSurface {
        revision: Arc<AtomicU64>,
    }

    impl ToolSurface for DynamicSurface {
        fn instructions(&self) -> &'static str {
            "surface test"
        }

        fn tools(&self) -> Vec<Value> {
            let mut names = vec!["browser_status"];
            if self.revision.load(Ordering::SeqCst) > 0 {
                names.push("tab_list");
            }
            names
                .into_iter()
                .map(|name| {
                    json!({
                        "name": name,
                        "description": "test tool",
                        "inputSchema": { "type": "object", "properties": {} },
                    })
                })
                .collect()
        }

        fn validate(&self, name: &str, arguments: &Value) -> Result<Value, ErrorBody> {
            match name {
                "browser_status" | "tab_list" => Ok(arguments.clone()),
                _ => Err(ErrorBody::new(ErrorCode::BadArgs, "unknown tool")),
            }
        }

        fn signature(&self) -> String {
            format!("v{}", self.revision.load(Ordering::SeqCst))
        }
    }

    #[tokio::test]
    async fn stdio_advertises_and_announces_surface_changes() {
        let revision = Arc::new(AtomicU64::new(0));
        let surface = Arc::new(DynamicSurface {
            revision: Arc::clone(&revision),
        });
        let server = Arc::new(
            McpServer::new(Arc::new(NoBackend))
                .with_store(Arc::new(Store::default()))
                .with_surface(surface),
        );
        let (mut client_input, server_input) = tokio::io::duplex(8 * 1024);
        let (server_output, client_output) = tokio::io::duplex(8 * 1024);
        let serving = tokio::spawn(serve_stdio_on(server, server_input, server_output));
        client_input
            .write_all(
                b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-06-18\",\"capabilities\":{},\"clientInfo\":{\"name\":\"test\",\"version\":\"0\"}}}\n",
            )
            .await
            .expect("write initialize");
        let mut lines = BufReader::new(client_output).lines();
        let initialized: Value = serde_json::from_str(
            &tokio::time::timeout(Duration::from_secs(2), lines.next_line())
                .await
                .expect("initialize answer")
                .expect("line")
                .expect("non-empty"),
        )
        .expect("initialize json");
        assert_eq!(
            initialized["result"]["capabilities"]["tools"]["listChanged"].as_bool(),
            Some(true)
        );
        assert_eq!(
            initialized["result"]["instructions"].as_str(),
            Some("surface test")
        );

        revision.store(1, Ordering::SeqCst);
        let notification = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
            .await
            .expect("list changed notification")
            .expect("line")
            .expect("non-empty");
        let notification: Value = serde_json::from_str(&notification).expect("notification json");
        assert_eq!(
            notification["method"].as_str(),
            Some("notifications/tools/list_changed")
        );

        drop(client_input);
        let _ = tokio::time::timeout(Duration::from_secs(2), serving).await;
    }
}
