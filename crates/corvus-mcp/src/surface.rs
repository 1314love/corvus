//! Dynamic tool surface used by the v2 capability mode.
//!
//! The legacy catalogue is static. This surface is derived from the live
//! browser graph, so a session only sees groups that can actually run. The
//! daemon implements it; the MCP layer only needs to know how to list and
//! validate against it and when its effective tool list changed.

use serde_json::{json, Value};

use corvus_proto::{ErrorBody, ErrorCode};

/// Instructions shown when the dynamic surface is active.
pub const SURFACE_INSTRUCTIONS: &str =
    "Browser tools appear when a browser session is connected. Use browser_status to see available groups.";

/// Largest text accepted by `element_type`.
const MAX_TYPE_UNITS: usize = 20_000;
/// Inclusive bounds mirrored from the frozen tool contract.
const MAX_PAGE_CHARS: u64 = 200_000;
const MIN_PAGE_CHARS: u64 = 1_000;
const MAX_SNAPSHOT_NODES: u64 = 2_000;
const MIN_SNAPSHOT_NODES: u64 = 50;
const MAX_NAVIGATION_MS: u64 = 60_000;
const MIN_NAVIGATION_MS: u64 = 1_000;

/// One live tool surface.
pub trait ToolSurface: Send + Sync {
    /// Initialize instructions for a dynamic session.
    fn instructions(&self) -> &'static str;

    /// Definitions currently visible to a client.
    fn tools(&self) -> Vec<Value>;

    /// Validates and normalises arguments for one visible tool.
    fn validate(&self, name: &str, arguments: &Value) -> Result<Value, ErrorBody>;

    /// A cheap fingerprint of the effective tool list. It changes only when
    /// the list a client sees changes, never when a provider reconnects with
    /// identical capabilities.
    fn signature(&self) -> String;
}

/// The frozen public tool order. Input follows page because clicking a
/// reference needs a snapshot, and the frozen contract lists input only when
/// the page group is also available.
pub const PUBLIC_TOOLS: [&str; 10] = [
    "browser_status",
    "tab_list",
    "tab_open",
    "tab_close",
    "page_navigate",
    "page_read",
    "page_snapshot",
    "element_click",
    "element_type",
    "page_screenshot",
];

/// Capability groups in public order.
pub const GROUPS: [&str; 4] = ["tabs", "page", "input", "capture"];

/// Group each tool belongs to, except `browser_status`.
pub fn tool_group(name: &str) -> Option<&'static str> {
    match name {
        "tab_list" | "tab_open" | "tab_close" => Some("tabs"),
        "page_navigate" | "page_read" | "page_snapshot" => Some("page"),
        "element_click" | "element_type" => Some("input"),
        "page_screenshot" => Some("capture"),
        _ => None,
    }
}

/// Whether a group is complete under the frozen rules.
///
/// Input always needs page because it consumes references produced by a
/// snapshot. Every other group is self-contained.
pub fn group_visible(group: &str, enabled: &[&str]) -> bool {
    match group {
        "input" => enabled.contains(&"input") && enabled.contains(&"page"),
        "tabs" | "page" | "capture" => enabled.contains(&group),
        _ => false,
    }
}

