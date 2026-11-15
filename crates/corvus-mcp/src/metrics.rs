//! Process-wide tool call counters and the Prometheus rendering.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use corvus_proto::ErrorBody;
use serde_json::{json, Map, Value};

#[derive(Debug, Default, Clone)]
struct ToolStat {
    calls: u64,
    errors: u64,
    total_ms: u64,
}

#[derive(Debug, Default)]
struct Inner {
    by_tool: BTreeMap<String, ToolStat>,
    errors_by_code: BTreeMap<String, u64>,
}

/// Channel and CDP state rendered as Prometheus gauges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelMetrics {
    /// Active channel mode: auto, cdp or extension.
    pub mode: String,
    /// Whether a browser-level CDP connection is usable.
    pub cdp_connected: bool,
    /// Page targets known through CDP.
    pub cdp_targets: u64,
    /// Connected extension browsers.
    pub extension_browsers: u64,
}

impl Default for ChannelMetrics {
    fn default() -> Self {
        Self {
            mode: "unknown".to_string(),
            cdp_connected: false,
            cdp_targets: 0,
            extension_browsers: 0,
        }
    }
}

/// Counters for every tool call served since the process started.
#[derive(Debug)]
pub struct Metrics {
    started_at_ms: i64,
    started: Instant,
    calls: AtomicU64,
    errors: AtomicU64,
    inflight: AtomicU64,
    last_call_at_ms: AtomicU64,
    inner: Mutex<Inner>,
}

impl Metrics {
    /// Create a counter set anchored at the current instant.
    pub fn new() -> Self {
        Self {
            started_at_ms: now_millis(),
            started: Instant::now(),
            calls: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            inflight: AtomicU64::new(0),
            last_call_at_ms: AtomicU64::new(0),
            inner: Mutex::new(Inner::default()),
        }
    }

    /// Milliseconds since the counters were created.
    pub fn uptime_ms(&self) -> u64 {
        self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
    }

    /// Wall clock milliseconds at which the counters were created.
    pub fn started_at_ms(&self) -> i64 {
        self.started_at_ms
    }

    /// Total tool calls counted.
    pub fn calls(&self) -> u64 {
        self.calls.load(Ordering::Relaxed)
    }

    /// Total failed tool calls counted.
    pub fn errors(&self) -> u64 {
        self.errors.load(Ordering::Relaxed)
    }

    /// Calls currently being executed.
    pub fn inflight(&self) -> u64 {
        self.inflight.load(Ordering::Relaxed)
    }

    /// Start counting one call. The returned guard finishes the measurement
    /// when it is finished explicitly or dropped.
    pub fn begin(self: &Arc<Self>, tool: &str) -> CallGuard {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.inflight.fetch_add(1, Ordering::Relaxed);
        self.last_call_at_ms
            .store(now_millis().max(1) as u64, Ordering::Relaxed);
        if let Ok(mut inner) = self.inner.lock() {
            inner.by_tool.entry(tool.to_string()).or_default().calls += 1;
        }
        CallGuard {
            metrics: Arc::clone(self),
            tool: tool.to_string(),
            started: Instant::now(),
            finished: false,
        }
    }

    /// Snapshot of the process counters.
    pub fn snapshot(&self) -> Value {
        let inner = self.inner.lock().ok();
        let mut by_tool = Map::new();
        let mut errors_by_code = Map::new();
        if let Some(inner) = inner.as_ref() {
            for (tool, stat) in &inner.by_tool {
                let average = stat.total_ms.checked_div(stat.calls).unwrap_or(0);
                by_tool.insert(
                    tool.clone(),
                    json!({
                        "calls": stat.calls,
                        "errors": stat.errors,
                        "avg_ms": average,
                    }),
                );
            }
            for (code, count) in &inner.errors_by_code {
                errors_by_code.insert(code.clone(), json!(count));
            }
        }
        let mut document = json!({
            "uptime_ms": self.uptime_ms(),
            "started_at": self.started_at_ms,
            "tool_calls_total": self.calls(),
            "tool_errors_total": self.errors(),
            "by_tool": Value::Object(by_tool),
            "errors_by_code": Value::Object(errors_by_code),
            "inflight": self.inflight(),
        });
        let last = self.last_call_at_ms.load(Ordering::Relaxed);
        if last > 0 {
            document["last_call_at_ms"] = json!(last);
        }
        document
    }

