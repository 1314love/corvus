//! Manual smoke path: spawn a worker, call cdp.get_version and cdp.status.
//!
//! Start an isolated browser with a loopback debugging port, export
//! CORVUS_CDP_ENDPOINT to its browser WebSocket endpoint, then run:
//! cargo run -p corvus-cdp-worker --example worker_smoke -- <worker-exe>

use std::process::Stdio;

use corvus_worker::{read_frame, read_handshake, write_frame, WorkerMessage};
use tokio::process::Command;
use tokio::time::{timeout, Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let program = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "corvus-cdp-worker".into());
    let endpoint = std::env::var("CORVUS_CDP_ENDPOINT")?;
    let mut child = Command::new(program)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().ok_or("worker stdin unavailable")?;
    let mut stdout = child.stdout.take().ok_or("worker stdout unavailable")?;

    let hello = WorkerMessage::WorkerHello {
        protocol: 1,
        worker_id: "w-smoke".into(),
        browser_id: "browser-smoke".into(),
        cdp_ws_endpoint: endpoint,
        home_dir: std::env::temp_dir().display().to_string(),
        log_level: "info".into(),
    };
    write_frame(&mut stdin, &serde_json::to_value(&hello)?).await?;
    let ack = read_handshake(&mut stdout).await?;
    println!("ack: {ack}");

    for (id, action) in [("smoke-1", "cdp.get_version"), ("smoke-2", "cdp.status")] {
        let invoke = WorkerMessage::Invoke {
            id: id.into(),
            action: action.into(),
            args: serde_json::json!({}),
            deadline_ms: 15_000,
        };
        write_frame(&mut stdin, &serde_json::to_value(&invoke)?).await?;
        let result = read_frame(&mut stdout).await?;
        println!("result: {result}");
    }

    write_frame(
        &mut stdin,
        &serde_json::to_value(WorkerMessage::Shutdown {})?,
    )
    .await?;
    let status = timeout(Duration::from_secs(2), child.wait()).await??;
    println!("exit: {status}");
    Ok(())
}
