//! Tool catalogue: names, JSON schemas, descriptions and MCP annotations.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use corvus_domain::BatchPolicy;
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

const SPECS: [Spec; 62] = [
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
        description: "Return a compact tree of the page. Element references from the snapshot are the values browser_click and browser_type accept as target. In compact format, pass diff true on a later snapshot of the same tab to mark added, changed and removed lines against the previous snapshot.",
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
        name: "browser_screenshot",
        title: "Screenshot",
        description: "Capture the visible viewport, the full scrollable page, or one element as webp, png or jpeg. The image is saved under the Corvus home screenshots directory; inline true also returns it as MCP image content. Element targets accept @c<backendNodeId> references from page_snapshot.",
        schema: screenshot_schema,
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
    Spec {
        name: "session_list",
        title: "List sessions",
        description: "List recorded MCP sessions, newest first, with transport, agent, step and error counts.",
        schema: session_list_schema,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: false,
    },
    Spec {
        name: "session_steps",
        title: "Read session steps",
        description: "Read the recorded steps of one session in sequence order. Arguments and results are parsed JSON when they were stored complete, otherwise a stored prefix string.",
        schema: session_steps_schema,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: false,
    },
    Spec {
        name: "session_replay",
        title: "Replay a session",
        description: "Plan or re-execute the recorded steps of a session. Dry run reports the plan; live mode sends each replayable call through the router and records it in a new replay session. Control tools and tab tools are skipped.",
        schema: session_replay_schema,
        read_only: false,
        destructive: true,
        idempotent: false,
        open_world: true,
    },
    Spec {
        name: "session_export",
        title: "Export a session",
        description: "Export one recorded session as JSON Lines: a session row followed by every step row, capped at 2 MiB.",
        schema: session_export_schema,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: false,
    },
    Spec {
        name: "metrics",
        title: "Daemon metrics",
        description: "Report process uptime, connected browsers, recorded sessions and per-tool call, error and average latency counters since the daemon started.",
        schema: metrics_schema,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: false,
    },
    Spec {
        name: "logs_tail",
        title: "Read daemon logs",
        description: "Return the most recent daemon log entries from the bounded in-process buffer, optionally filtered by minimum level and timestamp.",
        schema: logs_tail_schema,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: false,
    },
    Spec {
        name: "cdp_status",
        title: "CDP status",
        description: "Report the Chrome DevTools Protocol connection, the extension descriptor that advertised it, target and event-ring counts, and the CDP-capable tools.",
        schema: cdp_status_schema,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: false,
    },
    Spec {
        name: "cdp_targets",
        title: "CDP targets",
        description: "List the browser targets known through Target.setDiscoverTargets, including page or worker type, URL, title and whether a flattened session is attached.",
        schema: cdp_targets_schema,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: false,
    },
    Spec {
        name: "cdp_send",
        title: "Send CDP command",
        description: "Send one Chrome DevTools Protocol command through the browser-level connection and return its raw result. Use target_id or session_id to address a page; the timeout is bounded.",
        schema: cdp_send_schema,
        read_only: false,
        destructive: true,
        idempotent: false,
        open_world: true,
    },
    Spec {
        name: "cdp_events",
        title: "Read CDP events",
        description: "Read the bounded CDP event ring in sequence order, optionally filtered by method, waiting up to wait_ms for new events. clear removes the returned entries.",
        schema: cdp_events_schema,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: true,
    },
    Spec {
        name: "browser_channel",
        title: "Browser channel",
        description: "Inspect or switch how browser tools are carried: auto prefers CDP when connected and falls back to the extension, cdp never falls back, and extension ignores CDP.",
        schema: browser_channel_schema,
        read_only: false,
        destructive: false,
        idempotent: true,
        open_world: false,
    },
    Spec {
        name: "browser_console",
        title: "Browser console",
        description: "Return recent console and uncaught-exception entries captured from the tab through Runtime.enable.",
        schema: browser_console_schema,
        read_only: true,
        destructive: false,
        idempotent: false,
        open_world: true,
    },
    Spec {
        name: "browser_cookies",
        title: "Browser cookies",
        description: "Return browser cookies, or the cookies visible to specific URLs. Sensitive values are returned as the browser exposes them. When a guardrails policy enables secret redaction, each cookie value is replaced with [redacted] and marked redacted: true.",
        schema: browser_cookies_schema,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: true,
    },
    Spec {
        name: "browser_dialog",
        title: "Handle dialog",
        description: "Accept or dismiss the JavaScript dialog currently blocking a tab, optionally supplying prompt text.",
        schema: browser_dialog_schema,
        read_only: false,
        destructive: true,
        idempotent: false,
        open_world: true,
    },
    Spec {
        name: "browser_emulate",
        title: "Emulate device",
        description: "Apply device metrics, network conditions, a user agent override or a timezone to a tab, or reset every override this tool applies.",
        schema: browser_emulate_schema,
        read_only: false,
        destructive: false,
        idempotent: true,
        open_world: true,
    },
    Spec {
        name: "browser_batch",
        title: "Browser batch",
        description: "Run up to 20 browser tool calls in order and return one result per step. Each step names a browser tool and its arguments. stop_on_error defaults to true; browser_batch cannot call itself.",
        schema: batch_schema,
        read_only: false,
        destructive: true,
        idempotent: false,
        open_world: true,
    },
    Spec {
        name: "tool_help",
        title: "Tool help",
        description: "Return the tool catalogue index, one category with its tools, or the full description, schema and annotations of one tool.",
        schema: tool_help_schema,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: false,
    },
    Spec {
        name: "page_pdf",
        title: "Save Page as PDF",
        description: "Print the active tab to a PDF saved under the Corvus home pdf directory. Paper size is in inches; margins are fixed at 0.4 inch on every side.",
        schema: page_pdf_schema,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: true,
    },
    Spec {
        name: "page_links",
        title: "List Page Links",
        description: "List absolute links in the top document and its open shadow roots, optionally scoped by selector and filtered by visibility.",
        schema: page_links_schema,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: true,
    },
    Spec {
        name: "page_forms",
        title: "Inspect Page Forms",
        description: "Inspect forms in the top document and open shadow roots, including field selectors, labels and optionally non-password values.",
        schema: page_forms_schema,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: true,
    },
    Spec {
        name: "page_accessibility",
        title: "Page Accessibility Tree",
        description: "Return the accessibility tree of a tab or one root element, with ignored and presentational nodes removed.",
        schema: page_accessibility_schema,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: true,
    },
    Spec {
        name: "browser_drag",
        title: "Drag Between Elements",
        description: "Drag the pointer from one element to another using the element references of page_snapshot. HTML5 data transfer is not guaranteed.",
        schema: browser_drag_schema,
        read_only: false,
        destructive: true,
        idempotent: false,
        open_world: true,
    },
    Spec {
        name: "browser_focus",
        title: "Focus Element",
        description: "Focus one element and scroll it into view.",
        schema: browser_focus_schema,
        read_only: false,
        destructive: false,
        idempotent: true,
        open_world: true,
    },
    Spec {
        name: "browser_highlight",
        title: "Highlight Element",
        description: "Show a transient browser overlay on one element reference or selector for a short duration, without changing the page DOM.",
        schema: browser_highlight_schema,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: true,
    },
    Spec {
        name: "tab_group",
        title: "Group Tabs",
        description: "Group two or more tabs into one tab group and set its title and colour. This tool is implemented by the extension only.",
        schema: tab_group_schema,
        read_only: false,
        destructive: false,
        idempotent: true,
        open_world: false,
    },
    Spec {
        name: "tab_duplicate",
        title: "Duplicate Tab",
        description: "Duplicate one tab and return the new tab. This tool is implemented by the extension only.",
        schema: tab_duplicate_schema,
        read_only: false,
        destructive: false,
        idempotent: false,
        open_world: false,
    },
    Spec {
        name: "window_list",
        title: "List Windows",
        description: "List browser windows and optionally their tabs. This tool is implemented by the extension only.",
        schema: window_list_schema,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: false,
    },
    Spec {
        name: "window_new",
        title: "New Window",
        description: "Open a new browser window with an optional http, https or about:blank URL. This tool is implemented by the extension only.",
        schema: window_new_schema,
        read_only: false,
        destructive: false,
        idempotent: false,
        open_world: true,
    },
    Spec {
        name: "cookie_set",
        title: "Set Cookie",
        description: "Set one browser cookie. The value is never echoed in the result or in session recording.",
        schema: cookie_set_schema,
        read_only: false,
        destructive: false,
        idempotent: true,
        open_world: false,
    },
    Spec {
        name: "cookie_clear",
        title: "Clear Cookies",
        description: "Delete browser cookies selected by URL, domain or name. Cookie values are never returned.",
        schema: cookie_clear_schema,
        read_only: false,
        destructive: true,
        idempotent: true,
        open_world: false,
    },
    Spec {
        name: "storage_get",
        title: "Read Web Storage",
        description: "Read localStorage or sessionStorage entries for the page origin. Session recording redacts every value.",
        schema: storage_get_schema,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: false,
    },
    Spec {
        name: "storage_set",
        title: "Write Web Storage",
        description: "Write localStorage or sessionStorage entries for the page origin. Values are not echoed and session recording redacts them.",
        schema: storage_set_schema,
        read_only: false,
        destructive: false,
        idempotent: true,
        open_world: false,
    },
    Spec {
        name: "storage_clear",
        title: "Clear Web Storage",
        description: "Clear all keys or selected keys from localStorage or sessionStorage for the page origin.",
        schema: storage_clear_schema,
        read_only: false,
        destructive: true,
        idempotent: true,
        open_world: false,
    },
    Spec {
        name: "health",
        title: "Daemon Health",
        description: "Return daemon status, version, uptime, browser counts, channel, database health, guardrails state and the tool catalogue size. No paths, URLs or secrets are included.",
        schema: health_schema,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: false,
    },
    Spec {
        name: "version",
        title: "Daemon Version",
        description: "Return the daemon name, version, protocol version, tool and category counts and the schema snapshot label.",
        schema: version_schema,
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: false,
    },
];

