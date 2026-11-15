//! Pure routing, batching, snapshot, timeout, retention and auth policies.

use crate::error::PolicyError;

/// The operator-selected browser automation channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelMode {
    /// Use CDP when connected, otherwise fall back to the extension.
    Auto,
    /// Require the CDP transport.
    Cdp,
    /// Require the extension transport.
    Extension,
}

impl ChannelMode {
    /// Parses the CLI spelling of a channel mode.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(Self::Auto),
            "cdp" => Some(Self::Cdp),
            "extension" => Some(Self::Extension),
            _ => None,
        }
    }

    /// Returns the stable CLI spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Cdp => "cdp",
            Self::Extension => "extension",
        }
    }
}

/// Which transport can serve a browser-facing tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolFamily {
    /// Only the CDP transport implements the tool.
    CdpOnly,
    /// Only the extension transport implements the tool.
    ExtensionOnly,
    /// Both transports implement the tool.
    Shared,
    /// A daemon-local tool that never reaches a browser.
    Local,
}

/// The selected route for a browser-facing tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// Use the CDP automation adapter.
    Cdp,
    /// Use the extension gateway.
    Extension,
    /// No route is available for the requested channel.
    Rejected,
}

/// Selects a transport for an MCP tool without performing I/O.
pub fn route_tool(mode: ChannelMode, family: ToolFamily, cdp_connected: bool) -> Route {
    match family {
        ToolFamily::CdpOnly => {
            if mode == ChannelMode::Extension {
                Route::Rejected
            } else if cdp_connected || mode == ChannelMode::Cdp {
                Route::Cdp
            } else {
                Route::Rejected
            }
        }
        ToolFamily::ExtensionOnly => {
            if mode == ChannelMode::Cdp {
                Route::Rejected
            } else {
                Route::Extension
            }
        }
        ToolFamily::Shared => match mode {
            ChannelMode::Extension => Route::Extension,
            ChannelMode::Cdp => Route::Cdp,
            ChannelMode::Auto if cdp_connected => Route::Cdp,
            ChannelMode::Auto => Route::Extension,
        },
        ToolFamily::Local => Route::Rejected,
    }
}

/// A CDP-looking tab id/target id is exactly 32 hexadecimal characters.
pub const EXTENSION_TAB_HINT: &str =
    "omit tab_id to use the active tab; CDP target ids are not extension tab ids";
/// An extension tab id is a non-negative integer and never a CDP target id.
pub const CDP_TAB_HINT: &str =
    "omit tab_id to use the active tab; extension tab ids are not CDP target ids";

/// Classification of a tab identifier observed at a channel boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetIdKind {
    /// A 32-hex-character CDP target id.
    CdpTarget,
    /// A non-negative integer extension tab id.
    ExtensionTab,
    /// A string that is neither shape (for example a CSS selector).
    Other,
}

/// Classifies a string identifier using the same rules as the daemon guard.
pub fn classify_target_id(value: &str) -> TargetIdKind {
    if value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        TargetIdKind::CdpTarget
    } else if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
        TargetIdKind::ExtensionTab
    } else {
        TargetIdKind::Other
    }
}

/// Browser batch sequencing policy.
pub struct BatchPolicy;

