//! Tool catalogue: names, JSON schemas, descriptions and MCP annotations.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use corvus_proto::{ErrorBody, ErrorCode};
use serde_json::{json, Map, Value};

struct Spec {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    schema: fn() -> Value,
    read_only: bool,
    destructive: bool,
    idempotent: bool,
    open_world: bool,
}

const SPECS: [Spec; 26] = [
    Spec {
        name: "browser_navigate",
        title: "Navigate",
        description: "Open an absolute URL in a tab of the connected browser and wait for the new document to finish loading. The result reports loaded (true when the load completed within the wait budget, false when the page was still busy) and url (the final URL after redirects). A page_snapshot taken after this call reflects the new document.",
        schema: navigate_schema,
        read_only: false,
        destructive: false,
        idempotent: false,
        open_world: true,
    },
    Spec {
        name: "page_snapshot",
        title: "Page snapshot",
        description: "Return a compact tree of the page. Element references from the snapshot are the values browser_click and browser_type accept as target.",
        schema: snapshot_schema,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: true,
    },
    Spec {
        name: "browser_click",
        title: "Click",
        description: "Click the element that target refers to. Fails with NOT_INTERACTABLE when the element is covered or disabled.",
        schema: click_schema,
        read_only: false,
        destructive: true,
        idempotent: false,
        open_world: true,
    },
    Spec {
        name: "browser_type",
        title: "Type",
        description: "Focus the element that target refers to and replace its text with the given value.",
        schema: type_schema,
        read_only: false,
        destructive: true,
        idempotent: false,
        open_world: true,
    },
    Spec {
        name: "page_text",
        title: "Page text",
        description: "Return the visible text of a page, capped by max_chars. The result reports the page url and title, the text and whether the text was truncated.",
        schema: page_text_schema,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: true,
    },
    Spec {
        name: "page_html",
        title: "Page HTML",
        description: "Return the HTML of the document, or of one element when target is given, capped by max_chars. The result reports the page url, the html and whether it was truncated.",
        schema: page_html_schema,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: true,
    },
    Spec {
        name: "page_find",
        title: "Find in page",
        description: "Search the page text, optionally filtered by accessibility role, and return element references that browser_click accepts. The references share the page_snapshot reference space.",
        schema: page_find_schema,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: true,
    },
    Spec {
        name: "browser_hover",
        title: "Hover",
        description: "Move the pointer over the element that target refers to and report the element it resolved.",
        schema: hover_schema,
        read_only: false,
        destructive: false,
        idempotent: true,
        open_world: true,
    },
    Spec {
        name: "browser_press",
        title: "Press keys",
        description: "Send one key or a sequence of keys to the page or to a target element. Modifiers use Control+, Shift+, Alt+ or Meta+ prefixes.",
        schema: press_schema,
        read_only: false,
        destructive: true,
        idempotent: false,
        open_world: true,
    },
    Spec {
        name: "browser_scroll",
        title: "Scroll",
        description: "Scroll the page or a target element by a number of viewport pages and report the resulting scroll offset.",
        schema: scroll_schema,
        read_only: false,
        destructive: false,
        idempotent: false,
        open_world: true,
    },
    Spec {
        name: "browser_select",
        title: "Select options",
        description: "Choose one or more values in a select element. The result reports the selected values and the current value.",
        schema: select_schema,
        read_only: false,
        destructive: true,
        idempotent: false,
        open_world: true,
    },
    Spec {
        name: "browser_eval",
        title: "Evaluate script",
        description: "Evaluate a JavaScript expression through the page debugger channel. The world defaults to \"isolated\", which uses a dedicated isolated world; \"main\" uses the page main world. Page CSP does not restrict the expression. A page exception is a successful call with {ok:false,error:{name,message}}, not a tool error.",
        schema: eval_schema,
        read_only: false,
        destructive: true,
        idempotent: false,
        open_world: true,
    },
    Spec {
        name: "browser_wait",
        title: "Wait for condition",
        description: "Wait until a selector, text or URL condition matches, up to timeout_ms. The timeout is reported as the TIMEOUT error code.",
        schema: wait_schema,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: true,
    },
    Spec {
        name: "browser_history",
        title: "History",
        description: "Go back or forward exactly one entry in the tab session history, or reload the page. The result reports the action, the final loaded url and the load state.",
        schema: history_schema,
        read_only: false,
        destructive: false,
        idempotent: false,
        open_world: true,
    },
    Spec {
        name: "tab_list",
        title: "List tabs",
        description: "List the open tabs with their identifiers, indexes, urls, titles and load status.",
        schema: tab_list_schema,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: true,
    },
    Spec {
        name: "tab_new",
        title: "New tab",
        description: "Open a new tab, optionally at a url, and report the new tab identifier and the previously active tab.",
        schema: tab_new_schema,
        read_only: false,
        destructive: false,
        idempotent: false,
        open_world: true,
    },
    Spec {
        name: "tab_close",
        title: "Close tab",
        description: "Close a tab and report which tab becomes active next when the browser provides one.",
        schema: tab_close_schema,
        read_only: false,
        destructive: true,
        idempotent: false,
        open_world: true,
    },
    Spec {
        name: "tab_switch",
        title: "Switch tab",
        description: "Make a tab active and report its url, title and active state.",
        schema: tab_switch_schema,
        read_only: false,
        destructive: false,
        idempotent: true,
        open_world: true,
    },
    Spec {
        name: "browser_list",
        title: "List browsers",
        description: "List the browsers currently connected to the daemon, sorted by connection time. The result is answered locally from the transport registry and never reaches a browser.",
        schema: browser_list_schema,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: false,
    },
    Spec {
        name: "net_capture",
        title: "Network capture",
        description: "Start, stop or inspect network capture for a tab. Starting attaches the debugger when needed, enables the network domain and keeps a 2000-entry per-tab ring buffer that survives same-tab navigation.",
        schema: net_capture_schema,
        read_only: false,
        destructive: false,
        idempotent: false,
        open_world: true,
    },
    Spec {
        name: "net_log",
        title: "Network log",
        description: "Return captured network entries, optionally filtered by url, method, resource type and status range. Entries are sorted by sequence; request and response headers are included only when requested.",
        schema: net_log_schema,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: true,
    },
    Spec {
        name: "net_clear",
        title: "Clear network log",
        description: "Clear captured network entries for one tab, or for every captured tab of the browser when tab_id is omitted.",
        schema: net_clear_schema,
        read_only: false,
        destructive: true,
        idempotent: false,
        open_world: true,
    },
    Spec {
        name: "net_intercept",
        title: "Network interception",
        description: "Replace the network interception rules for a tab, or browser-wide when tab_id is omitted. Rules are matched in order; each matching request is blocked, mocked or continued.",
        schema: net_intercept_schema,
        read_only: false,
        destructive: true,
        idempotent: true,
        open_world: true,
    },
    Spec {
        name: "download_list",
        title: "List downloads",
        description: "List recent downloads for a browser, newest first, with state, byte counts and local file availability.",
        schema: download_list_schema,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: true,
    },
    Spec {
        name: "download_wait",
        title: "Wait for download",
        description: "Wait until a download reaches the complete or interrupted state, then report its id, state, filename, url and byte counts. Without an id it waits for the most recent download started after the call. A timeout is reported as TIMEOUT.",
        schema: download_wait_schema,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: true,
    },
    Spec {
        name: "upload_file",
        title: "Upload file",
        description: "Set one or more local files on a file input resolved through the debugger channel, so hidden inputs work, then dispatch input and change events.",
        schema: upload_file_schema,
        read_only: false,
        destructive: false,
        idempotent: false,
        open_world: false,
    },
];