/// Whether browser_batch may call this tool. Daemon control, recording,
/// diagnostic-log and help tools are deliberately excluded, as is recursion.
pub fn batch_tool_allowed(name: &str) -> bool {
    SPECS.iter().any(|spec| spec.name == name) && BatchPolicy::allows(category(name), name)
}

/// Names of every tool, in catalogue order.
pub fn names() -> Vec<&'static str> {
    SPECS.iter().map(|spec| spec.name).collect()
}

/// Whether a tool is known and, when it is, whether its MCP annotation marks
/// it as read-only. Guardrails use this catalogue as the single source of
/// truth instead of maintaining a second tool list.
pub fn is_read_only(name: &str) -> Option<bool> {
    SPECS
        .iter()
        .find(|spec| spec.name == name)
        .map(|spec| spec.read_only)
}

/// Whether the name belongs to a tool this server exposes.
pub fn is_known_tool(name: &str) -> bool {
    SPECS.iter().any(|spec| spec.name == name)
}

/// MCP tool definitions, ready to be embedded in a tools/list result.
pub fn definitions() -> Vec<Value> {
    definitions_for(None, "full")
}

/// Category name for a tool.
pub fn category(name: &str) -> &'static str {
    match name {
        "browser_navigate" | "page_snapshot" | "browser_screenshot" | "page_text" | "page_html"
        | "page_find" | "browser_wait" | "browser_history" | "browser_dialog"
        | "browser_emulate" | "browser_batch" | "page_pdf" | "page_links" | "page_forms"
        | "page_accessibility" => "page",
        "browser_click" | "browser_type" | "browser_hover" | "browser_press" | "browser_scroll"
        | "browser_select" | "browser_eval" | "browser_drag" | "browser_focus"
        | "browser_highlight" => "input",
        "tab_list" | "tab_new" | "tab_close" | "tab_switch" | "tab_group" | "tab_duplicate"
        | "window_list" | "window_new" => "tabs",
        "net_capture" | "net_log" | "net_clear" | "net_intercept" | "browser_cookies" => "network",
        "cookie_set" | "cookie_clear" | "storage_get" | "storage_set" | "storage_clear" => {
            "storage"
        }
        "upload_file" | "download_list" | "download_wait" => "files",
        "cdp_status" | "cdp_targets" | "cdp_send" | "cdp_events" | "browser_channel" => "cdp",
        "session_list" | "session_steps" | "session_replay" | "session_export" => "sessions",
        "browser_list" | "metrics" | "logs_tail" | "browser_console" | "health" | "version" => {
            "diagnostics"
        }
        "tool_help" => "help",
        _ => "diagnostics",
    }
}

/// One-line summary used by the brief tool list.
pub fn summary(name: &str) -> &'static str {
    match name {
        "browser_navigate" => "Open a URL in a tab and wait for the document.",
        "page_snapshot" => "Compact page tree with clickable element references.",
        "browser_click" => "Click an element reference.",
        "browser_type" => "Focus an element and replace its text.",
        "page_text" => "Visible page text, capped by max_chars.",
        "page_html" => "Document or element HTML, capped by max_chars.",
        "page_find" => "Find page text and return element references.",
        "browser_screenshot" => "Capture a viewport, full-page or element screenshot.",
        "browser_hover" => "Move the pointer over an element.",
        "browser_press" => "Send one key or a key sequence.",
        "browser_scroll" => "Scroll the page or an element.",
        "browser_select" => "Choose values in a select element.",
        "browser_eval" => "Evaluate JavaScript in the page.",
        "browser_wait" => "Wait for a selector, text or URL condition.",
        "browser_history" => "Go back, go forward or reload.",
        "tab_list" => "List open tabs.",
        "tab_new" => "Open a new tab.",
        "tab_close" => "Close a tab.",
        "tab_switch" => "Make a tab active.",
        "browser_list" => "List connected browsers and channel state.",
        "net_capture" => "Start, stop or inspect network capture.",
        "net_log" => "Read captured network requests.",
        "net_clear" => "Clear captured network entries.",
        "net_intercept" => "Set or clear request interception rules.",
        "download_list" => "List recent downloads.",
        "download_wait" => "Wait for a download to finish.",
        "upload_file" => "Set local files on a file input.",
        "session_list" => "List recorded MCP sessions.",
        "session_steps" => "Read the steps of one session.",
        "session_replay" => "Plan or replay a recorded session.",
        "session_export" => "Export a session as JSON Lines.",
        "metrics" => "Uptime, tool counters, sessions and recording state.",
        "logs_tail" => "Read the in-process daemon log buffer.",
        "cdp_status" => "CDP connection, descriptor and ring counts.",
        "cdp_targets" => "List CDP targets.",
        "cdp_send" => "Send a raw CDP command.",
        "cdp_events" => "Read the bounded CDP event ring.",
        "browser_channel" => "Inspect or switch the browser channel.",
        "browser_console" => "Recent console and exception entries.",
        "browser_cookies" => "Browser cookies.",
        "browser_dialog" => "Accept or dismiss a JavaScript dialog.",
        "browser_emulate" => "Apply or reset device and network emulation.",
        "browser_batch" => "Run up to 20 browser tool calls in order.",
        "page_pdf" => "Print a tab to a PDF and save it.",
        "page_links" => "List links with optional visibility filtering.",
        "page_forms" => "Inspect forms, fields, labels and values.",
        "page_accessibility" => "Accessibility tree with optional root and limits.",
        "browser_drag" => "Drag the pointer between two elements.",
        "browser_focus" => "Focus an element and scroll it into view.",
        "browser_highlight" => "Show a transient overlay on an element.",
        "tab_group" => "Group tabs and set the group title and colour.",
        "tab_duplicate" => "Duplicate a tab.",
        "window_list" => "List browser windows and their tabs.",
        "window_new" => "Open a new browser window.",
        "cookie_set" => "Set one browser cookie.",
        "cookie_clear" => "Delete cookies by URL, domain or name.",
        "storage_get" => "Read localStorage or sessionStorage.",
        "storage_set" => "Write localStorage or sessionStorage.",
        "storage_clear" => "Clear localStorage or sessionStorage.",
        "health" => "Daemon status, version, channels and database health.",
        "version" => "Daemon name, version and catalogue counts.",
        "tool_help" => "Catalogue index, category or full tool help.",
        _ => "Corvus tool.",
    }
}

/// Every category in stable order.
pub fn categories() -> Vec<&'static str> {
    vec![
        "page",
        "input",
        "tabs",
        "network",
        "storage",
        "files",
        "cdp",
        "sessions",
        "diagnostics",
        "help",
    ]
}

/// Remove property descriptions from a brief input schema while keeping the
/// structural keywords clients need: type, enum, default, bounds and required.
fn strip_schema_descriptions(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.remove("description");
            for child in object.values_mut() {
                strip_schema_descriptions(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                strip_schema_descriptions(item);
            }
        }
        _ => {}
    }
}

/// Tool definitions filtered by category and detail level.
///
/// brief (the default) replaces the long description with a one-line summary;
/// full keeps the documented description.
pub fn definitions_for(category_filter: Option<&str>, detail: &str) -> Vec<Value> {
    SPECS
        .iter()
        .filter(|spec| category_filter.is_none_or(|wanted| category(spec.name) == wanted))
        .map(|spec| {
            let mut schema = (spec.schema)();
            if !matches!(spec.name, "health" | "version") {
                add_browser_id(&mut schema);
                if is_new_browser_tool(spec.name) {
                    if let Some(browser_id) = schema
                        .get_mut("properties")
                        .and_then(Value::as_object_mut)
                        .and_then(|properties| properties.get_mut("browser_id"))
                        .and_then(Value::as_object_mut)
                    {
                        browser_id.insert("minLength".to_string(), json!(1));
                        browser_id.insert("maxLength".to_string(), json!(128));
                    }
                    if let Some(tab_id) = schema
                        .get_mut("properties")
                        .and_then(Value::as_object_mut)
                        .and_then(|properties| properties.get_mut("tab_id"))
                        .and_then(Value::as_object_mut)
                    {
                        tab_id.insert("minLength".to_string(), json!(1));
                        tab_id.insert("maxLength".to_string(), json!(128));
                    }
                }
            }
            if detail != "full" {
                strip_schema_descriptions(&mut schema);
            }
            let description = if detail == "full" {
                spec.description
            } else {
                summary(spec.name)
            };
            let mut definition = json!({
                "name": spec.name,
                "title": spec.title,
                "description": description,
                "inputSchema": schema,
                "annotations": {
                    "readOnlyHint": spec.read_only,
                    "destructiveHint": spec.destructive,
                    "idempotentHint": spec.idempotent,
                    "openWorldHint": spec.open_world,
                },
                "_meta": { "corvus.category": category(spec.name) },
            });
            if detail != "full" {
                if let Some(object) = definition.as_object_mut() {
                    object.remove("title");
                }
            }
            definition
        })
        .collect()
}

/// Validate a tools/list params object and return its definitions.
pub fn list(params: &Value) -> Result<Vec<Value>, ErrorBody> {
    let category_filter = params.get("category").and_then(Value::as_str);
    if let Some(category) = category_filter {
        if !categories().contains(&category) {
            return Err(ErrorBody::new(
                ErrorCode::BadArgs,
                format!("unknown category {category}"),
            ));
        }
    }
    let detail = match params.get("detail").and_then(Value::as_str) {
        None => "brief",
        Some("brief") | Some("full") => params.get("detail").and_then(Value::as_str).unwrap(),
        Some(other) => {
            return Err(ErrorBody::new(
                ErrorCode::BadArgs,
                format!("tools/list detail must be brief or full, got {other}"),
            ))
        }
    };
    Ok(definitions_for(category_filter, detail))
}

