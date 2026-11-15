//! Shared snapshot diff state for `page_snapshot`.
//!
//! The cache lives in the application layer so CDP and extension snapshots use
//! exactly the same line grammar and diff semantics. It is intentionally small
//! and process-local: baselines are an interactive convenience, not durable
//! state.

use std::collections::VecDeque;

use serde_json::{json, Value};

/// Maximum number of per-tab baselines retained by the router.
pub const SNAPSHOT_DIFF_CAPACITY: usize = 64;

/// Identity of one snapshot baseline.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SnapshotKey {
    /// Connected browser identifier.
    pub browser: String,
    /// MCP session identifier, empty when the caller is stateless.
    pub session: String,
    /// Target tab identifier reported by the snapshot.
    pub tab: String,
}

impl SnapshotKey {
    /// Build a cache key.
    pub fn new(
        browser: impl Into<String>,
        session: impl Into<String>,
        tab: impl Into<String>,
    ) -> Self {
        Self {
            browser: browser.into(),
            session: session.into(),
            tab: tab.into(),
        }
    }
}

#[derive(Clone, Debug)]
struct Baseline {
    url: String,
    text: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Counts {
    unchanged: usize,
    changed: usize,
    added: usize,
    removed: usize,
}

/// Bounded per-tab snapshot baseline cache.
#[derive(Debug)]
pub struct SnapshotDiffCache {
    capacity: usize,
    entries: VecDeque<(SnapshotKey, Baseline)>,
}

impl Default for SnapshotDiffCache {
    fn default() -> Self {
        Self::new(SNAPSHOT_DIFF_CAPACITY)
    }
}

impl SnapshotDiffCache {
    /// Build a cache with the given capacity. Zero is treated as one.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: VecDeque::new(),
        }
    }

    /// Apply the request's diff flag to a successful compact snapshot result.
    ///
    /// A normal compact snapshot refreshes the baseline. A diff request never
    /// refreshes it: without a matching baseline (or after a URL change) the
    /// text is returned unmarked with `baseline: false`.
    pub fn apply(&mut self, key: SnapshotKey, args: &Value, result: &mut Value) {
        if result.get("format").and_then(Value::as_str) != Some("compact") {
            return;
        }
        let Some(url) = result
            .get("url")
            .and_then(Value::as_str)
            .map(str::to_string)
        else {
            return;
        };
        let Some(text) = result
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_string)
        else {
            return;
        };
        let requested = args.get("diff").and_then(Value::as_bool).unwrap_or(false);
        if requested {
            let baseline = self
                .entries
                .iter()
                .find(|(candidate, _)| candidate == &key)
                .map(|(_, baseline)| baseline.clone());
            match baseline {
                Some(baseline) if baseline.url == url => {
                    let diff = diff_text(&baseline.text, &text);
                    result["text"] = Value::String(diff.text);
                    result["diff"] = json!({
                        "baseline": true,
                        "unchanged": diff.counts.unchanged,
                        "changed": diff.counts.changed,
                        "added": diff.counts.added,
                        "removed": diff.counts.removed,
                    });
                    self.touch(&key);
                }
                _ => {
                    result["diff"] = json!({
                        "baseline": false,
                        "unchanged": 0,
                        "changed": 0,
                        "added": 0,
                        "removed": 0,
                    });
                }
            }
        } else {
            self.refresh(key, url, text);
        }
    }

    fn refresh(&mut self, key: SnapshotKey, url: String, text: String) {
        self.entries.retain(|(candidate, _)| candidate != &key);
        self.entries.push_back((key, Baseline { url, text }));
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
        }
    }

    fn touch(&mut self, key: &SnapshotKey) {
        if let Some(index) = self
            .entries
            .iter()
            .position(|(candidate, _)| candidate == key)
        {
            if let Some(entry) = self.entries.remove(index) {
                self.entries.push_back(entry);
            }
        }
    }
}

struct DiffText {
    text: String,
    counts: Counts,
}

fn diff_text(baseline: &str, current: &str) -> DiffText {
    let baseline_lines = lines(baseline);
    let current_lines = lines(current);
    let mut consumed = vec![false; baseline_lines.len()];
    let mut counts = Counts::default();
    let mut out = Vec::with_capacity(current_lines.len() + baseline_lines.len());
    for line in current_lines {
        let key = line_key(&line);
        if let Some(index) = baseline_lines
            .iter()
            .enumerate()
            .position(|(index, candidate)| !consumed[index] && candidate == &line)
        {
            consumed[index] = true;
            counts.unchanged += 1;
            out.push(format!("  {line}"));
            continue;
        }
        if let Some(index) = baseline_lines
            .iter()
            .enumerate()
            .position(|(index, candidate)| !consumed[index] && line_key(candidate) == key)
        {
            consumed[index] = true;
            counts.changed += 1;
            out.push(format!("~ {line}"));
        } else {
            counts.added += 1;
            out.push(format!("+ {line}"));
        }
    }
    for (index, line) in baseline_lines.iter().enumerate() {
        if !consumed[index] {
            counts.removed += 1;
            out.push(format!("- {line}"));
        }
    }
    DiffText {
        text: out.join("\n"),
        counts,
    }
}

fn lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        Vec::new()
    } else {
        text.split('\n').map(str::to_string).collect()
    }
}