/// Build the definition for one public tool.
pub fn definition(name: &str) -> Value {
    let mut properties = serde_json::Map::new();
    if name != "browser_status" {
        properties.insert(
            "browser_id".to_string(),
            json!({
                "type": "string",
                "pattern": "^b-[0-9a-f]{8}$",
                "description": "Browser to use when more than one is connected."
            }),
        );
    }
    match name {
        "browser_status" => {
            properties.insert(
                "browser_id".to_string(),
                json!({
                    "type": "string",
                    "pattern": "^b-[0-9a-f]{8}$",
                    "description": "Limit the report to one browser."
                }),
            );
        }
        "tab_open" => {
            properties.insert(
                "url".to_string(),
                json!({ "type": "string", "description": "Address to open." }),
            );
            properties.insert(
                "active".to_string(),
                json!({ "type": "boolean", "default": true }),
            );
        }
        "tab_close" => {
            properties.insert(
                "tab_id".to_string(),
                json!({ "type": "string", "minLength": 1, "maxLength": 64 }),
            );
        }
        "page_navigate" => {
            properties.insert(
                "url".to_string(),
                json!({ "type": "string", "description": "Address to open." }),
            );
            properties.insert(
                "tab_id".to_string(),
                json!({ "type": "string", "minLength": 1, "maxLength": 64 }),
            );
            properties.insert(
                "wait_until".to_string(),
                json!({ "type": "string", "enum": ["load", "domcontentloaded"], "default": "load" }),
            );
            properties.insert(
                "timeout_ms".to_string(),
                json!({ "type": "integer", "minimum": MIN_NAVIGATION_MS, "maximum": MAX_NAVIGATION_MS, "default": 30_000 }),
            );
        }
        "page_read" => {
            properties.insert(
                "tab_id".to_string(),
                json!({ "type": "string", "minLength": 1, "maxLength": 64 }),
            );
            properties.insert(
                "mode".to_string(),
                json!({ "type": "string", "enum": ["text", "html"], "default": "text" }),
            );
            properties.insert(
                "selector".to_string(),
                json!({ "type": "string", "minLength": 1, "maxLength": 2_000 }),
            );
            properties.insert(
                "max_chars".to_string(),
                json!({ "type": "integer", "minimum": MIN_PAGE_CHARS, "maximum": MAX_PAGE_CHARS, "default": 20_000 }),
            );
        }
        "page_snapshot" => {
            properties.insert(
                "tab_id".to_string(),
                json!({ "type": "string", "minLength": 1, "maxLength": 64 }),
            );
            properties.insert(
                "max_nodes".to_string(),
                json!({ "type": "integer", "minimum": MIN_SNAPSHOT_NODES, "maximum": MAX_SNAPSHOT_NODES, "default": 500 }),
            );
            properties.insert(
                "include_hidden".to_string(),
                json!({ "type": "boolean", "default": false }),
            );
        }
        "element_click" => {
            properties.insert(
                "ref".to_string(),
                json!({ "type": "string", "pattern": "^r-[0-9a-f]{8}$" }),
            );
            properties.insert(
                "tab_id".to_string(),
                json!({ "type": "string", "minLength": 1, "maxLength": 64 }),
            );
        }
        "element_type" => {
            properties.insert(
                "ref".to_string(),
                json!({ "type": "string", "pattern": "^r-[0-9a-f]{8}$" }),
            );
            properties.insert(
                "text".to_string(),
                json!({ "type": "string", "description": "Text to enter, at most 20000 UTF-16 code units." }),
            );
            properties.insert(
                "clear".to_string(),
                json!({ "type": "boolean", "default": true }),
            );
            properties.insert(
                "submit".to_string(),
                json!({ "type": "boolean", "default": false }),
            );
            properties.insert(
                "tab_id".to_string(),
                json!({ "type": "string", "minLength": 1, "maxLength": 64 }),
            );
        }
        "page_screenshot" => {
            properties.insert(
                "tab_id".to_string(),
                json!({ "type": "string", "minLength": 1, "maxLength": 64 }),
            );
            properties.insert(
                "format".to_string(),
                json!({ "type": "string", "enum": ["png", "jpeg"], "default": "png" }),
            );
            properties.insert(
                "quality".to_string(),
                json!({ "type": "integer", "minimum": 1, "maximum": 100, "default": 80 }),
            );
            properties.insert(
                "full_page".to_string(),
                json!({ "type": "boolean", "default": false }),
            );
            properties.insert(
                "inline".to_string(),
                json!({ "type": "boolean", "default": false }),
            );
        }
        _ => {}
    }
    let required = match name {
        "tab_open" | "page_navigate" => json!(["url"]),
        "tab_close" => json!(["tab_id"]),
        "element_click" => json!(["ref"]),
        "element_type" => json!(["ref", "text"]),
        _ => json!([]),
    };
    json!({
        "name": name,
        "description": description(name),
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false,
        },
    })
}