/// Handle the tool_help tool.
pub fn help_document(arguments: &Value) -> Result<Value, ErrorBody> {
    let name = arguments.get("name").and_then(Value::as_str);
    let category_filter = arguments.get("category").and_then(Value::as_str);
    if name.is_some() && category_filter.is_some() {
        return Err(ErrorBody::new(
            ErrorCode::BadArgs,
            "tool_help accepts name or category, not both",
        ));
    }
    if let Some(name) = name {
        let spec = SPECS
            .iter()
            .find(|spec| spec.name == name)
            .ok_or_else(|| ErrorBody::new(ErrorCode::BadArgs, format!("unknown tool {name}")))?;
        let mut schema = (spec.schema)();
        add_browser_id(&mut schema);
        return Ok(json!({
            "name": spec.name,
            "title": spec.title,
            "category": category(spec.name),
            "summary": summary(spec.name),
            "description": spec.description,
            "inputSchema": schema,
            "annotations": {
                "readOnlyHint": spec.read_only,
                "destructiveHint": spec.destructive,
                "idempotentHint": spec.idempotent,
                "openWorldHint": spec.open_world,
            },
        }));
    }
    if let Some(category) = category_filter {
        if !categories().contains(&category) {
            return Err(ErrorBody::new(
                ErrorCode::BadArgs,
                format!("unknown category {category}"),
            ));
        }
        let tools: Vec<Value> = SPECS
            .iter()
            .filter(|spec| self::category(spec.name) == category)
            .map(|spec| {
                json!({
                    "name": spec.name,
                    "title": spec.title,
                    "summary": summary(spec.name),
                })
            })
            .collect();
        return Ok(json!({ "category": category, "count": tools.len(), "tools": tools }));
    }
    let index: Vec<Value> = categories()
        .into_iter()
        .map(|category| {
            let tools: Vec<&str> = SPECS
                .iter()
                .filter(|spec| self::category(spec.name) == category)
                .map(|spec| spec.name)
                .collect();
            json!({ "category": category, "count": tools.len(), "tools": tools })
        })
        .collect();
    Ok(json!({
        "categories": index,
        "usage": "Pass name for full help or category to list one group. tools/list accepts detail brief or full.",
    }))
}

/// Tools added in v1.7 that accept a bounded browser_id.
fn is_new_browser_tool(name: &str) -> bool {
    matches!(
        name,
        "page_pdf"
            | "page_links"
            | "page_forms"
            | "page_accessibility"
            | "browser_drag"
            | "browser_focus"
            | "browser_highlight"
            | "tab_group"
            | "tab_duplicate"
            | "window_list"
            | "window_new"
            | "cookie_set"
            | "cookie_clear"
            | "storage_get"
            | "storage_set"
            | "storage_clear"
    )
}

/// Add the optional routing field every browser tool accepts.
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
            "format": {
                "type": "string",
                "enum": ["compact", "json"],
                "default": "compact",
                "description": "compact returns the shared line grammar; json keeps the legacy node objects.",
            },
            "viewport_only": {
                "type": "boolean",
                "default": false,
                "description": "Only include nodes intersecting the current viewport.",
            },
            "include_bbox": {
                "type": "boolean",
                "default": false,
                "description": "Include viewport-relative bounding boxes in compact lines.",
            },
            "diff": {
                "type": "boolean",
                "default": false,
                "description": "Compact format only: mark added, changed and removed lines against the previous snapshot of the same tab.",
            },
        },
        "additionalProperties": false,
    })
}

fn screenshot_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "target": {
                "type": "string",
                "enum": ["viewport", "full_page", "element"],
                "default": "viewport",
                "description": "Region to capture: the visible viewport, the full scrollable page, or one element.",
            },
            "ref": {
                "type": "string",
                "description": "Element reference from page_snapshot such as @c42; required when target is element.",
            },
            "format": {
                "type": "string",
                "enum": ["webp", "png", "jpeg"],
                "default": "webp",
                "description": "Image encoding; quality is ignored for png.",
            },
            "quality": {
                "type": "integer",
                "minimum": 1,
                "maximum": 100,
                "default": 80,
                "description": "Encoder quality for webp and jpeg.",
            },
            "inline": {
                "type": "boolean",
                "default": false,
                "description": "Also return the image bytes as MCP image content; the file is always saved.",
            },
            "tab_id": {
                "type": "string",
                "description": "Tab to capture; the active tab when omitted.",
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

fn session_list_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": 200,
                "default": 50,
                "description": "Upper bound on the sessions returned, newest first.",
            },
            "since_ms": {
                "type": "integer",
                "minimum": 0,
                "description": "Only return sessions that started at or after this Unix time in milliseconds.",
            },
        },
        "additionalProperties": false,
    })
}

fn session_steps_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "session_id": {
                "type": "string",
                "description": "Recorded session to read.",
            },
            "from_seq": {
                "type": "integer",
                "minimum": 0,
                "default": 0,
                "description": "Return steps whose sequence number is at or above this value.",
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": 500,
                "default": 100,
                "description": "Upper bound on the steps returned.",
            },
        },
        "required": ["session_id"],
        "additionalProperties": false,
    })
}

fn session_replay_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "session_id": {
                "type": "string",
                "description": "Recorded session to replay.",
            },
            "from_seq": {
                "type": "integer",
                "minimum": 0,
                "description": "First recorded step to consider; the first step when omitted.",
            },
            "to_seq": {
                "type": "integer",
                "minimum": 0,
                "description": "Last recorded step to consider; the last step when omitted.",
            },
            "mode": {
                "type": "string",
                "enum": ["dry_run", "live"],
                "default": "dry_run",
                "description": "Plan only, or execute the replayable steps through the router.",
            },
            "continue_on_error": {
                "type": "boolean",
                "default": false,
                "description": "Keep replaying after a failing step instead of stopping.",
            },
        },
        "required": ["session_id"],
        "additionalProperties": false,
    })
}

fn session_export_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "session_id": {
                "type": "string",
                "description": "Recorded session to export.",
            },
            "format": {
                "type": "string",
                "enum": ["jsonl"],
                "default": "jsonl",
                "description": "Export encoding.",
            },
        },
        "required": ["session_id"],
        "additionalProperties": false,
    })
}

fn metrics_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false,
    })
}

fn logs_tail_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": 1000,
                "default": 200,
                "description": "Maximum number of entries to return.",
            },
            "level": {
                "type": "string",
                "enum": ["debug", "info", "warn", "error"],
                "description": "Minimum level to include.",
            },
            "since_ms": {
                "type": "integer",
                "minimum": 0,
                "description": "Only return entries at or after this epoch millisecond.",
            },
        },
        "additionalProperties": false,
    })
}

fn cdp_status_schema() -> Value {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

fn cdp_targets_schema() -> Value {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

fn cdp_send_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "method": { "type": "string", "description": "CDP method name, for example Browser.getVersion." },
            "params": { "type": "object", "description": "Method parameters; defaults to an empty object." },
            "session_id": { "type": "string", "description": "Flattened CDP session identifier." },
            "target_id": { "type": "string", "description": "Target to attach before sending the method." },
            "timeout_ms": { "type": "integer", "minimum": 1, "maximum": 60000, "default": 10000 },
        },
        "required": ["method"],
        "additionalProperties": false,
    })
}

fn cdp_events_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "since_seq": { "type": "integer", "minimum": 0, "default": 0 },
            "limit": { "type": "integer", "minimum": 1, "maximum": 500, "default": 100 },
            "method": { "type": "string", "description": "Only events with this CDP method name." },
            "wait_ms": { "type": "integer", "minimum": 0, "maximum": 30000, "default": 0 },
            "clear": { "type": "boolean", "default": false },
        },
        "additionalProperties": false,
    })
}

fn browser_channel_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": { "type": "string", "enum": ["status", "set"], "default": "status" },
            "channel": { "type": "string", "enum": ["auto", "cdp", "extension"] },
        },
        "additionalProperties": false,
    })
}

fn browser_console_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tab_id": { "type": "string", "description": "Target tab; the active tab when omitted." },
            "limit": { "type": "integer", "minimum": 1, "maximum": 500, "default": 100 },
        },
        "additionalProperties": false,
    })
}

fn browser_cookies_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tab_id": { "type": "string" },
            "urls": { "type": "array", "items": { "type": "string" }, "maxItems": 20 },
        },
        "additionalProperties": false,
    })
}

fn browser_dialog_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tab_id": { "type": "string" },
            "accept": { "type": "boolean", "default": true },
            "prompt_text": { "type": "string" },
        },
        "additionalProperties": false,
    })
}

fn browser_emulate_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tab_id": { "type": "string" },
            "reset": { "type": "boolean" },
            "width": { "type": "integer", "minimum": 1, "maximum": 100000 },
            "height": { "type": "integer", "minimum": 1, "maximum": 100000 },
            "device_scale_factor": { "type": "number", "minimum": 0.1, "maximum": 10 },
            "mobile": { "type": "boolean" },
            "offline": { "type": "boolean" },
            "latency_ms": { "type": "integer", "minimum": 0, "maximum": 600000 },
            "download_throughput": { "type": "number", "minimum": -1 },
            "upload_throughput": { "type": "number", "minimum": -1 },
            "user_agent": { "type": "string" },
            "timezone": { "type": "string" },
        },
        "additionalProperties": false,
    })
}

fn batch_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "steps": {
                "type": "array",
                "minItems": 1,
                "maxItems": BatchPolicy::MAX_STEPS,
                "description": "Browser tool calls to run in order.",
                "items": {
                    "type": "object",
                    "properties": {
                        "tool": {
                            "type": "string",
                            "description": "A browser tool name such as page_snapshot or browser_click.",
                        },
                        "args": {
                            "type": "object",
                            "description": "Arguments for the step tool; defaults to an empty object.",
                        },
                    },
                    "required": ["tool"],
                    "additionalProperties": false,
                },
            },
            "stop_on_error": {
                "type": "boolean",
                "default": true,
                "description": "Stop after the first failing step.",
            },
        },
        "required": ["steps"],
        "additionalProperties": false,
    })
}

fn tool_help_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": { "type": "string", "description": "Tool to describe." },
            "category": { "type": "string", "description": "Category to list." },
        },
        "additionalProperties": false,
    })
}

