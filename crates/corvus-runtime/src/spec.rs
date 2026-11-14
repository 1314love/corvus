//! The tool surface the daemon accepts from agents.

use corvus_proto::capability;
use serde_json::Value;

/// Default budget for browser_wait when the caller does not name one.
pub const WAIT_DEFAULT_MS: u64 = 10_000;
/// Smallest wait budget a caller may request.
pub const WAIT_MIN_MS: u64 = 100;
/// Largest wait budget a caller may request.
pub const WAIT_MAX_MS: u64 = 120_000;
/// Extra budget on top of the wait budget so the daemon outlives the page wait.
pub const WAIT_GRACE_MS: u64 = 5_000;
/// Hard ceiling for the browser_wait command timeout.
pub const WAIT_CEILING_MS: u64 = 125_000;
/// Default budget for browser_eval.
pub const EVAL_TIMEOUT_MS: u64 = 30_000;
/// Default wait budget for download_wait.
pub const DOWNLOAD_WAIT_DEFAULT_MS: u64 = 30_000;
/// Smallest download wait budget a caller may request.
pub const DOWNLOAD_WAIT_MIN_MS: u64 = 100;
/// Largest download wait budget a caller may request.
pub const DOWNLOAD_WAIT_MAX_MS: u64 = 120_000;
/// Extra budget on top of the download wait so the daemon outlives it.
pub const DOWNLOAD_WAIT_GRACE_MS: u64 = 5_000;
/// Hard ceiling for the download_wait command timeout.
pub const DOWNLOAD_WAIT_CEILING_MS: u64 = 125_000;

/// One tool the router knows how to forward to a browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolSpec {
    /// Stable tool name.
    pub name: &'static str,
    /// Whether two calls may not overlap for the same browser.
    pub serial: bool,
    /// Capability the browser must offer before the tool is forwarded; None
    /// means the tool only needs the browser connection itself.
    pub capability: Option<&'static str>,
    /// Timeout used when the caller does not request one; None uses the
    /// router default.
    pub default_timeout_ms: Option<u64>,
}

/// Every tool exposed in this version.
pub const TOOLS: [ToolSpec; 26] = [
    ToolSpec {
        name: "browser_navigate",
        serial: true,
        capability: Some(capability::DOM),
        default_timeout_ms: None,
    },
    ToolSpec {
        name: "page_snapshot",
        serial: false,
        capability: Some(capability::DOM),
        default_timeout_ms: None,
    },
    ToolSpec {
        name: "browser_click",
        serial: true,
        capability: Some(capability::DOM),
        default_timeout_ms: None,
    },
    ToolSpec {
        name: "browser_type",
        serial: true,
        capability: Some(capability::DOM),
        default_timeout_ms: None,
    },
    ToolSpec {
        name: "page_text",
        serial: false,
        capability: Some(capability::DOM),
        default_timeout_ms: None,
    },
    ToolSpec {
        name: "page_html",
        serial: false,
        capability: Some(capability::DOM),
        default_timeout_ms: None,
    },
    ToolSpec {
        name: "page_find",
        serial: false,
        capability: Some(capability::DOM),
        default_timeout_ms: None,
    },
    ToolSpec {
        name: "browser_hover",
        serial: true,
        capability: Some(capability::DOM),
        default_timeout_ms: None,
    },
    ToolSpec {
        name: "browser_press",
        serial: true,
        capability: Some(capability::DOM),
        default_timeout_ms: None,
    },
    ToolSpec {
        name: "browser_scroll",
        serial: true,
        capability: Some(capability::DOM),
        default_timeout_ms: None,
    },
    ToolSpec {
        name: "browser_select",
        serial: true,
        capability: Some(capability::DOM),
        default_timeout_ms: None,
    },
    ToolSpec {
        name: "browser_eval",
        serial: true,
        capability: Some(capability::DOM),
        default_timeout_ms: Some(EVAL_TIMEOUT_MS),
    },
    ToolSpec {
        name: "browser_wait",
        serial: false,
        capability: Some(capability::DOM),
        default_timeout_ms: None,
    },
    ToolSpec {
        name: "browser_history",
        serial: true,
        capability: Some(capability::DOM),
        default_timeout_ms: None,
    },
    ToolSpec {
        name: "tab_list",
        serial: false,
        capability: None,
        default_timeout_ms: None,
    },
    ToolSpec {
        name: "tab_new",
        serial: true,
        capability: None,
        default_timeout_ms: None,
    },
    ToolSpec {
        name: "tab_close",
        serial: true,
        capability: None,
        default_timeout_ms: None,
    },
    ToolSpec {
        name: "tab_switch",
        serial: true,
        capability: None,
        default_timeout_ms: None,
    },
    ToolSpec {
        name: "browser_list",
        serial: false,
        capability: None,
        default_timeout_ms: Some(60_000),
    },
    ToolSpec {
        name: "net_capture",
        serial: true,
        capability: None,
        default_timeout_ms: Some(60_000),
    },
    ToolSpec {
        name: "net_log",
        serial: false,
        capability: None,
        default_timeout_ms: Some(60_000),
    },
    ToolSpec {
        name: "net_clear",
        serial: true,
        capability: None,
        default_timeout_ms: Some(60_000),
    },
    ToolSpec {
        name: "net_intercept",
        serial: true,
        capability: None,
        default_timeout_ms: Some(60_000),
    },
    ToolSpec {
        name: "download_list",
        serial: false,
        capability: None,
        default_timeout_ms: Some(60_000),
    },
    ToolSpec {
        name: "download_wait",
        serial: false,
        capability: None,
        default_timeout_ms: Some(60_000),
    },
    ToolSpec {
        name: "upload_file",
        serial: true,
        capability: None,
        default_timeout_ms: Some(60_000),
    },
];