fn description(name: &str) -> &'static str {
    match name {
        "browser_status" => "Report connected browsers and available capability groups.",
        "tab_list" => "List open tabs.",
        "tab_open" => "Open a tab.",
        "tab_close" => "Close a tab.",
        "page_navigate" => "Navigate a page.",
        "page_read" => "Read page text or HTML.",
        "page_snapshot" => "Snapshot interactive page elements and assign references.",
        "element_click" => "Click an element reference from page_snapshot.",
        "element_type" => "Type into an element reference from page_snapshot.",
        "page_screenshot" => "Capture a page screenshot.",
        _ => "Browser tool.",
    }
}

/// Validate a tool call against the frozen P2 argument contract.
///
/// `enabled` names the groups currently available. The returned value contains
/// only the fields the internal action understands, with every default filled.
pub fn validate_args(name: &str, arguments: &Value, enabled: &[&str]) -> Result<Value, ErrorBody> {
    let Some(group) = tool_group(name) else {
        if name == "browser_status" {
            return validate_browser_status(arguments, enabled);
        }
        return Err(bad_args(format!("unknown tool: {name}")));
    };
    if !PUBLIC_TOOLS.contains(&name) {
        return Err(bad_args(format!("unknown tool: {name}")));
    }
    if !group_visible(group, enabled) || (group == "input" && !enabled.contains(&"page")) {
        return Err(capability_unavailable(format!(
            "{name} is not currently available"
        )));
    }
    let object = arguments
        .as_object()
        .ok_or_else(|| bad_args("arguments must be an object"))?;
    let allowed: &[&str] = match name {
        "tab_list" => &["browser_id"],
        "tab_open" => &["url", "active", "browser_id"],
        "tab_close" => &["tab_id", "browser_id"],
        "page_navigate" => &["url", "tab_id", "wait_until", "timeout_ms", "browser_id"],
        "page_read" => &["tab_id", "mode", "selector", "max_chars", "browser_id"],
        "page_snapshot" => &["tab_id", "max_nodes", "include_hidden", "browser_id"],
        "element_click" => &["ref", "tab_id", "browser_id"],
        "element_type" => &["ref", "text", "clear", "submit", "tab_id", "browser_id"],
        "page_screenshot" => &[
            "tab_id",
            "format",
            "quality",
            "full_page",
            "inline",
            "browser_id",
        ],
        _ => &[],
    };
    reject_unknown(object, allowed)?;
    let mut out = serde_json::Map::new();
    if let Some(browser_id) = optional_string(object, "browser_id")? {
        if !is_browser_id(&browser_id) {
            return Err(bad_args(
                "browser_id must look like b- followed by 8 lowercase hex digits",
            ));
        }
        out.insert("browser_id".to_string(), json!(browser_id));
    }
    match name {
        "tab_list" => {}
        "tab_open" => {
            out.insert("url".to_string(), json!(required_url(object, "url")?));
            out.insert(
                "active".to_string(),
                json!(optional_bool(object, "active", true)?),
            );
        }
        "tab_close" => {
            out.insert(
                "tab_id".to_string(),
                json!(required_tab_id(object, "tab_id")?),
            );
        }
        "page_navigate" => {
            out.insert("url".to_string(), json!(required_url(object, "url")?));
            if let Some(tab_id) = optional_string(object, "tab_id")? {
                out.insert("tab_id".to_string(), json!(tab_id));
            }
            let wait = optional_string(object, "wait_until")?.unwrap_or_else(|| "load".to_string());
            if wait != "load" && wait != "domcontentloaded" {
                return Err(bad_args("wait_until must be load or domcontentloaded"));
            }
            out.insert("wait_until".to_string(), json!(wait));
            out.insert(
                "timeout_ms".to_string(),
                json!(optional_integer(
                    object,
                    "timeout_ms",
                    30_000,
                    MIN_NAVIGATION_MS,
                    MAX_NAVIGATION_MS
                )?),
            );
        }
        "page_read" => {
            if let Some(tab_id) = optional_string(object, "tab_id")? {
                out.insert("tab_id".to_string(), json!(tab_id));
            }
            let mode = optional_string(object, "mode")?.unwrap_or_else(|| "text".to_string());
            if mode != "text" && mode != "html" {
                return Err(bad_args("mode must be text or html"));
            }
            out.insert("mode".to_string(), json!(mode));
            if let Some(selector) = optional_string(object, "selector")? {
                if selector.trim().is_empty() {
                    return Err(bad_args("selector must not be empty"));
                }
                if selector.chars().count() > 2_000 {
                    return Err(bad_args("selector must be at most 2000 characters"));
                }
                out.insert("selector".to_string(), json!(selector));
            }
            out.insert(
                "max_chars".to_string(),
                json!(optional_integer(
                    object,
                    "max_chars",
                    20_000,
                    MIN_PAGE_CHARS,
                    MAX_PAGE_CHARS
                )?),
            );
        }
        "page_snapshot" => {
            if let Some(tab_id) = optional_string(object, "tab_id")? {
                out.insert("tab_id".to_string(), json!(tab_id));
            }
            out.insert(
                "max_nodes".to_string(),
                json!(optional_integer(
                    object,
                    "max_nodes",
                    500,
                    MIN_SNAPSHOT_NODES,
                    MAX_SNAPSHOT_NODES
                )?),
            );
            out.insert(
                "include_hidden".to_string(),
                json!(optional_bool(object, "include_hidden", false)?),
            );
        }
        "element_click" => {
            let reference = required_string(object, "ref")?;
            if !is_ref(&reference) {
                return Err(bad_args(
                    "ref must look like r- followed by 8 lowercase hex digits",
                ));
            }
            out.insert("ref".to_string(), json!(reference));
            if let Some(tab_id) = optional_string(object, "tab_id")? {
                out.insert("tab_id".to_string(), json!(tab_id));
            }
        }
        "element_type" => {
            let reference = required_string(object, "ref")?;
            if !is_ref(&reference) {
                return Err(bad_args(
                    "ref must look like r- followed by 8 lowercase hex digits",
                ));
            }
            out.insert("ref".to_string(), json!(reference));
            let text = required_string(object, "text")?;
            if text.encode_utf16().count() > MAX_TYPE_UNITS {
                return Err(bad_args("text must be at most 20000 UTF-16 code units"));
            }
            out.insert("text".to_string(), json!(text));
            out.insert(
                "clear".to_string(),
                json!(optional_bool(object, "clear", true)?),
            );
            out.insert(
                "submit".to_string(),
                json!(optional_bool(object, "submit", false)?),
            );
            if let Some(tab_id) = optional_string(object, "tab_id")? {
                out.insert("tab_id".to_string(), json!(tab_id));
            }
        }
        "page_screenshot" => {
            if let Some(tab_id) = optional_string(object, "tab_id")? {
                out.insert("tab_id".to_string(), json!(tab_id));
            }
            let format = optional_string(object, "format")?.unwrap_or_else(|| "png".to_string());
            if format != "png" && format != "jpeg" {
                return Err(bad_args("format must be png or jpeg"));
            }
            out.insert("format".to_string(), json!(format));
            out.insert(
                "quality".to_string(),
                json!(optional_integer(object, "quality", 80, 1, 100)?),
            );
            out.insert(
                "full_page".to_string(),
                json!(optional_bool(object, "full_page", false)?),
            );
            out.insert(
                "inline".to_string(),
                json!(optional_bool(object, "inline", false)?),
            );
        }
        _ => unreachable!(),
    }
    Ok(Value::Object(out))
}

