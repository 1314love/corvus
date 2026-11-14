//! The tool surface the daemon accepts from agents.

use corvus_proto::capability;

/// One tool the router knows how to forward to a browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolSpec {
    /// Stable tool name.
    pub name: &'static str,
    /// Whether two calls may not overlap for the same browser.
    pub serial: bool,
    /// Capability the browser must offer before the tool is forwarded.
    pub capability: &'static str,
}

/// Every tool exposed in this version.
pub const TOOLS: [ToolSpec; 4] = [
    ToolSpec {
        name: "browser_navigate",
        serial: true,
        capability: capability::DOM,
    },
    ToolSpec {
        name: "page_snapshot",
        serial: false,
        capability: capability::DOM,
    },
    ToolSpec {
        name: "browser_click",
        serial: true,
        capability: capability::DOM,
    },
    ToolSpec {
        name: "browser_type",
        serial: true,
        capability: capability::DOM,
    },
];

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

    #[test]
    fn tool_table_is_lookup_able() {
        assert_eq!(names().len(), 4);
        for spec in TOOLS {
            assert_eq!(lookup(spec.name).map(|found| found.name), Some(spec.name));
            assert!(spec.capability == capability::DOM);
        }
        assert!(lookup("browser_teleport").is_none());
        assert!(lookup("browser_click")
            .map(|spec| spec.serial)
            .unwrap_or(false));
        assert!(lookup("page_snapshot")
            .map(|spec| !spec.serial)
            .unwrap_or(false));
    }
}