fn page_pdf_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tab_id": { "type": "string" },
            "paper_width": { "type": "number", "minimum": 1, "maximum": 100, "default": 8.5 },
            "paper_height": { "type": "number", "minimum": 1, "maximum": 100, "default": 11 },
            "landscape": { "type": "boolean", "default": false },
            "print_background": { "type": "boolean", "default": true },
            "scale": { "type": "number", "minimum": 0.1, "maximum": 2, "default": 1 },
        },
        "additionalProperties": false,
    })
}

fn page_links_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tab_id": { "type": "string" },
            "selector": { "type": "string", "maxLength": 512 },
            "max_results": { "type": "integer", "minimum": 1, "maximum": 1000, "default": 200 },
            "include_hidden": { "type": "boolean", "default": false },
        },
        "additionalProperties": false,
    })
}

fn page_forms_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tab_id": { "type": "string" },
            "selector": { "type": "string", "maxLength": 512 },
            "include_values": { "type": "boolean", "default": false },
            "max_forms": { "type": "integer", "minimum": 1, "maximum": 100, "default": 20 },
        },
        "additionalProperties": false,
    })
}

fn page_accessibility_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tab_id": { "type": "string" },
            "root": { "type": "string", "maxLength": 512 },
            "max_nodes": { "type": "integer", "minimum": 1, "maximum": 2000, "default": 500 },
            "max_depth": { "type": "integer", "minimum": 1, "maximum": 50, "default": 12 },
        },
        "additionalProperties": false,
    })
}

fn browser_drag_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tab_id": { "type": "string" },
            "from_ref": { "type": "string", "minLength": 1, "maxLength": 512 },
            "to_ref": { "type": "string", "minLength": 1, "maxLength": 512 },
            "steps": { "type": "integer", "minimum": 1, "maximum": 50, "default": 10 },
        },
        "required": ["from_ref", "to_ref"],
        "additionalProperties": false,
    })
}

fn browser_focus_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tab_id": { "type": "string" },
            "ref": { "type": "string", "minLength": 1, "maxLength": 512 },
        },
        "required": ["ref"],
        "additionalProperties": false,
    })
}

fn browser_highlight_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tab_id": { "type": "string" },
            "ref": { "type": "string", "minLength": 1, "maxLength": 512 },
            "selector": { "type": "string", "maxLength": 512 },
            "duration_ms": { "type": "integer", "minimum": 0, "maximum": 10000, "default": 1200 },
            "color": { "type": "string", "pattern": "^#[0-9a-fA-F]{6}$", "default": "#ffd400" },
        },
        "additionalProperties": false,
    })
}

fn tab_group_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tab_ids": {
                "type": "array",
                "items": { "type": "string", "minLength": 1, "maxLength": 128 },
                "minItems": 1,
                "maxItems": 50,
            },
            "title": { "type": "string", "maxLength": 64 },
            "color": {
                "type": "string",
                "enum": ["grey", "blue", "red", "yellow", "green", "pink", "purple", "cyan", "orange"],
                "default": "blue",
            },
        },
        "required": ["tab_ids"],
        "additionalProperties": false,
    })
}

fn tab_duplicate_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tab_id": { "type": "string", "minLength": 1 },
        },
        "required": ["tab_id"],
        "additionalProperties": false,
    })
}

fn window_list_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "include_tabs": { "type": "boolean", "default": true },
            "max_windows": { "type": "integer", "minimum": 1, "maximum": 50, "default": 20 },
        },
        "additionalProperties": false,
    })
}

fn window_new_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "url": { "type": "string", "maxLength": 4096 },
            "incognito": { "type": "boolean", "default": false },
            "focused": { "type": "boolean", "default": true },
            "width": { "type": "integer", "minimum": 200, "maximum": 10000 },
            "height": { "type": "integer", "minimum": 200, "maximum": 10000 },
        },
        "additionalProperties": false,
    })
}

fn cookie_set_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tab_id": { "type": "string" },
            "name": { "type": "string", "minLength": 1, "maxLength": 256 },
            "value": { "type": "string", "maxLength": 4096 },
            "url": { "type": "string", "maxLength": 4096 },
            "domain": { "type": "string", "maxLength": 256 },
            "path": { "type": "string", "maxLength": 512, "default": "/" },
            "secure": { "type": "boolean", "default": false },
            "http_only": { "type": "boolean", "default": false },
            "same_site": { "type": "string", "enum": ["strict", "lax", "none"], "default": "lax" },
            "expires": { "type": "integer", "minimum": 0 },
        },
        "required": ["name", "value"],
        "additionalProperties": false,
    })
}

fn cookie_clear_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tab_id": { "type": "string" },
            "url": { "type": "string", "maxLength": 4096 },
            "domain": { "type": "string", "maxLength": 256 },
            "name": { "type": "string", "minLength": 1, "maxLength": 256 },
        },
        "additionalProperties": false,
    })
}

fn storage_get_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tab_id": { "type": "string" },
            "area": { "type": "string", "enum": ["local", "session"], "default": "local" },
            "keys": {
                "type": "array",
                "items": { "type": "string", "maxLength": 256 },
                "maxItems": 100,
            },
        },
        "additionalProperties": false,
    })
}

fn storage_set_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tab_id": { "type": "string" },
            "area": { "type": "string", "enum": ["local", "session"], "default": "local" },
            "entries": {
                "type": "object",
                "minProperties": 1,
                "maxProperties": 100,
                "additionalProperties": { "type": "string", "maxLength": 4096 },
            },
        },
        "required": ["entries"],
        "additionalProperties": false,
    })
}

fn storage_clear_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tab_id": { "type": "string" },
            "area": { "type": "string", "enum": ["local", "session"], "default": "local" },
            "keys": {
                "type": "array",
                "items": { "type": "string", "maxLength": 256 },
                "maxItems": 100,
            },
        },
        "additionalProperties": false,
    })
}

fn health_schema() -> Value {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

fn version_schema() -> Value {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

fn validate_batch(object: &mut Map<String, Value>) -> Result<(), ErrorBody> {
    let steps = match object.get_mut("steps") {
        Some(Value::Array(steps)) if BatchPolicy::validate_count(steps.len()).is_ok() => steps,
        _ => {
            return Err(ErrorBody::new(
                ErrorCode::BadArgs,
                format!(
                    "browser_batch steps must be an array of 1 to {} steps",
                    BatchPolicy::MAX_STEPS
                ),
            ))
        }
    };
    for (index, step) in steps.iter_mut().enumerate() {
        let step_number = index + 1;
        let step = step.as_object_mut().ok_or_else(|| {
            ErrorBody::new(
                ErrorCode::BadArgs,
                format!("browser_batch step {step_number} must be an object"),
            )
        })?;
        if step.keys().any(|key| key != "tool" && key != "args") {
            return Err(ErrorBody::new(
                ErrorCode::BadArgs,
                format!("browser_batch step {step_number} has unknown fields"),
            ));
        }
        let tool = step
            .get("tool")
            .and_then(Value::as_str)
            .filter(|tool| !tool.is_empty())
            .map(str::to_string)
            .ok_or_else(|| {
                ErrorBody::new(
                    ErrorCode::BadArgs,
                    format!("browser_batch step {step_number} requires a non-empty tool"),
                )
            })?;
        if !batch_tool_allowed(&tool) {
            let message = if tool == "browser_batch" {
                "browser_batch cannot call itself".to_string()
            } else {
                format!("browser_batch step {step_number} tool {tool} is not allowed")
            };
            return Err(ErrorBody::new(ErrorCode::BadArgs, message)
                .with_details(json!({ "step": step_number, "tool": tool })));
        }
        let args = step.entry("args".to_string()).or_insert_with(|| json!({}));
        if !args.is_object() {
            return Err(ErrorBody::new(
                ErrorCode::BadArgs,
                format!("browser_batch step {step_number} args must be an object"),
            ));
        }
        let normalized = validate(&tool, args).map_err(|error| {
            ErrorBody::new(
                error.code,
                format!("browser_batch step {step_number}: {}", error.message),
            )
            .with_details(
                error
                    .details
                    .unwrap_or_else(|| json!({ "step": step_number })),
            )
        })?;
        *args = normalized;
    }
    match object.get("stop_on_error") {
        None => {
            object.insert("stop_on_error".to_string(), json!(true));
        }
        Some(Value::Bool(_)) => {}
        Some(_) => {
            return Err(ErrorBody::new(
                ErrorCode::BadArgs,
                "browser_batch stop_on_error must be a boolean",
            ))
        }
    }
    Ok(())
}

/// Check a tool call against the catalogue and normalise its arguments.
///
/// Everything this function rejects is a caller mistake, so the returned error
/// is always BAD_ARGS.
fn optional_string_max(
    object: &Map<String, Value>,
    field: &str,
    max: usize,
    tool: &str,
) -> Result<(), ErrorBody> {
    optional_string(object, field, tool)?;
    if let Some(text) = object.get(field).and_then(Value::as_str) {
        if text.chars().count() > max {
            return Err(ErrorBody::new(
                ErrorCode::BadArgs,
                format!("{tool} argument {field} must be at most {max} characters"),
            ));
        }
    }
    Ok(())
}

fn required_present_string(
    object: &Map<String, Value>,
    field: &str,
    max: usize,
    tool: &str,
) -> Result<(), ErrorBody> {
    match object.get(field) {
        Some(Value::String(text)) if text.chars().count() <= max => Ok(()),
        Some(Value::String(_)) => Err(ErrorBody::new(
            ErrorCode::BadArgs,
            format!("{tool} argument {field} must be at most {max} characters"),
        )),
        _ => Err(ErrorBody::new(
            ErrorCode::BadArgs,
            format!("{tool} requires a string {field} argument"),
        )),
    }
}

fn required_string_max(
    object: &Map<String, Value>,
    field: &str,
    max: usize,
    tool: &str,
) -> Result<(), ErrorBody> {
    required_string(object, field, tool)?;
    optional_string_max(object, field, max, tool)
}

fn optional_string_array(
    object: &Map<String, Value>,
    field: &str,
    max_items: usize,
    max_length: usize,
    tool: &str,
) -> Result<(), ErrorBody> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(()),
        Some(Value::Array(items))
            if items.len() <= max_items
                && items.iter().all(|item| {
                    item.as_str()
                        .is_some_and(|text| text.chars().count() <= max_length)
                }) =>
        {
            Ok(())
        }
        Some(_) => Err(ErrorBody::new(
            ErrorCode::BadArgs,
            format!(
                "{tool} argument {field} must be an array of at most {max_items} strings of at most {max_length} characters"
            ),
        )),
    }
}

