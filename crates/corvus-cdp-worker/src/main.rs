//! Binary entry point for the out-of-process CDP worker.

use std::process::ExitCode;

use corvus_cdp_worker::{serve_worker, WorkerOptions};

#[tokio::main]
async fn main() -> ExitCode {
    let mut reader = tokio::io::stdin();
    let mut writer = tokio::io::stdout();
    match serve_worker(&mut reader, &mut writer, WorkerOptions::default()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("corvus-cdp-worker: {error}");
            ExitCode::FAILURE
        }
    }
}