/// Names of every tool, in catalogue order.
pub fn names() -> Vec<&'static str> {
    SPECS.iter().map(|spec| spec.name).collect()
}

/// MCP tool definitions, ready to be embedded in a tools/list result.
pub fn definitions() -> Vec<Value> {
    SPECS
        .iter()
        .map(|spec| {
            let mut schema = (spec.schema)();
            add_browser_id(&mut schema);
            json!({
                "name": spec.name,
                "title": spec.title,
                "description": spec.description,
                "inputSchema": schema,
                "annotations": {
                    "readOnlyHint": spec.read_only,
                    "destructiveHint": spec.destructive,
                    "idempotentHint": spec.idempotent,
                    "openWorldHint": spec.open_world,
                },
            })
        })
        .collect()
}

/// Add the optional routing field every tool accepts.
fn add_browser_id(schema: &mut Value) {
    if let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) {
        properties.insert(
            "browser_id".to_string(),
            json!({
                "type": "string",
                "description": "Browser to route the call to; the only connected browser when omitted.",
            }),
        );
    }
}

fn navigate_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "url": {
                "type": "string",
                "description": "Absolute URL to open.",
            },
            "tab_id": {
                "type": "string",
                "description": "Tab to navigate; the active tab when omitted.",
            },
        },
        "required": ["url"],
        "additionalProperties": false,
    })
}

fn snapshot_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tab_id": {
                "type": "string",
                "description": "Tab to capture; the active tab when omitted.",
            },
            "mode": {
                "type": "string",
                "enum": ["interactive", "full"],
                "default": "interactive",
                "description": "interactive returns only actionable nodes, full the whole tree.",
            },
            "max_nodes": {
                "type": "integer",
                "minimum": 40,
                "maximum": 3000,
                "description": "Upper bound on the nodes returned; defaults to the daemon limit.",
            },
        },
        "additionalProperties": false,
    })
}

fn click_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "target": {
                "type": "string",
                "description": "Element reference from page_snapshot such as @e12, or a CSS selector.",
            },
            "tab_id": {
                "type": "string",
                "description": "Tab that holds the element; the active tab when omitted.",
            },
        },
        "required": ["target"],
        "additionalProperties": false,
    })
}

fn type_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "target": {
                "type": "string",
                "description": "Element reference from page_snapshot such as @e7, or a CSS selector.",
            },
            "text": {
                "type": "string",
                "description": "Text to enter into the element.",
            },
            "clear": {
                "type": "boolean",
                "default": true,
                "description": "Replace the current value instead of appending to it.",
            },
            "blur": {
                "type": "boolean",
                "default": false,
                "description": "Blur the element after typing so change handlers run.",
            },
            "tab_id": {
                "type": "string",
                "description": "Tab that holds the element; the active tab when omitted.",
            },
        },
        "required": ["target", "text"],
        "additionalProperties": false,
    })
}

fn page_text_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tab_id": {
                "type": "string",
                "description": "Tab to read; the active tab when omitted.",
            },
            "max_chars": {
                "type": "integer",
                "minimum": 1,
                "maximum": 200000,
                "default": 100000,
                "description": "Upper bound on the characters returned.",
            },
        },
        "additionalProperties": false,
    })
}

fn page_html_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tab_id": {
                "type": "string",
                "description": "Tab to read; the active tab when omitted.",
            },
            "target": {
                "type": "string",
                "description": "Element reference from page_snapshot such as @e12, or a CSS selector; the whole document when omitted.",
            },
            "max_chars": {
                "type": "integer",
                "minimum": 1,
                "maximum": 500000,
                "default": 200000,
                "description": "Upper bound on the characters returned.",
            },
        },
        "additionalProperties": false,
    })
}

fn page_find_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "Text or pattern to find.",
            },
            "tab_id": {
                "type": "string",
                "description": "Tab to search; the active tab when omitted.",
            },
            "role": {
                "type": "string",
                "description": "Accessibility role to restrict matches, for example link or button.",
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": 100,
                "default": 20,
                "description": "Upper bound on the matches returned.",
            },
            "regex": {
                "type": "boolean",
                "default": false,
                "description": "Treat query as a regular expression.",
            },
        },
        "required": ["query"],
        "additionalProperties": false,
    })
}

fn hover_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "target": {
                "type": "string",
                "description": "Element reference from page_snapshot such as @e12, or a CSS selector.",
            },
            "tab_id": {
                "type": "string",
                "description": "Tab that holds the element; the active tab when omitted.",
            },
        },
        "required": ["target"],
        "additionalProperties": false,
    })
}