fn validate_storage_entries(object: &Map<String, Value>) -> Result<(), ErrorBody> {
    let Some(entries) = object.get("entries").and_then(Value::as_object) else {
        return Err(ErrorBody::new(
            ErrorCode::BadArgs,
            "storage_set requires an entries object with 1 to 100 string values",
        ));
    };
    if entries.is_empty() || entries.len() > 100 {
        return Err(ErrorBody::new(
            ErrorCode::BadArgs,
            "storage_set requires an entries object with 1 to 100 string values",
        ));
    }
    let mut total = 0usize;
    for (key, value) in entries {
        if key.chars().count() > 256 {
            return Err(ErrorBody::new(
                ErrorCode::BadArgs,
                "storage_set keys must be at most 256 characters",
            ));
        }
        let Some(text) = value.as_str() else {
            return Err(ErrorBody::new(
                ErrorCode::BadArgs,
                "storage_set values must be strings",
            ));
        };
        if text.chars().count() > 4096 {
            return Err(ErrorBody::new(
                ErrorCode::BadArgs,
                "storage_set values must be at most 4096 characters",
            ));
        }
        total += key.len() + text.len();
    }
    if total > 65_536 {
        return Err(ErrorBody::new(
            ErrorCode::BadArgs,
            "storage_set entries must not exceed 64 KiB in total",
        ));
    }
    Ok(())
}

fn validate_highlight_color(object: &Map<String, Value>, tool: &str) -> Result<(), ErrorBody> {
    let Some(color) = object.get("color") else {
        return Ok(());
    };
    let Some(text) = color.as_str() else {
        return Err(ErrorBody::new(
            ErrorCode::BadArgs,
            format!("{tool} argument color must be a #rrggbb string"),
        ));
    };
    let valid = text.len() == 7
        && text.starts_with('#')
        && text[1..]
            .chars()
            .all(|character| character.is_ascii_hexdigit());
    if !valid {
        return Err(ErrorBody::new(
            ErrorCode::BadArgs,
            format!("{tool} argument color must be a #rrggbb string"),
        ));
    }
    Ok(())
}

/// Whether a window_new URL is well formed and uses an accepted scheme.
///
/// A prefix check is not enough: "http://" and "https:// example" pass a
/// starts_with test but have no usable host, and a browser window would fail
/// only after the call was accepted. Parse the URL instead.
fn valid_window_url(url: &str) -> bool {
    if url.trim().eq_ignore_ascii_case("about:blank") {
        return true;
    }
    url::Url::parse(url).is_ok_and(|parsed| {
        matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some()
    })
}

