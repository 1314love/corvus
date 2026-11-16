//! Out-of-process Corvus CDP worker.

#![warn(missing_docs)]

use std::time::Duration;

use corvus_cdp::CdpClient;
use corvus_worker::{
    read_frame, read_handshake, write_frame, WorkerBrowser, WorkerError, WorkerErrorBody,
    WorkerMessage,
};
use serde_json::{json, Value};
use tokio::io::{AsyncRead, AsyncWrite};

/// Timing policy for one worker lifetime.
#[derive(Debug, Clone)]
pub struct WorkerOptions {
    /// Maximum wait for the CDP WebSocket to connect.
    pub connect_timeout: Duration,
    /// Default CDP request timeout.
    pub call_timeout: Duration,
}

impl Default for WorkerOptions {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(5),
            call_timeout: Duration::from_secs(15),
        }
    }
}

/// Serve one gateway worker connection over the supplied stdio pair.
///
/// A clean handshake refusal writes a failed `worker_ack` and returns `Ok(())`.
/// A runtime or framing failure returns `Err`, which the binary exits nonzero.
pub async fn serve_worker<R, W>(
    reader: &mut R,
    writer: &mut W,
    options: WorkerOptions,
) -> Result<(), WorkerError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let hello = read_handshake(reader).await?;
    let hello: WorkerMessage = serde_json::from_value(hello)?;
    let (protocol, worker_id, endpoint) = match hello {
        WorkerMessage::WorkerHello {
            protocol,
            worker_id,
            cdp_ws_endpoint,
            ..
        } => (protocol, worker_id, cdp_ws_endpoint),
        other => {
            return Err(WorkerError::Handshake(format!(
                "expected worker_hello, got {}",
                other.kind()
            )))
        }
    };
    if protocol != 1 {
        write_ack(
            writer,
            &worker_id,
            None,
            false,
            Some(WorkerErrorBody {
                code: "PROTOCOL_ERROR".into(),
                message: format!("unsupported worker protocol {protocol}"),
                retryable: false,
            }),
        )
        .await?;
        return Ok(());
    }

    let client = CdpClient::new();
    let connected =
        client.connect(&endpoint).is_ok() && client.wait_connected(options.connect_timeout).await;
    if !connected {
        write_ack(
            writer,
            &worker_id,
            None,
            false,
            Some(WorkerErrorBody {
                code: "PROVIDER_LOST".into(),
                message: "the browser endpoint did not connect".into(),
                retryable: true,
            }),
        )
        .await?;
        return Ok(());
    }

    let version = match client
        .send("Browser.getVersion", json!({}), None, None, Some(5_000))
        .await
    {
        Ok(value) => value,
        Err(error) => {
            write_ack(
                writer,
                &worker_id,
                None,
                false,
                Some(WorkerErrorBody {
                    code: "CDP_ERROR".into(),
                    message: "the browser did not report its version".into(),
                    retryable: error.retryable(),
                }),
            )
            .await?;
            client.disconnect();
            return Ok(());
        }
    };
    let browser = browser_from_version(&version);
    write_ack(writer, &worker_id, Some(browser.clone()), true, None).await?;

    loop {
        let frame = match read_frame(reader).await {
            Ok(frame) => frame,
            Err(WorkerError::Io(_)) => break,
            Err(error) => {
                client.disconnect();
                return Err(error);
            }
        };
        let message: WorkerMessage = match serde_json::from_value(frame) {
            Ok(message) => message,
            Err(error) => {
                let _ = write_frame(
                    writer,
                    &serde_json::to_value(WorkerMessage::Error {
                        id: None,
                        code: "PROTOCOL_ERROR".into(),
                        message: error.to_string(),
                        retryable: false,
                    })?,
                )
                .await;
                client.disconnect();
                return Err(WorkerError::Malformed(error.to_string()));
            }
        };
        match message {
            WorkerMessage::Invoke {
                id,
                action,
                args,
                deadline_ms,
            } => {
                let deadline = Duration::from_millis(deadline_ms.clamp(1, 120_000));
                let result = dispatch(&client, &action, args, deadline, options.call_timeout).await;
                let value = match result {
                    Ok(value) => WorkerMessage::Result {
                        id,
                        ok: true,
                        value: Some(value),
                        error: None,
                    },
                    Err(error) => WorkerMessage::Result {
                        id,
                        ok: false,
                        value: None,
                        error: Some(WorkerErrorBody {
                            code: error.code().into(),
                            message: error.to_string(),
                            retryable: error.retryable(),
                        }),
                    },
                };
                write_frame(writer, &serde_json::to_value(value)?).await?;
            }
            WorkerMessage::Cancel { .. } => {}
            WorkerMessage::Shutdown {} => break,
            WorkerMessage::Error { code, message, .. } => {
                client.disconnect();
                return Err(WorkerError::Malformed(format!("{code}: {message}")));
            }
            other => {
                let _ = write_frame(
                    writer,
                    &serde_json::to_value(WorkerMessage::Error {
                        id: None,
                        code: "BAD_ARGS".into(),
                        message: format!("unexpected worker message {}", other.kind()),
                        retryable: false,
                    })?,
                )
                .await;
            }
        }
    }
    client.disconnect();
    Ok(())
}