fn press_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "keys": {
                "description": "One key or a sequence of keys. Supported keys: Enter, Tab, Escape, Backspace, Delete, Space, ArrowUp, ArrowDown, ArrowLeft, ArrowRight, PageUp, PageDown, Home, End, single characters, and combinations with Control+, Shift+, Alt+ or Meta+.",
                "oneOf": [
                    { "type": "string", "minLength": 1 },
                    {
                        "type": "array",
                        "items": { "type": "string", "minLength": 1 },
                        "minItems": 1,
                    }
                ],
            },
            "target": {
                "type": "string",
                "description": "Element to focus before pressing keys; the current focus when omitted.",
            },
            "tab_id": {
                "type": "string",
                "description": "Tab that holds the page; the active tab when omitted.",
            },
        },
        "required": ["keys"],
        "additionalProperties": false,
    })
}

fn scroll_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "direction": {
                "type": "string",
                "enum": ["up", "down", "left", "right"],
                "description": "Direction to scroll.",
            },
            "pages": {
                "type": "number",
                "minimum": 0,
                "maximum": 50,
                "default": 1,
                "description": "Number of viewport pages to scroll.",
            },
            "target": {
                "type": "string",
                "description": "Element to scroll; the page when omitted.",
            },
            "tab_id": {
                "type": "string",
                "description": "Tab that holds the page; the active tab when omitted.",
            },
        },
        "required": ["direction"],
        "additionalProperties": false,
    })
}

fn select_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "target": {
                "type": "string",
                "description": "Select element reference from page_snapshot such as @e12, or a CSS selector.",
            },
            "values": {
                "type": "array",
                "items": { "type": "string" },
                "minItems": 1,
                "description": "Values to select. Multiple values require a select element that allows multiple selections.",
            },
            "tab_id": {
                "type": "string",
                "description": "Tab that holds the element; the active tab when omitted.",
            },
        },
        "required": ["target", "values"],
        "additionalProperties": false,
    })
}

fn eval_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "expression": {
                "type": "string",
                "description": "JavaScript expression to evaluate.",
            },
            "world": {
                "type": "string",
                "enum": ["main", "isolated"],
                "default": "isolated",
                "description": "main evaluates in the page world, isolated in the content-script world.",
            },
            "tab_id": {
                "type": "string",
                "description": "Tab that holds the page; the active tab when omitted.",
            },
        },
        "required": ["expression"],
        "additionalProperties": false,
    })
}

fn wait_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tab_id": {
                "type": "string",
                "description": "Tab to watch; the active tab when omitted.",
            },
            "selector": {
                "type": "string",
                "description": "CSS selector that must match an element.",
            },
            "text": {
                "type": "string",
                "description": "Text that must appear in the page.",
            },
            "url_contains": {
                "type": "string",
                "description": "Text that the page url must contain.",
            },
            "timeout_ms": {
                "type": "integer",
                "minimum": 100,
                "maximum": 120000,
                "default": 10000,
                "description": "Maximum time to wait for the condition.",
            },
        },
        "anyOf": [
            { "required": ["selector"] },
            { "required": ["text"] },
            { "required": ["url_contains"] }
        ],
        "additionalProperties": false,
    })
}

fn history_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["back", "forward", "reload"],
                "description": "History action to perform.",
            },
            "tab_id": {
                "type": "string",
                "description": "Tab to act on; the active tab when omitted.",
            },
        },
        "required": ["action"],
        "additionalProperties": false,
    })
}

fn tab_list_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false,
    })
}

fn tab_new_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "url": {
                "type": "string",
                "description": "Absolute URL to open in the new tab.",
            },
            "active": {
                "type": "boolean",
                "default": true,
                "description": "Make the new tab active.",
            },
        },
        "additionalProperties": false,
    })
}

fn tab_close_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tab_id": {
                "type": "string",
                "description": "Tab to close.",
            },
        },
        "required": ["tab_id"],
        "additionalProperties": false,
    })
}

fn tab_switch_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tab_id": {
                "type": "string",
                "description": "Tab to make active.",
            },
        },
        "required": ["tab_id"],
        "additionalProperties": false,
    })
}

fn browser_list_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false,
    })
}

fn net_capture_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["start", "stop", "status"],
                "description": "Start capture, stop capture, or report capture state.",
            },
            "tab_id": {
                "type": "string",
                "description": "Tab to capture; the active tab when omitted.",
            },
        },
        "required": ["action"],
        "additionalProperties": false,
    })
}

fn net_log_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tab_id": {
                "type": "string",
                "description": "Tab whose captured entries to read; every captured tab of the browser when omitted.",
            },
            "since_seq": {
                "type": "integer",
                "minimum": 0,
                "default": 0,
                "description": "Return entries with a sequence number greater than this value.",
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": 1000,
                "default": 100,
                "description": "Upper bound on the entries returned after filtering.",
            },
            "url_contains": {
                "type": "string",
                "description": "Only return entries whose url contains this text.",
            },
            "method": {
                "type": "string",
                "description": "Only return requests with this HTTP method.",
            },
            "resource_type": {
                "type": "string",
                "description": "Only return entries with this network resource type.",
            },
            "status_min": {
                "type": "integer",
                "minimum": 100,
                "maximum": 599,
                "description": "Only return responses with a status at or above this value.",
            },
            "status_max": {
                "type": "integer",
                "minimum": 100,
                "maximum": 599,
                "description": "Only return responses with a status at or below this value.",
            },
            "include_headers": {
                "type": "boolean",
                "default": false,
                "description": "Include request and response headers in each entry.",
            },
        },
        "additionalProperties": false,
    })
}

fn net_clear_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tab_id": {
                "type": "string",
                "description": "Tab to clear; every captured tab of the browser when omitted.",
            },
        },
        "additionalProperties": false,
    })
}

