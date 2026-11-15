//! Cross-platform shutdown signals for the daemon entry points.

use std::io;

#[cfg(unix)]
use tokio::signal::unix::{signal, SignalKind};

/// Resolve when the process should shut down.
///
/// On Unix this waits for either SIGINT (Ctrl+C) or SIGTERM, which is the
/// signal used by systemd, Docker and Kubernetes. On Windows it waits for
/// Ctrl+C, preserving the existing behavior.
pub async fn shutdown() -> io::Result<()> {
    #[cfg(unix)]
    {
        let mut terminate = signal(SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result,
            _ = terminate.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await
    }
}