impl ToolSpec {
    /// Effective command timeout for this tool and these arguments.
    ///
    /// The router clamps the result again to its own ceiling. browser_wait
    /// adds a grace period so the page can report its own timeout first.
    pub fn timeout_ms(&self, arguments: &Value, default_timeout_ms: u64) -> u64 {
        if self.name == "browser_wait" {
            let requested = arguments
                .get("timeout_ms")
                .and_then(Value::as_u64)
                .unwrap_or(WAIT_DEFAULT_MS)
                .clamp(WAIT_MIN_MS, WAIT_MAX_MS);
            return requested.saturating_add(WAIT_GRACE_MS).min(WAIT_CEILING_MS);
        }
        if self.name == "download_wait" {
            let requested = arguments
                .get("timeout_ms")
                .and_then(Value::as_u64)
                .unwrap_or(DOWNLOAD_WAIT_DEFAULT_MS)
                .clamp(DOWNLOAD_WAIT_MIN_MS, DOWNLOAD_WAIT_MAX_MS);
            return requested
                .saturating_add(DOWNLOAD_WAIT_GRACE_MS)
                .min(DOWNLOAD_WAIT_CEILING_MS);
        }
        self.default_timeout_ms.unwrap_or(default_timeout_ms)
    }
}

/// Look up a tool by name.
pub fn lookup(name: &str) -> Option<&'static ToolSpec> {
    TOOLS.iter().find(|spec| spec.name == name)
}

/// Names of every tool, in declaration order.
pub fn names() -> Vec<&'static str> {
    TOOLS.iter().map(|spec| spec.name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_table_is_lookup_able() {
        assert_eq!(names().len(), 26);
        for spec in TOOLS {
            assert_eq!(lookup(spec.name).map(|found| found.name), Some(spec.name));
        }
        assert!(lookup("browser_teleport").is_none());
    }

    #[test]
    fn capabilities_match_the_tool_family() {
        for spec in TOOLS {
            match spec.name {
                "tab_list" | "tab_new" | "tab_close" | "tab_switch" | "browser_list"
                | "net_capture" | "net_log" | "net_clear" | "net_intercept" | "download_list"
                | "download_wait" | "upload_file" => {
                    assert_eq!(spec.capability, None, "{} needs no DOM", spec.name);
                }
                _ => assert_eq!(
                    spec.capability,
                    Some(capability::DOM),
                    "{} needs the DOM capability",
                    spec.name
                ),
            }
        }
    }

    #[test]
    fn serial_flags_separate_reads_from_actions() {
        for (name, serial) in [
            ("browser_navigate", true),
            ("page_snapshot", false),
            ("browser_click", true),
            ("browser_type", true),
            ("page_text", false),
            ("page_html", false),
            ("page_find", false),
            ("browser_hover", true),
            ("browser_press", true),
            ("browser_scroll", true),
            ("browser_select", true),
            ("browser_eval", true),
            ("browser_wait", false),
            ("browser_history", true),
            ("tab_list", false),
            ("tab_new", true),
            ("tab_close", true),
            ("tab_switch", true),
            ("browser_list", false),
            ("net_capture", true),
            ("net_log", false),
            ("net_clear", true),
            ("net_intercept", true),
            ("download_list", false),
            ("download_wait", false),
            ("upload_file", true),
        ] {
            let spec = lookup(name).expect("known tool");
            assert_eq!(spec.serial, serial, "wrong serial flag for {name}");
        }
    }

    #[test]
    fn timeouts_follow_the_tool_policy() {
        let click = lookup("browser_click").expect("click");
        assert_eq!(click.timeout_ms(&json!({}), 60_000), 60_000);
        let eval = lookup("browser_eval").expect("eval");
        assert_eq!(eval.timeout_ms(&json!({}), 60_000), 30_000);
        let wait = lookup("browser_wait").expect("wait");
        assert_eq!(wait.timeout_ms(&json!({}), 60_000), 15_000);
        assert_eq!(
            wait.timeout_ms(&json!({ "timeout_ms": 120_000 }), 60_000),
            125_000
        );
        assert_eq!(
            wait.timeout_ms(&json!({ "timeout_ms": 100 }), 60_000),
            5_100
        );
        let capture = lookup("net_capture").expect("capture");
        assert_eq!(capture.timeout_ms(&json!({}), 5_000), 60_000);
        let downloads = lookup("download_list").expect("downloads");
        assert_eq!(downloads.timeout_ms(&json!({}), 5_000), 60_000);
        let download_wait = lookup("download_wait").expect("download_wait");
        assert_eq!(download_wait.timeout_ms(&json!({}), 60_000), 35_000);
        assert_eq!(
            download_wait.timeout_ms(&json!({ "timeout_ms": 120_000 }), 60_000),
            125_000
        );
        assert_eq!(
            download_wait.timeout_ms(&json!({ "timeout_ms": 100 }), 60_000),
            5_100
        );
        let browser_list = lookup("browser_list").expect("browser_list");
        assert_eq!(browser_list.timeout_ms(&json!({}), 5_000), 60_000);
    }
}