fn validate_browser_status(arguments: &Value, enabled: &[&str]) -> Result<Value, ErrorBody> {
    let object = arguments
        .as_object()
        .ok_or_else(|| bad_args("arguments must be an object"))?;
    reject_unknown(object, &["browser_id"])?;
    let mut out = serde_json::Map::new();
    if let Some(browser_id) = optional_string(object, "browser_id")? {
        if !is_browser_id(&browser_id) {
            return Err(bad_args(
                "browser_id must look like b- followed by 8 lowercase hex digits",
            ));
        }
        out.insert("browser_id".to_string(), json!(browser_id));
    }
    let _ = enabled;
    Ok(Value::Object(out))
}

fn reject_unknown(
    object: &serde_json::Map<String, Value>,
    allowed: &[&str],
) -> Result<(), ErrorBody> {
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(bad_args(format!("unknown argument: {key}")));
    }
    Ok(())
}

fn optional_string(
    object: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<String>, ErrorBody> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(bad_args(format!("{key} must be a string"))),
    }
}

fn required_string(
    object: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<String, ErrorBody> {
    optional_string(object, key)?.ok_or_else(|| bad_args(format!("{key} is required")))
}

fn required_tab_id(
    object: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<String, ErrorBody> {
    let value = required_string(object, key)?;
    if value.is_empty() || value.chars().count() > 64 {
        return Err(bad_args(format!(
            "{key} must be between 1 and 64 characters"
        )));
    }
    Ok(value)
}

fn required_url(object: &serde_json::Map<String, Value>, key: &str) -> Result<String, ErrorBody> {
    let value = required_string(object, key)?;
    if !is_web_url(&value) {
        return Err(bad_args(
            "url must be an http, https or about:blank address",
        ));
    }
    Ok(value)
}

fn optional_bool(
    object: &serde_json::Map<String, Value>,
    key: &str,
    default: bool,
) -> Result<bool, ErrorBody> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(bad_args(format!("{key} must be a boolean"))),
    }
}