impl BatchPolicy {
    /// Maximum number of steps accepted by browser_batch.
    pub const MAX_STEPS: usize = 20;
    /// Tools that may never run inside a batch.
    pub const EXCLUDED_TOOLS: [&'static str; 4] =
        ["browser_batch", "browser_channel", "metrics", "logs_tail"];
    /// Tool categories that are safe to batch.
    pub const ALLOWED_CATEGORIES: [&'static str; 7] = [
        "page",
        "input",
        "tabs",
        "network",
        "files",
        "cdp",
        "diagnostics",
    ];

    /// Validates the requested step count.
    pub fn validate_count(count: usize) -> Result<(), PolicyError> {
        if count == 0 {
            return Err(PolicyError::Empty { field: "steps" });
        }
        if count > Self::MAX_STEPS {
            return Err(PolicyError::TooManySteps {
                max: Self::MAX_STEPS,
                actual: count,
            });
        }
        Ok(())
    }

    /// Returns whether a catalogued tool may run inside a batch.
    pub fn allows(category: &str, tool: &str) -> bool {
        Self::ALLOWED_CATEGORIES.contains(&category) && !Self::EXCLUDED_TOOLS.contains(&tool)
    }
}

/// Snapshot rendering boundaries.
pub struct SnapshotPolicy;

impl SnapshotPolicy {
    /// Default maximum emitted nodes.
    pub const DEFAULT_MAX_NODES: i64 = 300;
    /// Lowest accepted maximum.
    pub const MIN_MAX_NODES: i64 = 40;
    /// Highest accepted maximum.
    pub const MAX_MAX_NODES: i64 = 3_000;

    /// Clamps a requested node cap into the contract range.
    pub fn clamp_max_nodes(requested: i64) -> i64 {
        requested.clamp(Self::MIN_MAX_NODES, Self::MAX_MAX_NODES)
    }

    /// Validates a node cap without clamping.
    pub fn validate_max_nodes(requested: i64) -> Result<(), PolicyError> {
        if !(Self::MIN_MAX_NODES..=Self::MAX_MAX_NODES).contains(&requested) {
            return Err(PolicyError::OutOfRange {
                field: "max_nodes",
                min: Self::MIN_MAX_NODES,
                max: Self::MAX_MAX_NODES,
                actual: requested,
            });
        }
        Ok(())
    }
}

/// Per-command timeout boundaries.
pub struct TimeoutPolicy;

impl TimeoutPolicy {
    /// Default command timeout in milliseconds.
    pub const DEFAULT_MS: u64 = 60_000;
    /// Maximum command timeout in milliseconds.
    pub const MAX_MS: u64 = 125_000;

    /// Clamps a requested timeout into the accepted range.
    pub fn clamp(requested_ms: u64) -> u64 {
        requested_ms.clamp(1, Self::MAX_MS)
    }
}

/// Recording retention and payload truncation policy.
pub struct RetentionPolicy;

impl RetentionPolicy {
    /// Maximum serialized payload bytes retained per step.
    pub const MAX_PAYLOAD_BYTES: usize = 262_144;
    /// Retention window for sessions and steps.
    pub const RETENTION_MS: i64 = 30 * 24 * 60 * 60 * 1_000;

    /// Returns whether a timestamp is older than the retention window.
    pub fn is_expired(timestamp_ms: i64, now_ms: i64) -> bool {
        now_ms.saturating_sub(timestamp_ms) > Self::RETENTION_MS
    }

    /// Truncates a string to the payload cap on a UTF-8 character boundary.
    pub fn truncate_payload(value: &str) -> (String, bool) {
        if value.len() <= Self::MAX_PAYLOAD_BYTES {
            return (value.to_string(), false);
        }
        let mut end = Self::MAX_PAYLOAD_BYTES;
        while end > 0 && !value.is_char_boundary(end) {
            end -= 1;
        }
        (value[..end].to_string(), true)
    }
}

/// Authentication mode for the HTTP surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    /// Local loopback-only operation without tokens.
    Off,
    /// Local username/password plus JWT access and refresh tokens.
    Local,
}

impl AuthMode {
    /// Parses the configuration spelling.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "off" => Some(Self::Off),
            "local" => Some(Self::Local),
            _ => None,
        }
    }

    /// Returns the stable configuration spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Local => "local",
        }
    }
}

/// Authentication defaults and startup guards.
pub struct AuthPolicy;