fn net_intercept_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["set", "clear"],
                "description": "Replace the rule set or clear it.",
            },
            "tab_id": {
                "type": "string",
                "description": "Tab to scope the rules to; browser-wide rules when omitted.",
            },
            "rules": {
                "type": "array",
                "minItems": 1,
                "maxItems": 50,
                "description": "Rules to apply in order; the first matching rule wins. Required for action set.",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": {
                            "type": "string",
                            "minLength": 1,
                            "maxLength": 64,
                            "description": "Caller-chosen rule identifier, unique in the set.",
                        },
                        "url_regex": {
                            "type": "string",
                            "minLength": 1,
                            "description": "JavaScript regular expression matched against the request url.",
                        },
                        "method": {
                            "type": "string",
                            "description": "Only match this HTTP method.",
                        },
                        "resource_type": {
                            "type": "string",
                            "description": "Only match this network resource type.",
                        },
                        "action": {
                            "type": "string",
                            "enum": ["block", "mock", "continue"],
                            "description": "What to do with a matching request.",
                        },
                        "status": {
                            "type": "integer",
                            "minimum": 100,
                            "maximum": 599,
                            "default": 200,
                            "description": "Response status for a mock rule.",
                        },
                        "headers": {
                            "type": "object",
                            "additionalProperties": { "type": "string" },
                            "description": "Response headers for a mock rule.",
                        },
                        "body_base64": {
                            "type": "string",
                            "description": "Base64 response body for a mock rule, at most 1 MiB decoded.",
                        },
                    },
                    "required": ["id", "url_regex", "action"],
                    "additionalProperties": false,
                },
            },
        },
        "required": ["action"],
        "additionalProperties": false,
    })
}

fn download_list_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": 200,
                "default": 50,
                "description": "Upper bound on the downloads returned.",
            },
        },
        "additionalProperties": false,
    })
}

fn download_wait_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": {
                "type": "string",
                "description": "Download to wait for; the most recent download started after the call when omitted.",
            },
            "timeout_ms": {
                "type": "integer",
                "minimum": 100,
                "maximum": 120000,
                "default": 30000,
                "description": "Maximum time to wait for the download to settle.",
            },
        },
        "additionalProperties": false,
    })
}

fn upload_file_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "target": {
                "type": "string",
                "description": "File input reference from page_snapshot such as @e12, or a CSS selector.",
            },
            "files": {
                "type": "array",
                "items": { "type": "string", "minLength": 1 },
                "minItems": 1,
                "maxItems": 10,
                "description": "Absolute local file paths to set on the input.",
            },
            "tab_id": {
                "type": "string",
                "description": "Tab that holds the input; the active tab when omitted.",
            },
        },
        "required": ["target", "files"],
        "additionalProperties": false,
    })
}

