//! Tracing setup shared by the daemon and CLI binaries.
//!
//! Every process writes formatted events to stderr and into the in-memory ring
//! used by the logs_tail tool. When a log file is configured, the same events
//! also go to a daily rotating file without blocking request handling.

use std::io;
use std::path::{Path, PathBuf};

use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

use crate::logs;

/// Keeps the non-blocking file writer alive; dropping it flushes queued lines.
pub struct LoggingGuard {
    _worker: Option<WorkerGuard>,
}

/// Install stderr, in-memory and optional daily-file logging.
///
/// Parent directories are created only inside the resolved home. An explicit
/// log path outside the home must already have a writable parent; when the file
/// cannot be opened the process keeps logging to stderr and the in-memory ring.
pub fn init(log_file: Option<PathBuf>, home_root: &Path) -> LoggingGuard {
    let Some(path) = log_file else {
        install(None);
        return LoggingGuard { _worker: None };
    };
    let path = resolve_log_path(&path, home_root);
    match open_appender(&path, home_root) {
        Ok((writer, guard)) => {
            install(Some(writer));
            LoggingGuard {
                _worker: Some(guard),
            }
        }
        Err(error) => {
            eprintln!(
                "corvus: file logging disabled for {}: {error}; continuing on stderr",
                path.display()
            );
            install(None);
            LoggingGuard { _worker: None }
        }
    }
}

/// Anchor a relative log-file override in the home so no log file is created
/// next to the process working directory.
pub fn resolve_log_path(path: &Path, home_root: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        home_root.join(path)
    }
}

fn open_appender(path: &Path, home_root: &Path) -> io::Result<(NonBlocking, WorkerGuard)> {
    let directory = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    if directory.starts_with(home_root) {
        std::fs::create_dir_all(directory)?;
    }
    if !directory.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("log directory {} does not exist", directory.display()),
        ));
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "log file path must end in a UTF-8 file name",
            )
        })?;
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(name)
        .build(directory)
        .map_err(|error| io::Error::other(error.to_string()))?;
    Ok(tracing_appender::non_blocking(appender))
}

fn install(file_writer: Option<NonBlocking>) {
    let filter = || EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    if let Some(writer) = file_writer {
        tracing_subscriber::registry()
            .with(filter())
            .with(logs::global().clone())
            .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(writer)
                    .with_ansi(false),
            )
            .init();
    } else {
        tracing_subscriber::registry()
            .with(filter())
            .with(logs::global().clone())
            .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
            .init();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_log_paths_are_rooted_in_the_home() {
        let home = std::env::temp_dir().join("corvus-log-home");
        let relative = resolve_log_path(Path::new("nested/corvusd.log"), &home);
        assert_eq!(relative, home.join("nested").join("corvusd.log"));
        let absolute = home.join("logs").join("custom.log");
        assert_eq!(resolve_log_path(&absolute, &home), absolute);
    }
}