impl AuthPolicy {
    /// Rejects a non-loopback bind when authentication is disabled.
    pub fn validate(mode: AuthMode, bind: &str, is_loopback: bool) -> Result<(), PolicyError> {
        if mode == AuthMode::Off && !is_loopback {
            return Err(PolicyError::InsecureBinding {
                bind: bind.to_string(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_names_round_trip() {
        for mode in [ChannelMode::Auto, ChannelMode::Cdp, ChannelMode::Extension] {
            assert_eq!(ChannelMode::parse(mode.as_str()), Some(mode));
        }
        assert_eq!(ChannelMode::parse("bogus"), None);
    }

    #[test]
    fn routing_covers_forced_and_auto_channels() {
        assert_eq!(
            route_tool(ChannelMode::Auto, ToolFamily::Shared, true),
            Route::Cdp
        );
        assert_eq!(
            route_tool(ChannelMode::Auto, ToolFamily::Shared, false),
            Route::Extension
        );
        assert_eq!(
            route_tool(ChannelMode::Cdp, ToolFamily::Shared, false),
            Route::Cdp
        );
        assert_eq!(
            route_tool(ChannelMode::Extension, ToolFamily::Shared, true),
            Route::Extension
        );
        assert_eq!(
            route_tool(ChannelMode::Cdp, ToolFamily::CdpOnly, false),
            Route::Cdp
        );
        assert_eq!(
            route_tool(ChannelMode::Extension, ToolFamily::CdpOnly, true),
            Route::Rejected
        );
        assert_eq!(
            route_tool(ChannelMode::Cdp, ToolFamily::ExtensionOnly, true),
            Route::Rejected
        );
        assert_eq!(
            route_tool(ChannelMode::Auto, ToolFamily::Local, true),
            Route::Rejected
        );
    }

    #[test]
    fn namespace_classification_distinguishes_shapes() {
        assert_eq!(
            classify_target_id("0123456789abcdef0123456789abcdef"),
            TargetIdKind::CdpTarget
        );
        assert_eq!(classify_target_id("7"), TargetIdKind::ExtensionTab);
        assert_eq!(classify_target_id("#main"), TargetIdKind::Other);
        assert_eq!(classify_target_id(""), TargetIdKind::Other);
    }

    #[test]
    fn batch_policy_enforces_bounds_and_allowlist() {
        assert!(BatchPolicy::validate_count(20).is_ok());
        assert!(BatchPolicy::validate_count(21).is_err());
        assert!(BatchPolicy::validate_count(0).is_err());
        assert!(BatchPolicy::allows("page", "page_snapshot"));
        assert!(!BatchPolicy::allows("page", "browser_batch"));
        assert!(!BatchPolicy::allows("help", "tool_help"));
    }

    #[test]
    fn snapshot_and_timeout_clamp() {
        assert_eq!(SnapshotPolicy::clamp_max_nodes(1), 40);
        assert_eq!(SnapshotPolicy::clamp_max_nodes(10_000), 3_000);
        assert!(SnapshotPolicy::validate_max_nodes(39).is_err());
        assert_eq!(TimeoutPolicy::clamp(0), 1);
        assert_eq!(TimeoutPolicy::clamp(999_999), TimeoutPolicy::MAX_MS);
    }

    #[test]
    fn retention_truncates_on_a_character_boundary() {
        let input = format!("{}é", "a".repeat(RetentionPolicy::MAX_PAYLOAD_BYTES - 1));
        let (payload, truncated) = RetentionPolicy::truncate_payload(&input);
        assert!(truncated);
        assert!(payload.len() <= RetentionPolicy::MAX_PAYLOAD_BYTES);
        assert!(payload.is_char_boundary(payload.len()));
    }

    #[test]
    fn auth_off_is_rejected_for_non_loopback() {
        assert!(AuthPolicy::validate(AuthMode::Off, "127.0.0.1", true).is_ok());
        assert!(AuthPolicy::validate(AuthMode::Off, "0.0.0.0", false).is_err());
        assert!(AuthPolicy::validate(AuthMode::Local, "0.0.0.0", false).is_ok());
    }
}