/// Check a tool call against the catalogue and normalise its arguments.
///
/// Everything this function rejects is a caller mistake, so the returned error
/// is always BAD_ARGS.
pub fn validate(name: &str, arguments: &Value) -> Result<Value, ErrorBody> {
    let arguments = match arguments {
        Value::Null => json!({}),
        Value::Object(map) => Value::Object(map.clone()),
        _ => {
            return Err(ErrorBody::new(
                ErrorCode::BadArgs,
                "tool arguments must be a JSON object",
            ))
        }
    };
    let mut object = arguments.as_object().cloned().unwrap_or_default();
    optional_string(&object, "browser_id", name)?;
    match name {
        "browser_navigate" => {
            required_string(&object, "url", name)?;
            optional_string(&object, "tab_id", name)?;
        }
        "page_snapshot" => {
            optional_string(&object, "tab_id", name)?;
            match object.get("mode") {
                None => {
                    object.insert("mode".to_string(), json!("interactive"));
                }
                Some(Value::String(mode)) if mode == "full" || mode == "interactive" => {}
                Some(_) => {
                    return Err(ErrorBody::new(
                        ErrorCode::BadArgs,
                        "page_snapshot mode must be interactive or full",
                    ))
                }
            }
            match object.get("max_nodes") {
                None | Some(Value::Null) => {}
                Some(value) => match value.as_u64() {
                    Some(nodes) if (40..=3000).contains(&nodes) => {}
                    _ => {
                        return Err(ErrorBody::new(
                            ErrorCode::BadArgs,
                            "page_snapshot max_nodes must be an integer from 40 to 3000",
                        ))
                    }
                },
            }
        }
        "browser_click" => {
            required_string(&object, "target", name)?;
            optional_string(&object, "tab_id", name)?;
        }
        "browser_type" => {
            required_string(&object, "target", name)?;
            if object.get("text").and_then(Value::as_str).is_none() {
                return Err(ErrorBody::new(
                    ErrorCode::BadArgs,
                    "browser_type requires a string text argument",
                ));
            }
            optional_string(&object, "tab_id", name)?;
            optional_bool(&object, "clear", name)?;
            optional_bool(&object, "blur", name)?;
            object.entry("clear".to_string()).or_insert(json!(true));
            object.entry("blur".to_string()).or_insert(json!(false));
        }
        "page_text" => {
            optional_string(&object, "tab_id", name)?;
            optional_integer_range(&object, "max_chars", 1, 200_000, name)?;
            fill_default(&mut object, "max_chars", json!(100_000));
        }
        "page_html" => {
            optional_string(&object, "tab_id", name)?;
            optional_string(&object, "target", name)?;
            optional_integer_range(&object, "max_chars", 1, 500_000, name)?;
            fill_default(&mut object, "max_chars", json!(200_000));
        }
        "page_find" => {
            required_string(&object, "query", name)?;
            optional_string(&object, "tab_id", name)?;
            optional_string(&object, "role", name)?;
            optional_integer_range(&object, "limit", 1, 100, name)?;
            optional_bool(&object, "regex", name)?;
            fill_default(&mut object, "limit", json!(20));
            fill_default(&mut object, "regex", json!(false));
        }
        "browser_hover" => {
            required_string(&object, "target", name)?;
            optional_string(&object, "tab_id", name)?;
        }
        "browser_press" => {
            required_keys(&object, name)?;
            optional_string(&object, "target", name)?;
            optional_string(&object, "tab_id", name)?;
        }
        "browser_scroll" => {
            required_choice(&object, "direction", &["up", "down", "left", "right"], name)?;
            optional_number_range(&object, "pages", 0.0, 50.0, name)?;
            optional_string(&object, "target", name)?;
            optional_string(&object, "tab_id", name)?;
            fill_default(&mut object, "pages", json!(1));
        }
        "browser_select" => {
            required_string(&object, "target", name)?;
            required_string_list(&object, "values", name)?;
            optional_string(&object, "tab_id", name)?;
        }
        "browser_eval" => {
            required_string(&object, "expression", name)?;
            optional_choice(&object, "world", &["main", "isolated"], name)?;
            optional_string(&object, "tab_id", name)?;
            fill_default(&mut object, "world", json!("isolated"));
        }
        "browser_wait" => {
            optional_string(&object, "tab_id", name)?;
            optional_string(&object, "selector", name)?;
            optional_string(&object, "text", name)?;
            optional_string(&object, "url_contains", name)?;
            if ["selector", "text", "url_contains"]
                .iter()
                .all(|field| matches!(object.get(*field), None | Some(Value::Null)))
            {
                return Err(ErrorBody::new(
                    ErrorCode::BadArgs,
                    "browser_wait requires at least one of selector, text or url_contains",
                ));
            }
            optional_integer_range(&object, "timeout_ms", 100, 120_000, name)?;
            fill_default(&mut object, "timeout_ms", json!(10_000));
        }
        "browser_history" => {
            required_choice(&object, "action", &["back", "forward", "reload"], name)?;
            optional_string(&object, "tab_id", name)?;
        }
        "tab_list" => {}
        "tab_new" => {
            optional_string(&object, "url", name)?;
            optional_bool(&object, "active", name)?;
            fill_default(&mut object, "active", json!(true));
        }
        "tab_close" => {
            required_string(&object, "tab_id", name)?;
        }
        "tab_switch" => {
            required_string(&object, "tab_id", name)?;
        }
        "browser_list" => {}
        "net_capture" => {
            required_choice(&object, "action", &["start", "stop", "status"], name)?;
            optional_string(&object, "tab_id", name)?;
        }
        "net_log" => {
            optional_string(&object, "tab_id", name)?;
            optional_string(&object, "url_contains", name)?;
            optional_string(&object, "method", name)?;
            optional_string(&object, "resource_type", name)?;
            optional_integer_range(&object, "since_seq", 0, u64::MAX, name)?;
            optional_integer_range(&object, "limit", 1, 1000, name)?;
            optional_integer_range(&object, "status_min", 100, 599, name)?;
            optional_integer_range(&object, "status_max", 100, 599, name)?;
            optional_bool(&object, "include_headers", name)?;
            if let (Some(min), Some(max)) = (
                object.get("status_min").and_then(Value::as_u64),
                object.get("status_max").and_then(Value::as_u64),
            ) {
                if min > max {
                    return Err(ErrorBody::new(
                        ErrorCode::BadArgs,
                        "net_log status_min must not exceed status_max",
                    ));
                }
            }
            fill_default(&mut object, "since_seq", json!(0));
            fill_default(&mut object, "limit", json!(100));
            fill_default(&mut object, "include_headers", json!(false));
        }
        "net_clear" => {
            optional_string(&object, "tab_id", name)?;
        }
        "net_intercept" => {
            required_choice(&object, "action", &["set", "clear"], name)?;
            optional_string(&object, "tab_id", name)?;
            if object.get("action").and_then(Value::as_str) == Some("set") {
                validate_intercept_rules(&mut object)?;
            }
        }
        "download_list" => {
            optional_integer_range(&object, "limit", 1, 200, name)?;
            fill_default(&mut object, "limit", json!(50));
        }
        "download_wait" => {
            optional_string(&object, "id", name)?;
            optional_integer_range(&object, "timeout_ms", 100, 120_000, name)?;
            fill_default(&mut object, "timeout_ms", json!(30_000));
        }
        "upload_file" => {
            required_string(&object, "target", name)?;
            required_upload_files(&object)?;
            optional_string(&object, "tab_id", name)?;
        }
        other => {
            return Err(
                ErrorBody::new(ErrorCode::BadArgs, format!("unknown tool {other}"))
                    .with_details(json!({ "tool": other, "known": names() })),
            );
        }
    }
    Ok(Value::Object(object))
}

fn required_string(object: &Map<String, Value>, field: &str, tool: &str) -> Result<(), ErrorBody> {
    match object.get(field).and_then(Value::as_str) {
        Some(value) if !value.trim().is_empty() => Ok(()),
        _ => Err(ErrorBody::new(
            ErrorCode::BadArgs,
            format!("{tool} requires a non-empty string {field} argument"),
        )),
    }
}

fn optional_bool(object: &Map<String, Value>, field: &str, tool: &str) -> Result<(), ErrorBody> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(()),
        Some(Value::Bool(_)) => Ok(()),
        Some(_) => Err(ErrorBody::new(
            ErrorCode::BadArgs,
            format!("{tool} argument {field} must be a boolean"),
        )),
    }
}

fn optional_string(object: &Map<String, Value>, field: &str, tool: &str) -> Result<(), ErrorBody> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(()),
        Some(Value::String(_)) => Ok(()),
        Some(_) => Err(ErrorBody::new(
            ErrorCode::BadArgs,
            format!("{tool} argument {field} must be a string"),
        )),
    }
}

fn optional_integer_range(
    object: &Map<String, Value>,
    field: &str,
    min: u64,
    max: u64,
    tool: &str,
) -> Result<(), ErrorBody> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(()),
        Some(value) => match value.as_u64() {
            Some(number) if (min..=max).contains(&number) => Ok(()),
            _ => Err(ErrorBody::new(
                ErrorCode::BadArgs,
                format!("{tool} argument {field} must be an integer from {min} to {max}"),
            )),
        },
    }
}

fn optional_number_range(
    object: &Map<String, Value>,
    field: &str,
    min: f64,
    max: f64,
    tool: &str,
) -> Result<(), ErrorBody> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(()),
        Some(value) => match value.as_f64() {
            Some(number) if number >= min && number <= max => Ok(()),
            _ => Err(ErrorBody::new(
                ErrorCode::BadArgs,
                format!("{tool} argument {field} must be a number from {min} to {max}"),
            )),
        },
    }
}

