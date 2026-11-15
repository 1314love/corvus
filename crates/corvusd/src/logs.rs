//! In-process tracing capture for the logs_tail tool.
//!
//! The buffer is bounded and shared process wide. The daemon installs the
//! capture layer in its tracing subscriber; tests that start a daemon without
//! one get a capture-only subscriber so failures are still observable.

use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Mutex, Once, OnceLock};

use corvus_proto::{ErrorBody, ErrorCode};
use serde_json::{json, Map, Value};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::LookupSpan;

/// Maximum number of retained log entries.
pub const CAPACITY: usize = 5_000;
/// Maximum number of captured fields per entry.
pub const MAX_FIELDS: usize = 16;

const LEVELS: [&str; 4] = ["debug", "info", "warn", "error"];

/// One captured tracing event.
#[derive(Debug, Clone)]
pub struct LogEntry {
    /// Event time in milliseconds since the Unix epoch.
    pub ts: i64,
    /// Severity name: debug, info, warn or error.
    pub level: String,
    /// Module that emitted the event.
    pub target: String,
    /// Rendered message text.
    pub message: String,
    /// Structured fields attached to the event, capped at [`MAX_FIELDS`].
    pub fields: Map<String, Value>,
}

impl LogEntry {
    /// Render the entry in the logs_tail result shape.
    pub fn to_value(&self) -> Value {
        json!({
            "ts": self.ts,
            "level": self.level,
            "target": self.target,
            "message": self.message,
            "fields": Value::Object(self.fields.clone()),
        })
    }
}

#[derive(Debug, Default)]
struct Inner {
    entries: VecDeque<LogEntry>,
    dropped: u64,
}

/// Bounded, cloneable handle to the process log ring.
#[derive(Debug, Clone, Default)]
pub struct LogBuffer {
    inner: Arc<Mutex<Inner>>,
}

impl LogBuffer {
    /// Create an empty buffer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append one entry, evicting the oldest when the ring is full.
    pub fn record(&self, level: &str, target: &str, message: &str, fields: Map<String, Value>) {
        let entry = LogEntry {
            ts: now_millis(),
            level: level.to_string(),
            target: target.to_string(),
            message: message.to_string(),
            fields,
        };
        self.push(entry);
    }

    /// Append a prepared entry.
    pub fn push(&self, entry: LogEntry) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        if inner.entries.len() >= CAPACITY {
            inner.entries.pop_front();
            inner.dropped = inner.dropped.saturating_add(1);
        }
        inner.entries.push_back(entry);
    }

    /// Return at most limit matching entries in chronological order together
    /// with the number of entries dropped since the buffer was created.
    pub fn tail(
        &self,
        limit: usize,
        minimum_level: Option<&str>,
        since_ms: Option<i64>,
    ) -> (Vec<LogEntry>, u64) {
        let Ok(inner) = self.inner.lock() else {
            return (Vec::new(), 0);
        };
        let minimum = minimum_level.and_then(level_rank);
        let filtered: Vec<&LogEntry> = inner
            .entries
            .iter()
            .filter(|entry| {
                if let Some(since) = since_ms {
                    if entry.ts < since {
                        return false;
                    }
                }
                if let Some(minimum) = minimum {
                    let rank = level_rank(&entry.level).unwrap_or(0);
                    if rank < minimum {
                        return false;
                    }
                }
                true
            })
            .collect();
        let start = filtered.len().saturating_sub(limit);
        (
            filtered[start..]
                .iter()
                .map(|entry| (*entry).clone())
                .collect(),
            inner.dropped,
        )
    }
}

impl<S> Layer<S> for LogBuffer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        let mut visitor = EventVisitor::default();
        event.record(&mut visitor);
        self.record(
            &event.metadata().level().as_str().to_ascii_lowercase(),
            event.metadata().target(),
            visitor.message.as_deref().unwrap_or(""),
            visitor.fields,
        );
    }
}

#[derive(Default)]
struct EventVisitor {
    message: Option<String>,
    fields: Map<String, Value>,
}

impl EventVisitor {
    fn insert(&mut self, field: &Field, value: String) {
        if field.name() == "message" {
            self.message = Some(value);
        } else if self.fields.len() < MAX_FIELDS {
            self.fields
                .insert(field.name().to_string(), Value::String(value));
        }
    }
}

