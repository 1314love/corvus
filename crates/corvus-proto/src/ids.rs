//! Process local identifier generation.
//!
//! Identifiers are time ordered: the millisecond timestamp comes first, so
//! sorting a log by identifier sorts it by time, and a per process sequence
//! keeps identifiers unique inside the same millisecond.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Build a new identifier with the given prefix.
pub fn next_id(prefix: &str) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(0);
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed) % 1_000_000;
    format!("{prefix}-{millis:013}-{sequence:06}")
}

/// Identifier for one command sent to a browser.
pub fn command_id() -> String {
    next_id("cmd")
}

/// Identifier for one transport connection.
pub fn connection_id() -> String {
    next_id("conn")
}

/// Identifier for one recorded session.
pub fn session_id() -> String {
    next_id("s")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_unique_and_ordered() {
        let first = command_id();
        let second = command_id();
        assert_ne!(first, second);
        assert!(first.starts_with("cmd-"));
        assert!(first < second, "identifiers must sort in creation order");
    }
}