fn required_choice(
    object: &Map<String, Value>,
    field: &str,
    allowed: &[&str],
    tool: &str,
) -> Result<(), ErrorBody> {
    match object.get(field).and_then(Value::as_str) {
        Some(value) if allowed.contains(&value) => Ok(()),
        _ => Err(ErrorBody::new(
            ErrorCode::BadArgs,
            format!("{tool} {field} must be one of {}", allowed.join(", ")),
        )),
    }
}

fn optional_choice(
    object: &Map<String, Value>,
    field: &str,
    allowed: &[&str],
    tool: &str,
) -> Result<(), ErrorBody> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(()),
        Some(Value::String(value)) if allowed.contains(&value.as_str()) => Ok(()),
        Some(_) => Err(ErrorBody::new(
            ErrorCode::BadArgs,
            format!(
                "{tool} argument {field} must be one of {}",
                allowed.join(", ")
            ),
        )),
    }
}

fn required_string_list(
    object: &Map<String, Value>,
    field: &str,
    tool: &str,
) -> Result<(), ErrorBody> {
    match object.get(field) {
        Some(Value::Array(items))
            if !items.is_empty()
                && items
                    .iter()
                    .all(|item| item.as_str().is_some_and(|value| !value.trim().is_empty())) =>
        {
            Ok(())
        }
        _ => Err(ErrorBody::new(
            ErrorCode::BadArgs,
            format!("{tool} requires a non-empty array of strings {field} argument"),
        )),
    }
}

fn required_keys(object: &Map<String, Value>, tool: &str) -> Result<(), ErrorBody> {
    match object.get("keys") {
        Some(Value::String(keys)) if !keys.trim().is_empty() => Ok(()),
        Some(Value::Array(keys))
            if !keys.is_empty()
                && keys
                    .iter()
                    .all(|key| key.as_str().is_some_and(|value| !value.trim().is_empty())) =>
        {
            Ok(())
        }
        _ => Err(ErrorBody::new(
            ErrorCode::BadArgs,
            format!("{tool} requires keys as a non-empty string or an array of non-empty strings"),
        )),
    }
}