    /// Render the counters as a Prometheus text exposition.
    pub fn prometheus(
        &self,
        browsers_connected: u64,
        sessions_total: u64,
        steps_total: u64,
    ) -> String {
        self.prometheus_with_channel(
            browsers_connected,
            sessions_total,
            steps_total,
            &ChannelMetrics::default(),
        )
    }

    /// Render the counters and the channel gauges as a Prometheus text exposition.
    pub fn prometheus_with_channel(
        &self,
        browsers_connected: u64,
        sessions_total: u64,
        steps_total: u64,
        channel: &ChannelMetrics,
    ) -> String {
        let snapshot = self.snapshot();
        let mut text = String::new();
        text.push_str("# HELP corvus_uptime_seconds Seconds since the daemon process started.\n");
        text.push_str("# TYPE corvus_uptime_seconds gauge\n");
        text.push_str(&format!(
            "corvus_uptime_seconds {:.3}\n",
            self.uptime_ms() as f64 / 1000.0
        ));
        text.push_str("# HELP corvus_browsers_connected Number of connected browsers.\n");
        text.push_str("# TYPE corvus_browsers_connected gauge\n");
        text.push_str(&format!("corvus_browsers_connected {browsers_connected}\n"));
        text.push_str("# HELP corvus_tool_calls_total Tool calls served.\n");
        text.push_str("# TYPE corvus_tool_calls_total counter\n");
        if let Some(by_tool) = snapshot.get("by_tool").and_then(Value::as_object) {
            for (tool, stat) in by_tool {
                let calls = stat.get("calls").and_then(Value::as_u64).unwrap_or(0);
                text.push_str(&format!(
                    "corvus_tool_calls_total{{tool=\"{}\"}} {}\n",
                    escape_label(tool),
                    calls
                ));
            }
        }
        text.push_str("# HELP corvus_tool_errors_total Tool calls that returned an error.\n");
        text.push_str("# TYPE corvus_tool_errors_total counter\n");
        if let Some(by_code) = snapshot.get("errors_by_code").and_then(Value::as_object) {
            for (code, count) in by_code {
                let count = count.as_u64().unwrap_or(0);
                text.push_str(&format!(
                    "corvus_tool_errors_total{{code=\"{}\"}} {}\n",
                    escape_label(code),
                    count
                ));
            }
        }
        text.push_str("# HELP corvus_sessions_total Recorded sessions.\n");
        text.push_str("# TYPE corvus_sessions_total gauge\n");
        text.push_str(&format!("corvus_sessions_total {sessions_total}\n"));
        text.push_str("# HELP corvus_steps_total Recorded steps.\n");
        text.push_str("# TYPE corvus_steps_total gauge\n");
        text.push_str(&format!("corvus_steps_total {steps_total}\n"));
        text.push_str("# HELP corvus_channel Active browser channel mode.\n");
        text.push_str("# TYPE corvus_channel gauge\n");
        text.push_str(&format!(
            "corvus_channel{{mode=\"{}\"}} 1\n",
            escape_label(&channel.mode)
        ));
        text.push_str(
            "# HELP corvus_cdp_connected Whether a browser-level CDP connection is active.\n",
        );
        text.push_str("# TYPE corvus_cdp_connected gauge\n");
        text.push_str(&format!(
            "corvus_cdp_connected {}\n",
            u8::from(channel.cdp_connected)
        ));
        text.push_str("# HELP corvus_cdp_targets Page targets known through CDP.\n");
        text.push_str("# TYPE corvus_cdp_targets gauge\n");
        text.push_str(&format!("corvus_cdp_targets {}\n", channel.cdp_targets));
        text.push_str("# HELP corvus_extension_browsers Connected extension browsers.\n");
        text.push_str("# TYPE corvus_extension_browsers gauge\n");
        text.push_str(&format!(
            "corvus_extension_browsers {}\n",
            channel.extension_browsers
        ));
        text
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

/// One in-flight call measurement.
#[derive(Debug)]
pub struct CallGuard {
    metrics: Arc<Metrics>,
    tool: String,
    started: Instant,
    finished: bool,
}

impl CallGuard {
    /// Finish the measurement, recording the error code when the call failed.
    pub fn finish(&mut self, error: Option<&ErrorBody>) {
        if self.finished {
            return;
        }
        self.finished = true;
        let elapsed = self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        self.metrics.inflight.fetch_sub(1, Ordering::Relaxed);
        if let Ok(mut inner) = self.metrics.inner.lock() {
            if let Some(stat) = inner.by_tool.get_mut(&self.tool) {
                stat.total_ms = stat.total_ms.saturating_add(elapsed);
                if error.is_some() {
                    stat.errors = stat.errors.saturating_add(1);
                }
            }
            if let Some(error) = error {
                self.metrics.errors.fetch_add(1, Ordering::Relaxed);
                *inner
                    .errors_by_code
                    .entry(error.code.as_str().to_string())
                    .or_insert(0) += 1;
            }
        }
    }
}

impl Drop for CallGuard {
    fn drop(&mut self) {
        self.finish(None);
    }
}

fn escape_label(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use corvus_proto::ErrorCode;

    #[test]
    fn calls_errors_and_averages_are_counted() {
        let metrics = Arc::new(Metrics::new());
        let mut first = metrics.begin("browser_click");
        first.finish(None);
        let failure = ErrorBody::new(ErrorCode::NotFound, "missing");
        let mut second = metrics.begin("browser_click");
        second.finish(Some(&failure));
        assert_eq!(metrics.calls(), 2);
        assert_eq!(metrics.errors(), 1);
        assert_eq!(metrics.inflight(), 0);
        let snapshot = metrics.snapshot();
        let click = &snapshot["by_tool"]["browser_click"];
        assert_eq!(click["calls"], 2);
        assert_eq!(click["errors"], 1);
        assert!(click["avg_ms"].as_u64().is_some());
        assert_eq!(snapshot["errors_by_code"]["NOT_FOUND"], 1);
    }

    #[test]
    fn a_dropped_guard_counts_a_successful_call() {
        let metrics = Arc::new(Metrics::new());
        {
            let _guard = metrics.begin("page_text");
        }
        assert_eq!(metrics.calls(), 1);
        assert_eq!(metrics.errors(), 0);
        assert_eq!(metrics.inflight(), 0);
    }

    #[test]
    fn prometheus_contains_the_required_series() {
        let metrics = Arc::new(Metrics::new());
        let mut guard = metrics.begin("browser_click");
        guard.finish(Some(&ErrorBody::new(ErrorCode::Timeout, "slow")));
        let text = metrics.prometheus(2, 3, 4);
        assert!(text.contains("corvus_uptime_seconds "));
        assert!(text.contains("corvus_browsers_connected 2"));
        assert!(text.contains("corvus_tool_calls_total{tool=\"browser_click\"} 1"));
        assert!(text.contains("corvus_tool_errors_total{code=\"TIMEOUT\"} 1"));
        assert!(text.contains("corvus_sessions_total 3"));
        assert!(text.contains("corvus_steps_total 4"));
        assert!(text.contains("corvus_channel{mode=\"unknown\"} 1"));
        assert!(text.contains("corvus_cdp_connected 0"));
        assert!(text.contains("corvus_cdp_targets 0"));
        assert!(text.contains("corvus_extension_browsers 0"));
    }

    #[test]
    fn prometheus_renders_channel_gauges() {
        let metrics = Metrics::new();
        let channel = ChannelMetrics {
            mode: "auto".to_string(),
            cdp_connected: true,
            cdp_targets: 3,
            extension_browsers: 1,
        };
        let text = metrics.prometheus_with_channel(1, 2, 5, &channel);
        assert!(text.contains("corvus_channel{mode=\"auto\"} 1"));
        assert!(text.contains("corvus_cdp_connected 1"));
        assert!(text.contains("corvus_cdp_targets 3"));
        assert!(text.contains("corvus_extension_browsers 1"));
        assert!(text.contains("corvus_sessions_total 2"));
    }
}