impl Visit for EventVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.insert(field, value.to_string());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.insert(field, value.to_string());
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.insert(field, value.to_string());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.insert(field, value.to_string());
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.insert(field, value.to_string());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.insert(field, format!("{value:?}"));
    }
}

/// The process-wide ring.
pub fn global() -> &'static LogBuffer {
    static BUFFER: OnceLock<LogBuffer> = OnceLock::new();
    BUFFER.get_or_init(LogBuffer::new)
}

/// Ensure a subscriber capturing into the ring exists when the process has no
/// subscriber yet. Binary entry points install their own formatted subscriber
/// with the same layer; this fallback keeps library-only daemons observable.
pub fn ensure_installed() -> LogBuffer {
    static INSTALL: Once = Once::new();
    let buffer = global().clone();
    INSTALL.call_once(|| {
        let subscriber = tracing_subscriber::registry().with(buffer.clone());
        let _ = tracing::subscriber::set_global_default(subscriber);
    });
    buffer
}

/// Decode validated logs_tail arguments and read the ring.
pub fn tail(arguments: &Value) -> Result<Value, ErrorBody> {
    let limit = arguments
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(200)
        .clamp(1, 1_000) as usize;
    let level = arguments.get("level").and_then(Value::as_str);
    if let Some(value) = level {
        if !LEVELS.contains(&value) {
            return Err(ErrorBody::new(
                ErrorCode::BadArgs,
                format!("logs_tail level must be one of {}", LEVELS.join(", ")),
            )
            .with_details(json!({ "level": value })));
        }
    }
    let since = arguments.get("since_ms").and_then(Value::as_i64);
    let (entries, dropped) = global().tail(limit, level, since);
    Ok(json!({
        "entries": entries.iter().map(LogEntry::to_value).collect::<Vec<_>>(),
        "dropped": dropped,
    }))
}

fn level_rank(level: &str) -> Option<u8> {
    match level.to_ascii_lowercase().as_str() {
        "trace" => Some(0),
        "debug" => Some(1),
        "info" => Some(2),
        "warn" => Some(3),
        "error" => Some(4),
        _ => None,
    }
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(ts: i64, level: &str, message: &str) -> LogEntry {
        LogEntry {
            ts,
            level: level.to_string(),
            target: "test".to_string(),
            message: message.to_string(),
            fields: Map::new(),
        }
    }

    #[test]
    fn tail_limits_and_counts_dropped_entries() {
        let buffer = LogBuffer::new();
        for index in 0..5_002 {
            buffer.push(entry(index, "info", "line"));
        }
        let (entries, dropped) = buffer.tail(2, None, None);
        assert_eq!(dropped, 2);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].ts, 5_000);
        assert_eq!(entries[1].ts, 5_001);
    }

    #[test]
    fn tail_filters_by_minimum_level_and_since() {
        let buffer = LogBuffer::new();
        buffer.push(entry(10, "debug", "debug"));
        buffer.push(entry(20, "info", "info"));
        buffer.push(entry(30, "warn", "warn"));
        buffer.push(entry(40, "error", "error"));
        let (entries, _) = buffer.tail(10, Some("warn"), None);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].level, "warn");
        assert_eq!(entries[1].level, "error");
        let (entries, _) = buffer.tail(10, None, Some(25));
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].ts, 30);
    }

    #[test]
    fn an_invalid_level_is_bad_args() {
        let error = tail(&json!({ "level": "verbose" })).expect_err("invalid level");
        assert_eq!(error.code, ErrorCode::BadArgs);
        assert_eq!(error.details.as_ref().unwrap()["level"], "verbose");
    }

    #[test]
    fn the_layer_captures_message_and_fields() {
        let buffer = LogBuffer::new();
        let subscriber = tracing_subscriber::registry().with(buffer.clone());
        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!(tool = "browser_click", attempt = 2, "tool call failed");
        });
        let (entries, _) = buffer.tail(10, None, None);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].level, "warn");
        assert_eq!(entries[0].message, "tool call failed");
        assert_eq!(entries[0].fields["tool"], "browser_click");
        assert_eq!(entries[0].fields["attempt"], "2");
    }
}