fn validate_intercept_rules(object: &mut Map<String, Value>) -> Result<(), ErrorBody> {
    let Some(Value::Array(rules)) = object.get_mut("rules") else {
        return Err(ErrorBody::new(
            ErrorCode::BadArgs,
            "net_intercept action set requires 1 to 50 rules",
        ));
    };
    if rules.is_empty() || rules.len() > 50 {
        return Err(ErrorBody::new(
            ErrorCode::BadArgs,
            "net_intercept action set requires 1 to 50 rules",
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for value in rules.iter_mut() {
        let Some(rule) = value.as_object_mut() else {
            return Err(rule_error("", "net_intercept rules must be objects"));
        };
        let id = rule
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if id.is_empty() || id.chars().count() > 64 {
            return Err(rule_error(
                &id,
                "net_intercept rule id must be a non-empty string of at most 64 characters",
            ));
        }
        if !seen.insert(id.clone()) {
            return Err(rule_error(&id, "net_intercept rule ids must be unique"));
        }
        match rule.get("url_regex").and_then(Value::as_str) {
            Some(url_regex) if !url_regex.trim().is_empty() => {}
            _ => {
                return Err(rule_error(
                    &id,
                    "net_intercept rule url_regex must be a non-empty string",
                ))
            }
        }
        let action = match rule.get("action").and_then(Value::as_str) {
            Some(action @ ("block" | "mock" | "continue")) => action.to_string(),
            _ => {
                return Err(rule_error(
                    &id,
                    "net_intercept rule action must be block, mock or continue",
                ))
            }
        };
        for field in ["method", "resource_type"] {
            if let Some(value) = rule.get(field) {
                if !value.is_string() {
                    return Err(rule_error(
                        &id,
                        format!("net_intercept rule {field} must be a string"),
                    ));
                }
            }
        }
        if action != "mock" {
            for field in ["status", "headers", "body_base64"] {
                if rule.contains_key(field) {
                    return Err(rule_error(
                        &id,
                        format!("net_intercept rule {field} is only valid for mock rules"),
                    ));
                }
            }
            continue;
        }
        if let Some(status) = rule.get("status") {
            match status.as_u64() {
                Some(number) if (100..=599).contains(&number) => {}
                _ => {
                    return Err(rule_error(
                        &id,
                        "net_intercept rule status must be an integer from 100 to 599",
                    ))
                }
            }
        }
        if let Some(headers) = rule.get("headers") {
            match headers.as_object() {
                Some(headers) if headers.values().all(Value::is_string) => {}
                _ => {
                    return Err(rule_error(
                        &id,
                        "net_intercept rule headers must be an object of strings",
                    ))
                }
            }
        }
        if let Some(body) = rule.get("body_base64") {
            let Some(encoded) = body.as_str() else {
                return Err(rule_error(
                    &id,
                    "net_intercept rule body_base64 must be a string",
                ));
            };
            let decoded = BASE64.decode(encoded).map_err(|_| {
                rule_error(&id, "net_intercept rule body_base64 is not valid base64")
            })?;
            if decoded.len() > 1_048_576 {
                return Err(rule_error(
                    &id,
                    "net_intercept rule body_base64 must not exceed 1 MiB decoded",
                ));
            }
        }
        rule.entry("status".to_string()).or_insert(json!(200));
    }
    Ok(())
}

fn rule_error(rule_id: &str, message: impl Into<String>) -> ErrorBody {
    ErrorBody::new(ErrorCode::BadArgs, message).with_details(json!({ "rule_id": rule_id }))
}

fn required_upload_files(object: &Map<String, Value>) -> Result<(), ErrorBody> {
    let Some(Value::Array(files)) = object.get("files") else {
        return Err(ErrorBody::new(
            ErrorCode::BadArgs,
            "upload_file requires 1 to 10 absolute file paths",
        ));
    };
    if files.is_empty() || files.len() > 10 {
        return Err(ErrorBody::new(
            ErrorCode::BadArgs,
            "upload_file requires 1 to 10 absolute file paths",
        ));
    }
    for file in files {
        let Some(path) = file.as_str() else {
            return Err(ErrorBody::new(
                ErrorCode::BadArgs,
                "upload_file files must be absolute paths",
            )
            .with_details(json!({ "path": file })));
        };
        if path.trim().is_empty() || !std::path::Path::new(path).is_absolute() {
            return Err(ErrorBody::new(
                ErrorCode::BadArgs,
                "upload_file files must be absolute paths",
            )
            .with_details(json!({ "path": path })));
        }
    }
    Ok(())
}

fn fill_default(object: &mut Map<String, Value>, field: &str, value: Value) {
    if matches!(object.get(field), None | Some(Value::Null)) {
        object.insert(field.to_string(), value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definitions_carry_names_schemas_and_annotations() {
        let definitions = definitions();
        assert_eq!(definitions.len(), 26);
        for definition in &definitions {
            assert!(definition.get("name").and_then(Value::as_str).is_some());
            let schema = definition.get("inputSchema").expect("schema");
            assert_eq!(schema.get("type").and_then(Value::as_str), Some("object"));
            let annotations = definition.get("annotations").expect("annotations");
            for hint in [
                "readOnlyHint",
                "destructiveHint",
                "idempotentHint",
                "openWorldHint",
            ] {
                assert!(
                    annotations.get(hint).and_then(Value::as_bool).is_some(),
                    "missing {hint}"
                );
            }
        }
        assert_eq!(
            names(),
            vec![
                "browser_navigate",
                "page_snapshot",
                "browser_click",
                "browser_type",
                "page_text",
                "page_html",
                "page_find",
                "browser_hover",
                "browser_press",
                "browser_scroll",
                "browser_select",
                "browser_eval",
                "browser_wait",
                "browser_history",
                "tab_list",
                "tab_new",
                "tab_close",
                "tab_switch",
                "browser_list",
                "net_capture",
                "net_log",
                "net_clear",
                "net_intercept",
                "download_list",
                "download_wait",
                "upload_file",
            ]
        );
    }

    #[test]
    fn every_tool_schema_accepts_an_optional_browser_id() {
        for definition in definitions() {
            let name = definition
                .get("name")
                .and_then(Value::as_str)
                .expect("tool name");
            let properties = definition
                .get("inputSchema")
                .and_then(|schema| schema.get("properties"))
                .and_then(Value::as_object)
                .expect("properties");
            let browser_id = properties
                .get("browser_id")
                .unwrap_or_else(|| panic!("{name} is missing browser_id"));
            assert_eq!(
                browser_id.get("type").and_then(Value::as_str),
                Some("string"),
                "{name} browser_id must be a string"
            );
        }
        let click = validate(
            "browser_click",
            &json!({ "target": "@e1", "browser_id": "edge:one" }),
        )
        .expect("click with browser_id");
        assert_eq!(
            click.get("browser_id").and_then(Value::as_str),
            Some("edge:one")
        );
    }

    #[test]
    fn network_download_and_upload_defaults_are_filled() {
        let capture = validate("net_capture", &json!({ "action": "start" })).expect("capture");
        assert_eq!(capture.get("action").and_then(Value::as_str), Some("start"));
        let log = validate("net_log", &json!({})).expect("net_log");
        assert_eq!(log.get("since_seq").and_then(Value::as_u64), Some(0));
        assert_eq!(log.get("limit").and_then(Value::as_u64), Some(100));
        assert_eq!(
            log.get("include_headers").and_then(Value::as_bool),
            Some(false)
        );
        let intercept = validate(
            "net_intercept",
            &json!({
                "action": "set",
                "rules": [
                    { "id": "r1", "url_regex": "/api/", "action": "mock", "headers": { "x-test": "1" } }
                ]
            }),
        )
        .expect("net_intercept");
        assert_eq!(
            intercept
                .get("rules")
                .and_then(Value::as_array)
                .and_then(|rules| rules.first())
                .and_then(|rule| rule.get("status"))
                .and_then(Value::as_u64),
            Some(200)
        );
        let list = validate("download_list", &json!({})).expect("download_list");
        assert_eq!(list.get("limit").and_then(Value::as_u64), Some(50));
        let wait = validate("download_wait", &json!({})).expect("download_wait");
        assert_eq!(wait.get("timeout_ms").and_then(Value::as_u64), Some(30_000));
        let path = std::env::temp_dir().join("corvus-upload.txt");
        let upload = validate(
            "upload_file",
            &json!({ "target": "#file", "files": [path.to_string_lossy()] }),
        )
        .expect("upload_file");
        assert_eq!(
            upload.get("files").and_then(Value::as_array).map(Vec::len),
            Some(1)
        );
    }

    #[test]
    fn validation_accepts_good_arguments_and_fills_defaults() {
        assert!(validate(
            "browser_navigate",
            &json!({ "url": "https://example.test" })
        )
        .is_ok());
        let snapshot = validate("page_snapshot", &json!({})).expect("snapshot");
        assert_eq!(
            snapshot.get("mode").and_then(Value::as_str),
            Some("interactive")
        );
        let full = validate(
            "page_snapshot",
            &json!({ "mode": "full", "max_nodes": 500 }),
        )
        .expect("full snapshot");
        assert_eq!(full.get("mode").and_then(Value::as_str), Some("full"));
        assert_eq!(full.get("max_nodes").and_then(Value::as_u64), Some(500));
        let click =
            validate("browser_click", &json!({ "target": "@e1", "tab_id": "7" })).expect("click");
        assert_eq!(click.get("tab_id").and_then(Value::as_str), Some("7"));
        let typed =
            validate("browser_type", &json!({ "target": "@e1", "text": "" })).expect("type");
        assert_eq!(typed.get("clear").and_then(Value::as_bool), Some(true));
        assert_eq!(typed.get("blur").and_then(Value::as_bool), Some(false));
    }

    #[test]
    fn string_tab_ids_pass_through_unchanged() {
        let close = validate("tab_close", &json!({ "tab_id": "tab-999" })).expect("tab_close");
        assert_eq!(close.get("tab_id").and_then(Value::as_str), Some("tab-999"));
        let switch = validate("tab_switch", &json!({ "tab_id": "tab-42" })).expect("tab_switch");
        assert_eq!(switch.get("tab_id").and_then(Value::as_str), Some("tab-42"));
        let eval = validate(
            "browser_eval",
            &json!({ "expression": "1 + 1", "tab_id": "tab-7" }),
        )
        .expect("browser_eval");
        assert_eq!(eval.get("tab_id").and_then(Value::as_str), Some("tab-7"));
        let history = validate(
            "browser_history",
            &json!({ "action": "back", "tab_id": "tab-8" }),
        )
        .expect("browser_history");
        assert_eq!(history.get("tab_id").and_then(Value::as_str), Some("tab-8"));
    }

    #[test]
    fn new_tool_defaults_are_filled() {
        let text = validate("page_text", &json!({})).expect("page_text");
        assert_eq!(text.get("max_chars").and_then(Value::as_u64), Some(100_000));
        let html = validate("page_html", &json!({})).expect("page_html");
        assert_eq!(html.get("max_chars").and_then(Value::as_u64), Some(200_000));
        let find = validate("page_find", &json!({ "query": "hello" })).expect("page_find");
        assert_eq!(find.get("limit").and_then(Value::as_u64), Some(20));
        assert_eq!(find.get("regex").and_then(Value::as_bool), Some(false));
        let scroll = validate("browser_scroll", &json!({ "direction": "down" })).expect("scroll");
        assert_eq!(scroll.get("pages").and_then(Value::as_u64), Some(1));
        let eval = validate("browser_eval", &json!({ "expression": "1 + 1" })).expect("eval");
        assert_eq!(eval.get("world").and_then(Value::as_str), Some("isolated"));
        let wait = validate("browser_wait", &json!({ "text": "ready" })).expect("wait");
        assert_eq!(wait.get("timeout_ms").and_then(Value::as_u64), Some(10_000));
        let tab = validate("tab_new", &json!({})).expect("tab_new");
        assert_eq!(tab.get("active").and_then(Value::as_bool), Some(true));
    }

    #[test]
    fn validation_rejects_bad_arguments_with_bad_args() {
        let cases = [
            ("browser_navigate", json!({})),
            ("browser_navigate", json!({ "url": "" })),
            ("browser_navigate", json!({ "url": 7 })),
            ("page_snapshot", json!({ "mode": "everything" })),
            ("page_snapshot", json!({ "max_nodes": 10 })),
            ("page_snapshot", json!({ "max_nodes": "many" })),
            ("browser_click", json!({})),
            ("browser_click", json!({ "target": "@e1", "tab_id": 3 })),
            ("browser_type", json!({ "target": "@e1" })),
            (
                "browser_type",
                json!({ "target": "@e1", "text": "x", "clear": "yes" }),
            ),
            ("page_text", json!({ "max_chars": 0 })),
            ("page_text", json!({ "max_chars": 200_001 })),
            ("page_html", json!({ "max_chars": 500_001 })),
            ("page_find", json!({})),
            ("page_find", json!({ "query": "x", "limit": 0 })),
            ("page_find", json!({ "query": "x", "limit": 101 })),
            ("browser_hover", json!({})),
            ("browser_press", json!({})),
            ("browser_press", json!({ "keys": [] })),
            ("browser_scroll", json!({ "direction": "diagonal" })),
            (
                "browser_scroll",
                json!({ "direction": "down", "pages": -1 }),
            ),
            ("browser_select", json!({ "target": "#s" })),
            ("browser_select", json!({ "target": "#s", "values": [] })),
            ("browser_eval", json!({})),
            (
                "browser_eval",
                json!({ "expression": "1", "world": "server" }),
            ),
            ("browser_wait", json!({})),
            ("browser_wait", json!({ "text": "x", "timeout_ms": 99 })),
            ("browser_history", json!({ "action": "refresh" })),
            ("tab_close", json!({})),
            ("tab_switch", json!({})),
            ("tab_new", json!({ "active": "yes" })),
            ("browser_list", json!({ "browser_id": 7 })),
            ("net_capture", json!({})),
            ("net_capture", json!({ "action": "pause" })),
            ("net_log", json!({ "limit": 0 })),
            ("net_log", json!({ "limit": 1001 })),
            ("net_log", json!({ "status_min": 500, "status_max": 200 })),
            ("net_log", json!({ "include_headers": "yes" })),
            ("net_intercept", json!({ "action": "set" })),
            ("net_intercept", json!({ "action": "set", "rules": [] })),
            (
                "net_intercept",
                json!({
                    "action": "set",
                    "rules": [
                        { "id": "r1", "url_regex": "x", "action": "block", "status": 204 }
                    ]
                }),
            ),
            (
                "net_intercept",
                json!({
                    "action": "set",
                    "rules": [
                        { "id": "r1", "url_regex": "x", "action": "mock", "body_base64": "***" }
                    ]
                }),
            ),
            (
                "net_intercept",
                json!({
                    "action": "set",
                    "rules": [
                        { "id": "same", "url_regex": "a", "action": "block" },
                        { "id": "same", "url_regex": "b", "action": "block" }
                    ]
                }),
            ),
            ("download_list", json!({ "limit": 0 })),
            ("download_wait", json!({ "timeout_ms": 99 })),
            ("upload_file", json!({ "target": "#file" })),
            ("upload_file", json!({ "target": "#file", "files": [] })),
            (
                "upload_file",
                json!({ "target": "#file", "files": ["relative.txt"] }),
            ),
            ("upload_file", json!({ "target": "#file", "files": [7] })),
            ("browser_wiggle", json!({})),
        ];
        for (tool, arguments) in cases {
            let error = validate(tool, &arguments).expect_err("must reject");
            assert_eq!(error.code, ErrorCode::BadArgs);
        }
    }
}