fn optional_integer(
    object: &serde_json::Map<String, Value>,
    key: &str,
    default: u64,
    minimum: u64,
    maximum: u64,
) -> Result<u64, ErrorBody> {
    let value = match object.get(key) {
        None | Some(Value::Null) => default,
        Some(Value::Number(number)) => number
            .as_u64()
            .ok_or_else(|| bad_args(format!("{key} must be an integer")))?,
        Some(_) => return Err(bad_args(format!("{key} must be an integer"))),
    };
    if value < minimum || value > maximum {
        return Err(bad_args(format!(
            "{key} must be between {minimum} and {maximum}"
        )));
    }
    Ok(value)
}

/// Whether a string is an accepted navigation target.
pub fn is_web_url(value: &str) -> bool {
    if value.eq_ignore_ascii_case("about:blank") {
        return true;
    }
    let Ok(parsed) = url::Url::parse(value) else {
        return false;
    };
    matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some()
}

/// Opaque reference shape used on the public surface.
pub fn is_ref(value: &str) -> bool {
    let rest = value.strip_prefix("r-").unwrap_or("");
    rest.len() == 8
        && rest
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// Public browser identifier shape.
pub fn is_browser_id(value: &str) -> bool {
    let rest = value.strip_prefix("b-").unwrap_or("");
    rest.len() == 8
        && rest
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn bad_args(message: impl Into<String>) -> ErrorBody {
    ErrorBody::new(ErrorCode::BadArgs, message)
}

fn capability_unavailable(message: impl Into<String>) -> ErrorBody {
    ErrorBody::new(ErrorCode::CapabilityUnavailable, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_declares_exact_required_arguments() {
        let cases: [(&str, &[&str]); 10] = [
            ("browser_status", &[]),
            ("tab_list", &[]),
            ("tab_open", &["url"]),
            ("tab_close", &["tab_id"]),
            ("page_navigate", &["url"]),
            ("page_read", &[]),
            ("page_snapshot", &[]),
            ("element_click", &["ref"]),
            ("element_type", &["ref", "text"]),
            ("page_screenshot", &[]),
        ];
        assert_eq!(cases.len(), PUBLIC_TOOLS.len());
        for (name, required) in cases {
            let definition = definition(name);
            assert_eq!(definition["name"], name);
            assert_eq!(
                definition["inputSchema"]["required"],
                json!(required),
                "required mismatch for {name}"
            );
        }
    }
}