fn line_key(line: &str) -> &str {
    if line.starts_with('@') {
        line.split(' ').next().unwrap_or(line)
    } else {
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> SnapshotKey {
        SnapshotKey::new("browser", "session", "tab")
    }

    fn apply(cache: &mut SnapshotDiffCache, url: &str, text: &str, diff: bool) -> Value {
        let mut result = json!({
            "format": "compact",
            "url": url,
            "tab_id": "tab",
            "text": text,
        });
        cache.apply(key(), &json!({ "diff": diff }), &mut result);
        result
    }

    #[test]
    fn unchanged_lines_keep_the_baseline() {
        let mut cache = SnapshotDiffCache::default();
        apply(
            &mut cache,
            "https://a.test/",
            "@c1 button \"Go\"\n# \"Hello\"",
            false,
        );
        let result = apply(
            &mut cache,
            "https://a.test/",
            "@c1 button \"Go\"\n# \"Hello\"",
            true,
        );
        assert_eq!(result["diff"]["baseline"], true);
        assert_eq!(result["diff"]["unchanged"], 2);
        assert_eq!(result["diff"]["changed"], 0);
        assert_eq!(result["diff"]["added"], 0);
        assert_eq!(result["diff"]["removed"], 0);
        assert_eq!(result["text"], "  @c1 button \"Go\"\n  # \"Hello\"");
    }

    #[test]
    fn added_removed_and_changed_lines_are_marked() {
        let mut cache = SnapshotDiffCache::default();
        apply(
            &mut cache,
            "https://a.test/",
            "@c1 button \"Go\"\n# \"Hi\"\n@c2 textbox \"Name\"",
            false,
        );
        let result = apply(
            &mut cache,
            "https://a.test/",
            "@c1 button \"Go\"\n# \"Hello\"\n@c3 link \"Docs\"",
            true,
        );
        assert_eq!(result["diff"]["unchanged"], 1);
        assert_eq!(result["diff"]["changed"], 0);
        assert_eq!(result["diff"]["added"], 2);
        assert_eq!(result["diff"]["removed"], 2);
        assert_eq!(
            result["text"],
            "  @c1 button \"Go\"\n+ # \"Hello\"\n+ @c3 link \"Docs\"\n- # \"Hi\"\n- @c2 textbox \"Name\""
        );
    }

    #[test]
    fn repeated_text_lines_report_only_unmatched_removals() {
        let mut cache = SnapshotDiffCache::default();
        apply(&mut cache, "https://a.test/", "# \"A\"\n# \"A\"", false);
        let result = apply(&mut cache, "https://a.test/", "# \"A\"", true);
        assert_eq!(result["diff"]["unchanged"], 1);
        assert_eq!(result["diff"]["removed"], 1);
        assert_eq!(result["text"], "  # \"A\"\n- # \"A\"");
    }

    #[test]
    fn changed_element_lines_match_by_reference() {
        let mut cache = SnapshotDiffCache::default();
        apply(&mut cache, "https://a.test/", "@c1 button \"Go\"", false);
        let result = apply(&mut cache, "https://a.test/", "@c1 button \"Stop\"", true);
        assert_eq!(result["diff"]["changed"], 1);
        assert_eq!(result["diff"]["added"], 0);
        assert_eq!(result["diff"]["removed"], 0);
        assert_eq!(result["text"], "~ @c1 button \"Stop\"");
    }

    #[test]
    fn missing_baseline_and_url_reset_are_unmarked() {
        let mut cache = SnapshotDiffCache::default();
        let first = apply(&mut cache, "https://a.test/", "@c1 button \"Go\"", true);
        assert_eq!(first["diff"]["baseline"], false);
        assert_eq!(first["text"], "@c1 button \"Go\"");
        apply(&mut cache, "https://a.test/", "@c1 button \"Go\"", false);
        let other = apply(&mut cache, "https://b.test/", "@c2 link \"Docs\"", true);
        assert_eq!(other["diff"]["baseline"], false);
        assert_eq!(other["diff"]["unchanged"], 0);
        assert_eq!(other["text"], "@c2 link \"Docs\"");
        let same = apply(&mut cache, "https://a.test/", "@c1 button \"Go\"", true);
        assert_eq!(same["diff"]["baseline"], true);
    }

    #[test]
    fn json_format_never_refreshes_the_baseline() {
        let mut cache = SnapshotDiffCache::default();
        let mut result = json!({
            "format": "json",
            "url": "https://a.test/",
            "tab_id": "tab",
            "nodes": [],
        });
        cache.apply(key(), &json!({ "diff": false }), &mut result);
        let diff = apply(&mut cache, "https://a.test/", "@c1 button \"Go\"", true);
        assert_eq!(diff["diff"]["baseline"], false);
    }

    #[test]
    fn the_cache_evicts_the_oldest_baseline() {
        let mut cache = SnapshotDiffCache::new(2);
        apply(&mut cache, "https://a.test/", "@c1 button \"A\"", false);
        for id in ["other-1", "other-2"] {
            let mut result = json!({
                "format": "compact",
                "url": "https://a.test/",
                "text": format!("@c2 button \"{id}\""),
            });
            cache.apply(
                SnapshotKey::new("browser", "session", id),
                &json!({ "diff": false }),
                &mut result,
            );
        }
        let evicted = apply(&mut cache, "https://a.test/", "@c1 button \"A\"", true);
        assert_eq!(evicted["diff"]["baseline"], false);
    }
}