async fn write_ack<W: AsyncWrite + Unpin>(
    writer: &mut W,
    worker_id: &str,
    browser: Option<WorkerBrowser>,
    ok: bool,
    error: Option<WorkerErrorBody>,
) -> Result<(), WorkerError> {
    let ack = WorkerMessage::WorkerAck {
        protocol: 1,
        worker_id: worker_id.into(),
        provider_id: format!("cdp-{worker_id}"),
        ok,
        browser,
        capabilities: p2_ack_capabilities(),
        error,
    };
    write_frame(writer, &serde_json::to_value(ack)?).await
}

fn p2_ack_capabilities() -> Vec<String> {
    let mut capabilities: Vec<String> = corvus_cdp::P2_GROUPS
        .iter()
        .map(|(group, _)| (*group).to_string())
        .collect();
    capabilities.extend(corvus_cdp::p2_capability_actions());
    capabilities
}

fn browser_from_version(value: &Value) -> WorkerBrowser {
    WorkerBrowser {
        version: value
            .get("product")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        protocol_version: value
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
    }
}

async fn dispatch(
    client: &CdpClient,
    action: &str,
    args: Value,
    deadline: Duration,
    default_timeout: Duration,
) -> Result<Value, WorkerError> {
    match action {
        "cdp.get_version" => {
            if !args.as_object().is_some_and(|object| object.is_empty()) {
                return Err(WorkerError::BadArgs(
                    "cdp.get_version takes no arguments".into(),
                ));
            }
            let value = client
                .send(
                    "Browser.getVersion",
                    json!({}),
                    None,
                    None,
                    Some(timeout_ms(deadline, default_timeout)),
                )
                .await
                .map_err(cdp_error)?;
            Ok(json!({
                "browser": value.get("product").and_then(Value::as_str).unwrap_or("unknown"),
                "protocol_version": value
                    .get("protocolVersion")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown"),
            }))
        }
        "cdp.status" => {
            if !args.as_object().is_some_and(|object| object.is_empty()) {
                return Err(WorkerError::BadArgs("cdp.status takes no arguments".into()));
            }
            let status = client.status();
            Ok(json!({
                "connected": status.connected,
                "browser": status.browser,
                "protocol_version": status.protocol_version,
                "targets": status.targets,
                "connected_since_ms": status.connected_since_ms,
                "reconnects": status.reconnects,
            }))
        }
        action if corvus_cdp::P2_ACTIONS.contains(&action) => client
            .worker_action(action, &args, timeout_ms(deadline, default_timeout))
            .await
            .map_err(p2_action_error),
        _ => Err(WorkerError::Action {
            code: "BAD_ARGS",
            message: "unknown action".into(),
        }),
    }
}

fn timeout_ms(deadline: Duration, fallback: Duration) -> u64 {
    deadline.min(fallback).as_millis().clamp(1, 120_000) as u64
}

fn cdp_error(error: corvus_cdp::CdpError) -> WorkerError {
    WorkerError::Cdp(error.to_string())
}

fn p2_action_error(error: corvus_cdp::P2ActionError) -> WorkerError {
    WorkerError::Action {
        code: error.code(),
        message: error.message().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn browser_version_is_reduced_to_the_protocol_facts() {
        let browser = browser_from_version(&json!({
            "product": "Corvus/157.0.0.0",
            "protocolVersion": "1.3",
            "userAgent": "ignored"
        }));
        assert_eq!(browser.version, "Corvus/157.0.0.0");
        assert_eq!(browser.protocol_version, "1.3");
    }

    #[tokio::test]
    async fn unreachable_cdp_endpoint_is_refused_with_an_ack() {
        let (mut gateway, worker) = tokio::io::duplex(64 * 1024);
        let (mut reader, mut writer) = tokio::io::split(worker);
        let serve = tokio::spawn(async move {
            serve_worker(
                &mut reader,
                &mut writer,
                WorkerOptions {
                    connect_timeout: Duration::from_millis(50),
                    call_timeout: Duration::from_secs(1),
                },
            )
            .await
        });
        let hello = json!({
            "kind": "worker_hello",
            "protocol": 1,
            "worker_id": "w-test",
            "browser_id": "b-test",
            "cdp_ws_endpoint": "ws://127.0.0.1:1/devtools/browser/test",
            "home_dir": "C:/tmp",
            "log_level": "info"
        });
        corvus_worker::write_frame(&mut gateway, &hello)
            .await
            .expect("hello");
        let ack = corvus_worker::read_handshake(&mut gateway)
            .await
            .expect("ack");
        assert_eq!(ack["kind"], "worker_ack");
        assert_eq!(ack["ok"], false);
        assert_eq!(ack["error"]["code"], "PROVIDER_LOST");
        task_result(serve.await.expect("serve task"));
    }

    fn task_result(result: Result<(), WorkerError>) {
        assert!(result.is_ok());
    }
}