/// Validate and normalize the arguments of one tool call.
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
    if is_new_browser_tool(name) {
        if let Some(id) = object.get("browser_id").and_then(Value::as_str) {
            if id.is_empty() || id.chars().count() > 128 {
                return Err(ErrorBody::new(
                    ErrorCode::BadArgs,
                    format!("{name} argument browser_id must be 1 to 128 characters"),
                ));
            }
        }
    }
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
            optional_choice(&object, "format", &["compact", "json"], name)?;
            optional_bool(&object, "viewport_only", name)?;
            optional_bool(&object, "include_bbox", name)?;
            optional_bool(&object, "diff", name)?;
            fill_default(&mut object, "format", json!("compact"));
            fill_default(&mut object, "viewport_only", json!(false));
            fill_default(&mut object, "include_bbox", json!(false));
            fill_default(&mut object, "diff", json!(false));
            if object.get("diff").and_then(Value::as_bool) == Some(true)
                && object.get("format").and_then(Value::as_str) != Some("compact")
            {
                return Err(ErrorBody::new(
                    ErrorCode::BadArgs,
                    "page_snapshot diff is only supported with compact format",
                ));
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
        "session_list" => {
            optional_integer_range(&object, "limit", 1, 200, name)?;
            optional_integer_range(&object, "since_ms", 0, u64::MAX, name)?;
            fill_default(&mut object, "limit", json!(50));
        }
        "session_steps" => {
            required_string(&object, "session_id", name)?;
            optional_integer_range(&object, "from_seq", 0, u64::MAX, name)?;
            optional_integer_range(&object, "limit", 1, 500, name)?;
            fill_default(&mut object, "from_seq", json!(0));
            fill_default(&mut object, "limit", json!(100));
        }
        "session_replay" => {
            required_string(&object, "session_id", name)?;
            optional_integer_range(&object, "from_seq", 0, u64::MAX, name)?;
            optional_integer_range(&object, "to_seq", 0, u64::MAX, name)?;
            optional_choice(&object, "mode", &["dry_run", "live"], name)?;
            optional_bool(&object, "continue_on_error", name)?;
            if let (Some(from), Some(to)) = (
                object.get("from_seq").and_then(Value::as_u64),
                object.get("to_seq").and_then(Value::as_u64),
            ) {
                if from > to {
                    return Err(ErrorBody::new(
                        ErrorCode::BadArgs,
                        "session_replay from_seq must not exceed to_seq",
                    ));
                }
            }
            fill_default(&mut object, "mode", json!("dry_run"));
            fill_default(&mut object, "continue_on_error", json!(false));
        }
        "session_export" => {
            required_string(&object, "session_id", name)?;
            optional_choice(&object, "format", &["jsonl"], name)?;
            fill_default(&mut object, "format", json!("jsonl"));
        }
        "metrics" => {}
        "logs_tail" => {
            optional_integer_range(&object, "limit", 1, 1000, name)?;
            optional_integer_range(&object, "since_ms", 0, u64::MAX, name)?;
            if let Some(level) = object.get("level") {
                let valid = level
                    .as_str()
                    .is_some_and(|level| ["debug", "info", "warn", "error"].contains(&level));
                if !valid {
                    return Err(ErrorBody::new(
                        ErrorCode::BadArgs,
                        "logs_tail level must be one of debug, info, warn, error",
                    )
                    .with_details(json!({ "level": level })));
                }
            }
            fill_default(&mut object, "limit", json!(200));
        }
        "cdp_status" | "cdp_targets" => {}
        "cdp_send" => {
            required_string(&object, "method", name)?;
            match object.get("params") {
                None | Some(Value::Null) => {
                    object.insert("params".to_string(), json!({}));
                }
                Some(Value::Object(_)) => {}
                Some(_) => {
                    return Err(ErrorBody::new(
                        ErrorCode::BadArgs,
                        "cdp_send params must be an object",
                    ))
                }
            }
            optional_string(&object, "session_id", name)?;
            optional_string(&object, "target_id", name)?;
            optional_integer_range(&object, "timeout_ms", 1, 60_000, name)?;
            fill_default(&mut object, "timeout_ms", json!(10_000));
        }
        "cdp_events" => {
            optional_integer_range(&object, "since_seq", 0, u64::MAX, name)?;
            optional_integer_range(&object, "limit", 1, 500, name)?;
            optional_integer_range(&object, "wait_ms", 0, 30_000, name)?;
            optional_string(&object, "method", name)?;
            optional_bool(&object, "clear", name)?;
            fill_default(&mut object, "since_seq", json!(0));
            fill_default(&mut object, "limit", json!(100));
            fill_default(&mut object, "wait_ms", json!(0));
            fill_default(&mut object, "clear", json!(false));
        }
        "page_pdf" => {
            optional_string_max(&object, "tab_id", 128, name)?;
            optional_number_range(&object, "paper_width", 1.0, 100.0, name)?;
            optional_number_range(&object, "paper_height", 1.0, 100.0, name)?;
            optional_bool(&object, "landscape", name)?;
            optional_bool(&object, "print_background", name)?;
            optional_number_range(&object, "scale", 0.1, 2.0, name)?;
            fill_default(&mut object, "paper_width", json!(8.5));
            fill_default(&mut object, "paper_height", json!(11));
            fill_default(&mut object, "landscape", json!(false));
            fill_default(&mut object, "print_background", json!(true));
            fill_default(&mut object, "scale", json!(1));
        }
        "page_links" => {
            optional_string_max(&object, "tab_id", 128, name)?;
            optional_string_max(&object, "selector", 512, name)?;
            optional_integer_range(&object, "max_results", 1, 1000, name)?;
            optional_bool(&object, "include_hidden", name)?;
            fill_default(&mut object, "max_results", json!(200));
            fill_default(&mut object, "include_hidden", json!(false));
        }
        "page_forms" => {
            optional_string_max(&object, "tab_id", 128, name)?;
            optional_string_max(&object, "selector", 512, name)?;
            optional_bool(&object, "include_values", name)?;
            optional_integer_range(&object, "max_forms", 1, 100, name)?;
            fill_default(&mut object, "include_values", json!(false));
            fill_default(&mut object, "max_forms", json!(20));
        }
        "page_accessibility" => {
            optional_string_max(&object, "tab_id", 128, name)?;
            optional_string_max(&object, "root", 512, name)?;
            optional_integer_range(&object, "max_nodes", 1, 2000, name)?;
            optional_integer_range(&object, "max_depth", 1, 50, name)?;
            fill_default(&mut object, "max_nodes", json!(500));
            fill_default(&mut object, "max_depth", json!(12));
        }
        "browser_drag" => {
            optional_string_max(&object, "tab_id", 128, name)?;
            required_string_max(&object, "from_ref", 512, name)?;
            required_string_max(&object, "to_ref", 512, name)?;
            optional_integer_range(&object, "steps", 1, 50, name)?;
            fill_default(&mut object, "steps", json!(10));
            if object.get("from_ref") == object.get("to_ref") {
                return Err(ErrorBody::new(
                    ErrorCode::BadArgs,
                    "browser_drag from_ref and to_ref must differ",
                ));
            }
        }
        "browser_focus" => {
            optional_string_max(&object, "tab_id", 128, name)?;
            required_string_max(&object, "ref", 512, name)?;
        }
        "browser_highlight" => {
            optional_string_max(&object, "tab_id", 128, name)?;
            optional_string_max(&object, "ref", 512, name)?;
            optional_string_max(&object, "selector", 512, name)?;
            optional_integer_range(&object, "duration_ms", 0, 10_000, name)?;
            validate_highlight_color(&object, name)?;
            fill_default(&mut object, "duration_ms", json!(1200));
            fill_default(&mut object, "color", json!("#ffd400"));
            match (
                object.get("ref").and_then(Value::as_str),
                object.get("selector").and_then(Value::as_str),
            ) {
                (Some(_), None) | (None, Some(_)) => {}
                _ => {
                    return Err(ErrorBody::new(
                        ErrorCode::BadArgs,
                        "browser_highlight requires exactly one of ref or selector",
                    ))
                }
            }
        }
        "tab_group" => {
            let Some(Value::Array(tab_ids)) = object.get("tab_ids") else {
                return Err(ErrorBody::new(
                    ErrorCode::BadArgs,
                    "tab_group requires an array of 1 to 50 tab ids",
                ));
            };
            if tab_ids.is_empty()
                || tab_ids.len() > 50
                || !tab_ids.iter().all(|id| {
                    id.as_str()
                        .is_some_and(|text| !text.trim().is_empty() && text.chars().count() <= 128)
                })
            {
                return Err(ErrorBody::new(
                    ErrorCode::BadArgs,
                    "tab_group requires an array of 1 to 50 tab ids",
                ));
            }
            optional_string_max(&object, "title", 64, name)?;
            optional_choice(
                &object,
                "color",
                &[
                    "grey", "blue", "red", "yellow", "green", "pink", "purple", "cyan", "orange",
                ],
                name,
            )?;
            fill_default(&mut object, "color", json!("blue"));
        }
        "tab_duplicate" => {
            required_string_max(&object, "tab_id", 128, name)?;
        }
        "window_list" => {
            optional_bool(&object, "include_tabs", name)?;
            optional_integer_range(&object, "max_windows", 1, 50, name)?;
            fill_default(&mut object, "include_tabs", json!(true));
            fill_default(&mut object, "max_windows", json!(20));
        }
        "window_new" => {
            optional_string_max(&object, "url", 4096, name)?;
            optional_bool(&object, "incognito", name)?;
            optional_bool(&object, "focused", name)?;
            optional_integer_range(&object, "width", 200, 10_000, name)?;
            optional_integer_range(&object, "height", 200, 10_000, name)?;
            if let Some(url) = object.get("url").and_then(Value::as_str) {
                if !valid_window_url(url) {
                    return Err(ErrorBody::new(
                        ErrorCode::BadArgs,
                        "window_new url must be a valid http, https or about:blank URL",
                    ));
                }
            }
            fill_default(&mut object, "incognito", json!(false));
            fill_default(&mut object, "focused", json!(true));
        }
        "cookie_set" => {
            optional_string_max(&object, "tab_id", 128, name)?;
            required_string_max(&object, "name", 256, name)?;
            required_present_string(&object, "value", 4096, name)?;
            optional_string_max(&object, "url", 4096, name)?;
            optional_string_max(&object, "domain", 256, name)?;
            optional_string_max(&object, "path", 512, name)?;
            optional_bool(&object, "secure", name)?;
            optional_bool(&object, "http_only", name)?;
            optional_choice(&object, "same_site", &["strict", "lax", "none"], name)?;
            optional_integer_range(&object, "expires", 0, u64::MAX, name)?;
            fill_default(&mut object, "path", json!("/"));
            fill_default(&mut object, "secure", json!(false));
            fill_default(&mut object, "http_only", json!(false));
            fill_default(&mut object, "same_site", json!("lax"));
        }
        "cookie_clear" => {
            optional_string_max(&object, "tab_id", 128, name)?;
            optional_string_max(&object, "url", 4096, name)?;
            optional_string_max(&object, "domain", 256, name)?;
            optional_string_max(&object, "name", 256, name)?;
            if object
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|value| value.trim().is_empty())
            {
                return Err(ErrorBody::new(
                    ErrorCode::BadArgs,
                    "cookie_clear argument name must not be empty",
                ));
            }
        }
        "storage_get" => {
            optional_string_max(&object, "tab_id", 128, name)?;
            optional_choice(&object, "area", &["local", "session"], name)?;
            optional_string_array(&object, "keys", 100, 256, name)?;
            fill_default(&mut object, "area", json!("local"));
        }
        "storage_set" => {
            optional_string_max(&object, "tab_id", 128, name)?;
            optional_choice(&object, "area", &["local", "session"], name)?;
            validate_storage_entries(&object)?;
            fill_default(&mut object, "area", json!("local"));
        }
        "storage_clear" => {
            optional_string_max(&object, "tab_id", 128, name)?;
            optional_choice(&object, "area", &["local", "session"], name)?;
            optional_string_array(&object, "keys", 100, 256, name)?;
            fill_default(&mut object, "area", json!("local"));
        }
        "health" | "version" => {
            if !object.is_empty() {
                return Err(ErrorBody::new(
                    ErrorCode::BadArgs,
                    format!("{name} accepts no arguments"),
                ));
            }
        }
        "browser_batch" => {
            validate_batch(&mut object)?;
        }
        "browser_channel" => {
            optional_choice(&object, "action", &["status", "set"], name)?;
            optional_choice(&object, "channel", &["auto", "cdp", "extension"], name)?;
            fill_default(&mut object, "action", json!("status"));
            if object.get("action").and_then(Value::as_str) == Some("set")
                && object.get("channel").and_then(Value::as_str).is_none()
            {
                return Err(ErrorBody::new(
                    ErrorCode::BadArgs,
                    "browser_channel set requires a channel argument",
                ));
            }
        }
        "browser_console" => {
            optional_string(&object, "tab_id", name)?;
            optional_integer_range(&object, "limit", 1, 500, name)?;
            fill_default(&mut object, "limit", json!(100));
        }
        "browser_cookies" => {
            optional_string(&object, "tab_id", name)?;
            if let Some(urls) = object.get("urls") {
                match urls.as_array() {
                    Some(values)
                        if values.len() <= 20
                            && values.iter().all(|value| {
                                value.as_str().is_some_and(|text| text.len() <= 2048)
                            }) =>
                    {
                        if values.is_empty() {
                            object.remove("urls");
                        }
                    }
                    _ => {
                        return Err(ErrorBody::new(
                            ErrorCode::BadArgs,
                            "browser_cookies urls must be an array of at most 20 strings",
                        ))
                    }
                }
            }
        }
        "browser_screenshot" => {
            optional_choice(
                &object,
                "target",
                &["viewport", "full_page", "element"],
                name,
            )?;
            optional_string(&object, "ref", name)?;
            optional_choice(&object, "format", &["webp", "png", "jpeg"], name)?;
            optional_integer_range(&object, "quality", 1, 100, name)?;
            optional_bool(&object, "inline", name)?;
            optional_string(&object, "tab_id", name)?;
            fill_default(&mut object, "target", json!("viewport"));
            fill_default(&mut object, "format", json!("webp"));
            fill_default(&mut object, "quality", json!(80));
            fill_default(&mut object, "inline", json!(false));
            let target = object
                .get("target")
                .and_then(Value::as_str)
                .unwrap_or("viewport");
            if target == "element" {
                required_string(&object, "ref", name)?;
            } else if object.get("ref").is_some() {
                return Err(ErrorBody::new(
                    ErrorCode::BadArgs,
                    "browser_screenshot ref is only supported when target is element",
                ));
            }
        }
        "browser_dialog" => {
            optional_string(&object, "tab_id", name)?;
            optional_bool(&object, "accept", name)?;
            optional_string(&object, "prompt_text", name)?;
            fill_default(&mut object, "accept", json!(true));
        }
        "browser_emulate" => {
            optional_string(&object, "tab_id", name)?;
            optional_bool(&object, "reset", name)?;
            optional_bool(&object, "mobile", name)?;
            optional_bool(&object, "offline", name)?;
            optional_integer_range(&object, "width", 1, 100_000, name)?;
            optional_integer_range(&object, "height", 1, 100_000, name)?;
            if object.contains_key("width") != object.contains_key("height") {
                return Err(ErrorBody::new(
                    ErrorCode::BadArgs,
                    "browser_emulate width and height must be supplied together",
                ));
            }
            optional_number_range(&object, "device_scale_factor", 0.1, 10.0, name)?;
            optional_integer_range(&object, "latency_ms", 0, 600_000, name)?;
            optional_number_range(&object, "download_throughput", -1.0, 1.0e12, name)?;
            optional_number_range(&object, "upload_throughput", -1.0, 1.0e12, name)?;
            optional_string(&object, "user_agent", name)?;
            optional_string(&object, "timezone", name)?;
        }
        "tool_help" => {
            optional_string(&object, "name", name)?;
            optional_string(&object, "category", name)?;
            if object.get("name").is_some() && object.get("category").is_some() {
                return Err(ErrorBody::new(
                    ErrorCode::BadArgs,
                    "tool_help accepts name or category, not both",
                ));
            }
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
    fn every_definition_declares_a_category_and_summary() {
        let definitions = definitions_for(None, "brief");
        assert_eq!(definitions.len(), 62);
        let categories = categories();
        for definition in &definitions {
            let name = definition
                .get("name")
                .and_then(Value::as_str)
                .expect("name");
            let meta = definition.get("_meta").expect("_meta");
            let declared = meta
                .get("corvus.category")
                .and_then(Value::as_str)
                .expect("category");
            assert_eq!(declared, category(name));
            assert!(
                categories.contains(&declared),
                "{name} has unknown category"
            );
            let description = definition
                .get("description")
                .and_then(Value::as_str)
                .expect("description");
            assert_eq!(description, summary(name));
            assert!(!description.is_empty());
        }
    }

    #[test]
    fn brief_catalogue_fits_the_token_budget() {
        let brief = serde_json::to_string(&definitions_for(None, "brief")).expect("serialize");
        assert!(
            brief.len() <= 32_000,
            "brief catalogue is {} bytes",
            brief.len()
        );
        let full = serde_json::to_string(&definitions_for(None, "full")).expect("serialize");
        println!(
            "brief={} full={} tools={}",
            brief.len(),
            full.len(),
            definitions_for(None, "brief").len()
        );
    }

    #[test]
    fn new_tools_apply_defaults_and_reject_malformed_arguments() {
        let pdf = validate("page_pdf", &json!({})).expect("pdf defaults");
        assert_eq!(pdf["paper_width"], 8.5);
        assert_eq!(pdf["paper_height"], 11);
        assert_eq!(pdf["landscape"], false);
        assert_eq!(pdf["print_background"], true);
        assert_eq!(pdf["scale"], 1);
        assert!(validate("page_pdf", &json!({ "scale": 5 })).is_err());
        assert_eq!(
            validate("page_pdf", &json!({ "browser_id": "edge:default" })).expect("browser id")
                ["browser_id"],
            "edge:default"
        );
        assert!(validate("page_pdf", &json!({ "browser_id": "x".repeat(129) })).is_err());

        let links = validate("page_links", &json!({})).expect("link defaults");
        assert_eq!(links["max_results"], 200);
        assert_eq!(links["include_hidden"], false);
        assert!(validate("page_links", &json!({ "max_results": 0 })).is_err());

        let forms = validate("page_forms", &json!({})).expect("form defaults");
        assert_eq!(forms["include_values"], false);
        assert_eq!(forms["max_forms"], 20);
        let accessibility = validate("page_accessibility", &json!({})).expect("ax defaults");
        assert_eq!(accessibility["max_nodes"], 500);
        assert_eq!(accessibility["max_depth"], 12);

        assert!(validate("browser_drag", &json!({ "from_ref": "@e1" })).is_err());
        assert!(validate(
            "browser_drag",
            &json!({ "from_ref": "@e1", "to_ref": "@e1" })
        )
        .is_err());
        let drag = validate(
            "browser_drag",
            &json!({ "from_ref": "@e1", "to_ref": "@e2" }),
        )
        .expect("drag");
        assert_eq!(drag["steps"], 10);
        assert!(validate("browser_focus", &json!({})).is_err());

        assert!(validate("browser_highlight", &json!({})).is_err());
        assert!(validate(
            "browser_highlight",
            &json!({ "ref": "@e1", "selector": "#a" })
        )
        .is_err());
        let highlight =
            validate("browser_highlight", &json!({ "selector": "#a" })).expect("highlight");
        assert_eq!(highlight["duration_ms"], 1200);
        assert_eq!(highlight["color"], "#ffd400");
        assert!(validate(
            "browser_highlight",
            &json!({ "ref": "@e1", "color": "red" })
        )
        .is_err());

        assert!(validate("tab_group", &json!({ "tab_ids": [] })).is_err());
        let group = validate("tab_group", &json!({ "tab_ids": ["t1"] })).expect("group");
        assert_eq!(group["color"], "blue");
        assert!(validate("tab_duplicate", &json!({})).is_err());
        let windows = validate("window_list", &json!({})).expect("window list");
        assert_eq!(windows["include_tabs"], true);
        assert_eq!(windows["max_windows"], 20);
        assert!(validate("window_new", &json!({ "url": "mailto:a@b.test" })).is_err());
        let window = validate("window_new", &json!({})).expect("window new");
        assert_eq!(window["incognito"], false);
        assert_eq!(window["focused"], true);

        assert!(validate("cookie_set", &json!({ "name": "session" })).is_err());
        let cookie = validate("cookie_set", &json!({ "name": "session", "value": "" }))
            .expect("empty cookie value");
        assert_eq!(cookie["value"], "");
        assert_eq!(cookie["path"], "/");
        assert_eq!(cookie["same_site"], "lax");
        assert!(validate(
            "cookie_set",
            &json!({ "name": "session", "value": "x", "same_site": "maybe" })
        )
        .is_err());

        let storage = validate("storage_get", &json!({})).expect("storage get");
        assert_eq!(storage["area"], "local");
        assert!(validate("storage_set", &json!({})).is_err());
        let storage = validate(
            "storage_set",
            &json!({ "area": "session", "entries": { "token": "top-secret" } }),
        )
        .expect("storage set");
        assert_eq!(storage["area"], "session");
        assert_eq!(storage["entries"]["token"], "top-secret");
        let storage = validate(
            "storage_set",
            &json!({ "entries": { "x": "y".repeat(4097) } }),
        );
        assert!(storage.is_err());
        assert_eq!(
            validate("storage_clear", &json!({})).expect("storage clear")["area"],
            "local"
        );

        assert!(validate("health", &json!({})).is_ok());
        assert!(validate("health", &json!({ "verbose": true })).is_err());
        assert!(validate("version", &json!({})).is_ok());
        assert!(validate("version", &json!({ "detail": "full" })).is_err());
    }

    #[test]
    fn new_tools_enforce_hidden_length_and_url_bounds() {
        assert!(validate(
            "browser_drag",
            &json!({ "from_ref": "x".repeat(513), "to_ref": "@e2" })
        )
        .is_err());
        assert!(validate("browser_focus", &json!({ "ref": "x".repeat(513) })).is_err());
        assert!(validate("browser_highlight", &json!({ "ref": "x".repeat(513) })).is_err());
        assert!(validate("tab_group", &json!({ "tab_ids": ["x".repeat(129)] })).is_err());
        assert!(validate("storage_get", &json!({ "keys": ["x".repeat(257)] })).is_err());
        assert!(validate("storage_clear", &json!({ "keys": ["x".repeat(257)] })).is_err());

        for malformed in [
            "http://",
            "https:// example",
            "http://example.test:99999",
            "https://exa mple.test",
            "javascript:alert(1)",
            "about:blank#fragment",
        ] {
            assert!(
                validate("window_new", &json!({ "url": malformed })).is_err(),
                "{malformed} must be rejected"
            );
        }
        for accepted in [
            "http://example.test",
            "https://example.test/path?q=1",
            "about:blank",
        ] {
            assert!(
                validate("window_new", &json!({ "url": accepted })).is_ok(),
                "{accepted} must be accepted"
            );
        }

        assert!(validate("cookie_clear", &json!({ "name": "" })).is_err());
        assert!(validate("cookie_clear", &json!({ "name": "   " })).is_err());
        assert!(validate("cookie_clear", &json!({ "name": "session" })).is_ok());
    }

    #[test]
    fn new_tool_schemas_expose_their_hidden_bounds() {
        let definitions = definitions_for(None, "brief");
        let schema = |name: &str| {
            definitions
                .iter()
                .find(|definition| definition.get("name").and_then(Value::as_str) == Some(name))
                .and_then(|definition| definition.get("inputSchema"))
                .expect("schema")
        };
        assert_eq!(
            schema("browser_drag").pointer("/properties/from_ref/maxLength"),
            Some(&json!(512))
        );
        assert_eq!(
            schema("browser_drag").pointer("/properties/to_ref/maxLength"),
            Some(&json!(512))
        );
        assert_eq!(
            schema("browser_focus").pointer("/properties/ref/maxLength"),
            Some(&json!(512))
        );
        assert_eq!(
            schema("browser_highlight").pointer("/properties/ref/maxLength"),
            Some(&json!(512))
        );
        assert_eq!(
            schema("tab_group").pointer("/properties/tab_ids/items/maxLength"),
            Some(&json!(128))
        );
        assert_eq!(
            schema("storage_get").pointer("/properties/keys/items/maxLength"),
            Some(&json!(256))
        );
        assert_eq!(
            schema("storage_clear").pointer("/properties/keys/items/maxLength"),
            Some(&json!(256))
        );
    }

    #[test]
    fn brief_and_full_catalogues_share_structure_and_defaults() {
        fn strip_text(value: &mut Value) {
            match value {
                Value::Object(object) => {
                    object.remove("description");
                    object.remove("title");
                    for child in object.values_mut() {
                        strip_text(child);
                    }
                }
                Value::Array(items) => {
                    for item in items {
                        strip_text(item);
                    }
                }
                _ => {}
            }
        }
        let brief = definitions_for(None, "brief");
        let full = definitions_for(None, "full");
        assert_eq!(brief.len(), 62);
        assert_eq!(full.len(), 62);
        for (brief, full) in brief.iter().zip(full.iter()) {
            let name = brief.get("name").and_then(Value::as_str).expect("name");
            let mut brief = brief.clone();
            let mut full = full.clone();
            strip_text(&mut brief);
            strip_text(&mut full);
            assert_eq!(brief, full, "{name} differs between brief and full");
        }
    }

    #[test]
    fn browser_batch_is_bounded_normalised_and_rejects_recursion() {
        let batch = validate(
            "browser_batch",
            &json!({
                "steps": [
                    { "tool": "page_snapshot" },
                    { "tool": "tab_list", "args": {} }
                ]
            }),
        )
        .expect("batch");
        assert_eq!(
            batch.get("stop_on_error").and_then(Value::as_bool),
            Some(true)
        );
        let steps = batch.get("steps").and_then(Value::as_array).expect("steps");
        assert_eq!(steps.len(), 2);
        let snapshot = steps[0].get("args").expect("defaults");
        assert_eq!(
            snapshot.get("mode").and_then(Value::as_str),
            Some("interactive")
        );
        assert_eq!(
            snapshot.get("format").and_then(Value::as_str),
            Some("compact")
        );
        assert_eq!(
            snapshot.get("viewport_only").and_then(Value::as_bool),
            Some(false)
        );
        assert_eq!(
            snapshot.get("include_bbox").and_then(Value::as_bool),
            Some(false)
        );
        let lenient = validate(
            "browser_batch",
            &json!({ "steps": [{ "tool": "tab_list" }], "stop_on_error": false }),
        )
        .expect("lenient");
        assert_eq!(
            lenient.get("stop_on_error").and_then(Value::as_bool),
            Some(false)
        );
        assert!(batch_tool_allowed("page_snapshot"));
        assert!(batch_tool_allowed("browser_console"));
        assert!(batch_tool_allowed("cdp_send"));
        assert!(batch_tool_allowed("browser_list"));
        for refused in [
            "browser_batch",
            "browser_channel",
            "tool_help",
            "metrics",
            "logs_tail",
            "session_list",
        ] {
            assert!(!batch_tool_allowed(refused), "{refused} must be refused");
        }
        let recursive = validate(
            "browser_batch",
            &json!({ "steps": [{ "tool": "browser_batch" }] }),
        )
        .expect_err("recursion");
        assert_eq!(recursive.code, ErrorCode::BadArgs);
        assert!(recursive.message.contains("cannot call itself"));
        assert!(validate(
            "browser_batch",
            &json!({ "steps": [{ "tool": "metrics" }] }),
        )
        .is_err());
        let too_many: Vec<Value> = (0..21).map(|_| json!({ "tool": "tab_list" })).collect();
        assert!(validate("browser_batch", &json!({ "steps": too_many })).is_err());
    }

    #[test]
    fn brief_schemas_strip_property_descriptions_but_keep_structure() {
        let brief = definitions_for(Some("page"), "brief");
        let snapshot = brief
            .iter()
            .find(|definition| {
                definition.get("name").and_then(Value::as_str) == Some("page_snapshot")
            })
            .expect("page_snapshot");
        let format = snapshot
            .pointer("/inputSchema/properties/format")
            .expect("format property");
        assert_eq!(format.get("type").and_then(Value::as_str), Some("string"));
        assert_eq!(
            format.get("default").and_then(Value::as_str),
            Some("compact")
        );
        assert!(format.get("description").is_none());
        assert!(snapshot
            .pointer("/inputSchema/properties/max_nodes/description")
            .is_none());
        let full = definitions_for(Some("page"), "full");
        let snapshot = full
            .iter()
            .find(|definition| {
                definition.get("name").and_then(Value::as_str) == Some("page_snapshot")
            })
            .expect("full page_snapshot");
        assert!(snapshot
            .pointer("/inputSchema/properties/format/description")
            .and_then(Value::as_str)
            .is_some());
    }

    #[test]
    fn tool_help_returns_an_index_a_category_and_one_tool() {
        let index = help_document(&json!({})).expect("index");
        let groups = index
            .get("categories")
            .and_then(Value::as_array)
            .expect("categories");
        assert_eq!(groups.len(), categories().len());
        let network = help_document(&json!({ "category": "network" })).expect("category");
        assert_eq!(
            network.get("count").and_then(Value::as_u64),
            Some(5),
            "network category"
        );
        let one = help_document(&json!({ "name": "cdp_send" })).expect("tool");
        assert_eq!(one.get("category").and_then(Value::as_str), Some("cdp"));
        assert!(one.get("description").and_then(Value::as_str).is_some());
        assert!(help_document(&json!({ "category": "nope" })).is_err());
        assert!(help_document(&json!({ "name": "nope" })).is_err());
        assert!(help_document(&json!({ "name": "cdp_send", "category": "cdp" })).is_err());
    }

    #[test]
    fn new_tool_arguments_are_validated() {
        assert!(validate("cdp_send", &json!({})).is_err());
        assert!(validate("cdp_send", &json!({ "method": "Browser.getVersion" })).is_ok());
        assert!(validate("browser_channel", &json!({ "action": "set" })).is_err());
        assert!(validate(
            "browser_channel",
            &json!({ "action": "set", "channel": "cdp" })
        )
        .is_ok());
        assert!(validate("browser_emulate", &json!({ "width": 800 })).is_err());
        assert!(validate("browser_emulate", &json!({ "width": 800, "height": 600 })).is_ok());
        let events = validate("cdp_events", &json!({})).expect("defaults");
        assert_eq!(events.get("limit").and_then(Value::as_u64), Some(100));
        assert_eq!(events.get("wait_ms").and_then(Value::as_u64), Some(0));
        assert!(validate(
            "tool_help",
            &json!({ "name": "metrics", "category": "help" })
        )
        .is_err());
        assert!(validate(
            "browser_cookies",
            &json!({ "urls": ["https://example.test/"] })
        )
        .is_ok());
    }

    #[test]
    fn definitions_carry_names_schemas_and_annotations() {
        let definitions = definitions();
        assert_eq!(definitions.len(), 62);
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
                "browser_screenshot",
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
                "session_list",
                "session_steps",
                "session_replay",
                "session_export",
                "metrics",
                "logs_tail",
                "cdp_status",
                "cdp_targets",
                "cdp_send",
                "cdp_events",
                "browser_channel",
                "browser_console",
                "browser_cookies",
                "browser_dialog",
                "browser_emulate",
                "browser_batch",
                "tool_help",
                "page_pdf",
                "page_links",
                "page_forms",
                "page_accessibility",
                "browser_drag",
                "browser_focus",
                "browser_highlight",
                "tab_group",
                "tab_duplicate",
                "window_list",
                "window_new",
                "cookie_set",
                "cookie_clear",
                "storage_get",
                "storage_set",
                "storage_clear",
                "health",
                "version",
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
            if name == "health" || name == "version" {
                assert!(properties.is_empty(), "{name} must take no arguments");
                continue;
            }
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
        let screenshot = validate("browser_screenshot", &json!({})).expect("screenshot");
        assert_eq!(
            screenshot.get("target").and_then(Value::as_str),
            Some("viewport")
        );
        assert_eq!(
            screenshot.get("format").and_then(Value::as_str),
            Some("webp")
        );
        assert_eq!(screenshot.get("quality").and_then(Value::as_u64), Some(80));
        assert_eq!(
            screenshot.get("inline").and_then(Value::as_bool),
            Some(false)
        );
        let element = validate(
            "browser_screenshot",
            &json!({ "target": "element", "ref": "@c7" }),
        )
        .expect("element screenshot");
        assert_eq!(element.get("ref").and_then(Value::as_str), Some("@c7"));
        let snapshot = validate("page_snapshot", &json!({})).expect("page_snapshot");
        assert_eq!(
            snapshot.get("mode").and_then(Value::as_str),
            Some("interactive")
        );
        assert_eq!(
            snapshot.get("format").and_then(Value::as_str),
            Some("compact")
        );
        assert_eq!(
            snapshot.get("viewport_only").and_then(Value::as_bool),
            Some(false)
        );
        assert_eq!(
            snapshot.get("include_bbox").and_then(Value::as_bool),
            Some(false)
        );
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
        let sessions = validate("session_list", &json!({})).expect("session_list");
        assert_eq!(sessions.get("limit").and_then(Value::as_u64), Some(50));
        let logs = validate("logs_tail", &json!({})).expect("logs_tail");
        assert_eq!(logs.get("limit").and_then(Value::as_u64), Some(200));
        assert!(logs.get("level").is_none());
        assert!(validate("metrics", &json!({})).is_ok());
        let steps = validate("session_steps", &json!({ "session_id": "s-1" })).expect("steps");
        assert_eq!(steps.get("from_seq").and_then(Value::as_u64), Some(0));
        assert_eq!(steps.get("limit").and_then(Value::as_u64), Some(100));
        let replay = validate("session_replay", &json!({ "session_id": "s-1" })).expect("replay");
        assert_eq!(replay.get("mode").and_then(Value::as_str), Some("dry_run"));
        assert_eq!(
            replay.get("continue_on_error").and_then(Value::as_bool),
            Some(false)
        );
        let export = validate("session_export", &json!({ "session_id": "s-1" })).expect("export");
        assert_eq!(export.get("format").and_then(Value::as_str), Some("jsonl"));
    }

    #[test]
    fn snapshot_diff_is_compact_only() {
        let defaults = validate("page_snapshot", &json!({})).expect("defaults");
        assert_eq!(defaults.get("diff").and_then(Value::as_bool), Some(false));
        let diff = validate("page_snapshot", &json!({ "diff": true })).expect("diff");
        assert_eq!(diff.get("diff").and_then(Value::as_bool), Some(true));
        assert_eq!(diff.get("format").and_then(Value::as_str), Some("compact"));
        let error = validate("page_snapshot", &json!({ "diff": true, "format": "json" }))
            .expect_err("json diff");
        assert_eq!(error.code, ErrorCode::BadArgs);
        assert!(error.message.contains("compact"));
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
            ("browser_screenshot", json!({ "target": "element" })),
            ("browser_screenshot", json!({ "ref": "@c7" })),
            ("browser_screenshot", json!({ "format": "gif" })),
            ("browser_screenshot", json!({ "quality": 101 })),
            ("browser_screenshot", json!({ "inline": "yes" })),
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
            ("session_list", json!({ "limit": 0 })),
            ("session_list", json!({ "limit": 201 })),
            ("session_list", json!({ "since_ms": -1 })),
            ("session_steps", json!({})),
            ("session_steps", json!({ "session_id": "s", "limit": 501 })),
            ("session_replay", json!({})),
            (
                "session_replay",
                json!({ "session_id": "s", "mode": "fast" }),
            ),
            (
                "session_replay",
                json!({ "session_id": "s", "from_seq": 9, "to_seq": 3 }),
            ),
            (
                "session_replay",
                json!({ "session_id": "s", "continue_on_error": "yes" }),
            ),
            ("session_export", json!({})),
            (
                "session_export",
                json!({ "session_id": "s", "format": "csv" }),
            ),
            ("logs_tail", json!({ "limit": 0 })),
            ("logs_tail", json!({ "limit": 1001 })),
            ("logs_tail", json!({ "level": "verbose" })),
            ("logs_tail", json!({ "since_ms": -1 })),
            ("browser_wiggle", json!({})),
        ];
        for (tool, arguments) in cases {
            let error = validate(tool, &arguments).expect_err("must reject");
            assert_eq!(error.code, ErrorCode::BadArgs);
        }
    }
}
