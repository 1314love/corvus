//! Tool handlers executed over the browser-level CDP connection.
//!
//! Every handler preserves the result shape of the extension-backed tool it
//! replaces. CDP-only failures use the shared error codes plus a
//! transport:"cdp" detail block.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use base64::Engine as _;
use serde_json::{json, Map, Value};

use corvus_domain::SnapshotPolicy;

use crate::client::EVENT_CAPACITY;
use crate::snapshot::{self, Format, Mode, Options, PageInfo, Viewport};
use crate::{CdpClient, CdpError};

/// Existing tools that can be served through CDP.
pub const CDP_TOOL_NAMES: &[&str] = &[
    "browser_navigate",
    "browser_screenshot",
    "page_text",
    "page_html",
    "browser_eval",
    "browser_wait",
    "browser_history",
    "tab_list",
    "tab_new",
    "tab_close",
    "tab_switch",
    "net_capture",
    "net_log",
    "net_clear",
    "net_intercept",
    "download_list",
    "download_wait",
    "upload_file",
    "cdp_status",
    "cdp_targets",
    "cdp_send",
    "cdp_events",
    "browser_console",
    "browser_cookies",
    "browser_dialog",
    "browser_emulate",
    "page_snapshot",
    "page_find",
    "browser_click",
    "browser_type",
    "browser_hover",
    "browser_press",
    "browser_scroll",
    "browser_select",
    "page_pdf",
    "page_links",
    "page_forms",
    "page_accessibility",
    "browser_drag",
    "browser_focus",
    "browser_highlight",
    "cookie_set",
    "cookie_clear",
    "storage_get",
    "storage_set",
    "storage_clear",
];

/// Whether this tool can be answered over CDP.
pub fn is_cdp_tool(name: &str) -> bool {
    CDP_TOOL_NAMES.contains(&name)
}

/// Capability groups and the atomic actions each P2 provider must expose.
pub const P2_GROUPS: &[(&str, &[&str])] = &[
    ("tabs", &["tabs.list", "tabs.open", "tabs.close"]),
    ("page", &["page.navigate", "page.read", "page.snapshot"]),
    ("input", &["input.click", "input.type"]),
    ("capture", &["capture.viewport"]),
];

/// Every action the P2 worker accepts, including the local system query.
pub const P2_ACTIONS: &[&str] = &[
    "tabs.list",
    "tabs.open",
    "tabs.close",
    "page.navigate",
    "page.read",
    "page.snapshot",
    "input.click",
    "input.type",
    "capture.viewport",
    "system.info",
];

/// Capability groups in the JSON shape returned by system.info.
pub fn p2_capability_groups() -> Vec<Value> {
    P2_GROUPS
        .iter()
        .map(|(group, actions)| {
            json!({
                "group": group,
                "actions": actions,
            })
        })
        .collect()
}

/// Flat action identifiers advertised in a worker acknowledgement.
pub fn p2_capability_actions() -> Vec<String> {
    P2_GROUPS
        .iter()
        .flat_map(|(_, actions)| actions.iter().map(|action| (*action).to_string()))
        .collect()
}

/// Typed failure of one P2 worker action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum P2ActionError {
    /// The action or its arguments are invalid.
    BadArgs(String),
    /// The requested tab, element or reference does not exist.
    NotFound(String),
    /// The page could not complete the requested navigation.
    NavigationFailed(String),
    /// The operation exceeded its deadline.
    Timeout(String),
    /// The browser connection was lost.
    ProviderLost(String),
    /// The browser cannot perform this action.
    CapabilityUnavailable(String),
    /// The browser returned an unusable result.
    Internal(String),
}

impl P2ActionError {
    /// Stable machine code used in worker results.
    pub fn code(&self) -> &'static str {
        match self {
            Self::BadArgs(_) => "BAD_ARGS",
            Self::NotFound(_) => "NOT_FOUND",
            Self::NavigationFailed(_) => "NAVIGATION_FAILED",
            Self::Timeout(_) => "TIMEOUT",
            Self::ProviderLost(_) => "PROVIDER_LOST",
            Self::CapabilityUnavailable(_) => "CAPABILITY_UNAVAILABLE",
            Self::Internal(_) => "INTERNAL",
        }
    }

    /// Human-readable message that never includes transport details.
    pub fn message(&self) -> &str {
        match self {
            Self::BadArgs(message)
            | Self::NotFound(message)
            | Self::NavigationFailed(message)
            | Self::Timeout(message)
            | Self::ProviderLost(message)
            | Self::CapabilityUnavailable(message)
            | Self::Internal(message) => message,
        }
    }
}

impl std::fmt::Display for P2ActionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for P2ActionError {}

impl CdpClient {
    /// Execute one CDP-backed tool.
    pub async fn call_tool(&self, name: &str, args: &Value) -> Result<Value, CdpError> {
        if !is_cdp_tool(name) {
            return Err(CdpError::Unsupported(format!("{name} is not a CDP tool")));
        }
        match name {
            "browser_navigate" => self.navigate_tool(args).await,
            "browser_screenshot" => self.screenshot_tool(args).await,
            "page_text" => self.page_text_tool(args).await,
            "page_html" => self.page_html_tool(args).await,
            "browser_eval" => self.eval_tool(args).await,
            "browser_wait" => self.wait_tool(args).await,
            "browser_history" => self.history_tool(args).await,
            "tab_list" => self.tab_list_tool().await,
            "tab_new" => self.tab_new_tool(args).await,
            "tab_close" => self.tab_close_tool(args).await,
            "tab_switch" => self.tab_switch_tool(args).await,
            "net_capture" => self.net_capture_tool(args).await,
            "net_log" => self.net_log_tool(args).await,
            "net_clear" => self.net_clear_tool().await,
            "net_intercept" => self.net_intercept_tool(args).await,
            "download_list" => self.download_list_tool(args).await,
            "download_wait" => self.download_wait_tool(args).await,
            "upload_file" => self.upload_tool(args).await,
            "cdp_status" => self.status_tool().await,
            "cdp_targets" => self.targets_tool().await,
            "cdp_send" => self.send_tool(args).await,
            "cdp_events" => self.events_tool(args).await,
            "browser_console" => self.console_tool(args).await,
            "browser_cookies" => self.cookies_tool(args).await,
            "browser_dialog" => self.dialog_tool(args).await,
            "browser_emulate" => self.emulate_tool(args).await,
            "page_snapshot" => self.snapshot_tool(args).await,
            "page_find" => self.find_tool(args).await,
            "browser_click" => self.click_tool(args).await,
            "browser_type" => self.type_tool(args).await,
            "browser_hover" => self.hover_tool(args).await,
            "browser_press" => self.press_tool(args).await,
            "browser_scroll" => self.scroll_tool(args).await,
            "browser_select" => self.select_tool(args).await,
            "page_pdf" => self.page_pdf_tool(args).await,
            "page_links" => self.page_links_tool(args).await,
            "page_forms" => self.page_forms_tool(args).await,
            "page_accessibility" => self.page_accessibility_tool(args).await,
            "browser_drag" => self.drag_tool(args).await,
            "browser_focus" => self.focus_tool(args).await,
            "browser_highlight" => self.highlight_tool(args).await,
            "cookie_set" => self.cookie_set_tool(args).await,
            "cookie_clear" => self.cookie_clear_tool(args).await,
            "storage_get" => self.storage_get_tool(args).await,
            "storage_set" => self.storage_set_tool(args).await,
            "storage_clear" => self.storage_clear_tool(args).await,
            other => Err(CdpError::Unsupported(format!("{other} is not a CDP tool"))),
        }
    }

    async fn status_tool(&self) -> Result<Value, CdpError> {
        let mut value = serde_json::to_value(self.status())
            .map_err(|error| CdpError::Protocol(error.to_string()))?;
        if let Some(object) = value.as_object_mut() {
            object.insert("event_capacity".into(), json!(EVENT_CAPACITY));
            object.insert("event_count".into(), json!(self.event_count()));
            object.insert("tools".into(), json!(CDP_TOOL_NAMES));
        }
        Ok(value)
    }

    async fn targets_tool(&self) -> Result<Value, CdpError> {
        let targets = serde_json::to_value(self.targets())
            .map_err(|error| CdpError::Protocol(error.to_string()))?;
        Ok(json!({ "targets": targets }))
    }

    async fn send_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let method = required_string(args, "method")?;
        let params = match args.get("params") {
            Some(value) if value.is_object() => value.clone(),
            Some(Value::Null) | None => json!({}),
            Some(_) => return Err(CdpError::InvalidArgument("params must be an object".into())),
        };
        let session_id = optional_string(args, "session_id");
        let target_id = optional_string(args, "target_id");
        let timeout = timeout_ms(args, 10_000, 60_000);
        let result = self
            .send(
                &method,
                params,
                session_id.as_deref(),
                target_id.as_deref(),
                Some(timeout),
            )
            .await?;
        Ok(json!({ "method": method, "result": result }))
    }

    async fn events_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let since = args.get("since_seq").and_then(Value::as_u64).unwrap_or(0);
        let limit = integer(args, "limit", 100, 1, 500) as usize;
        let method = optional_string(args, "method");
        let wait_ms = integer(args, "wait_ms", 0, 0, 30_000);
        let clear = args.get("clear").and_then(Value::as_bool).unwrap_or(false);
        let events = self
            .events(
                Some(since),
                Some(limit),
                method.as_deref(),
                Some(wait_ms),
                clear,
            )
            .await;
        serde_json::to_value(events).map_err(|error| CdpError::Protocol(error.to_string()))
    }

    async fn navigate_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let url = required_string(args, "url")?;
        let (session, target_id) = self.page(args, 30_000).await?;
        let response = self
            .send(
                "Page.navigate",
                json!({ "url": url }),
                Some(&session),
                None,
                Some(30_000),
            )
            .await?;
        if let Some(error) = response.get("errorText").and_then(Value::as_str) {
            return Err(CdpError::Protocol(error.to_string()));
        }
        self.wait_ready(&session, Duration::from_secs(30)).await?;
        let current = self.eval_value(&session, "location.href", false).await?;
        Ok(json!({
            "loaded": true,
            "url": current,
            "tab_id": target_id,
        }))
    }

    async fn page_text_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let (session, target_id) = self.page(args, 15_000).await?;
        let expression = "(() => ({ url: location.href, title: document.title, text: (document.body ? document.body.innerText : (document.documentElement ? document.documentElement.innerText : '')) }))()";
        let value = self.eval_value(&session, expression, false).await?;
        let max_chars = integer(args, "max_chars", 100_000, 1, 200_000) as usize;
        let text = value
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let (text, truncated) = truncate(text, max_chars);
        Ok(json!({
            "tab_id": target_id,
            "url": value.get("url").cloned().unwrap_or(Value::Null),
            "title": value.get("title").cloned().unwrap_or(Value::Null),
            "text": text,
            "truncated": truncated,
        }))
    }

    async fn page_html_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let (session, target_id) = self.page(args, 15_000).await?;
        let selector = optional_string(args, "target");
        let expression = match selector {
            Some(selector) => format!(
                "(() => {{ const element = document.querySelector({}); return {{ url: location.href, html: element ? element.outerHTML : null }}; }})()",
                js_string(&selector)
            ),
            None => "(() => ({ url: location.href, html: document.documentElement ? document.documentElement.outerHTML : '' }))()".to_string(),
        };
        let value = self.eval_value(&session, &expression, false).await?;
        let html = value
            .get("html")
            .and_then(Value::as_str)
            .ok_or_else(|| CdpError::TargetNotFound("element selector".into()))?
            .to_string();
        let max_chars = integer(args, "max_chars", 200_000, 1, 500_000) as usize;
        let (html, truncated) = truncate(html, max_chars);
        Ok(json!({
            "tab_id": target_id,
            "url": value.get("url").cloned().unwrap_or(Value::Null),
            "html": html,
            "truncated": truncated,
        }))
    }

    async fn eval_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let expression = required_string(args, "expression")?;
        let (session, _) = self.page(args, 15_000).await?;
        let response = self
            .send(
                "Runtime.evaluate",
                json!({
                    "expression": expression,
                    "returnByValue": true,
                    "awaitPromise": true,
                    "userGesture": true,
                }),
                Some(&session),
                None,
                Some(15_000),
            )
            .await?;
        if let Some(exception) = response.get("exceptionDetails") {
            let name = exception
                .pointer("/exception/className")
                .and_then(Value::as_str)
                .or_else(|| exception.get("text").and_then(Value::as_str))
                .unwrap_or("Error");
            let message = exception
                .pointer("/exception/description")
                .and_then(Value::as_str)
                .or_else(|| exception.get("text").and_then(Value::as_str))
                .unwrap_or("evaluation failed");
            return Ok(json!({
                "ok": false,
                "error": { "name": name, "message": message },
            }));
        }
        let value = response
            .pointer("/result/value")
            .cloned()
            .or_else(|| response.pointer("/result/description").cloned())
            .unwrap_or(Value::Null);
        Ok(json!({ "ok": true, "value": value }))
    }

    /// A navigation may destroy the JavaScript context between two polls.
    /// Only that narrow class of protocol error is treated as a missed poll;
    /// transport and timeout failures must surface to the caller.
    async fn current_url(&self, session: &str) -> Result<Value, CdpError> {
        match self.eval_value(session, "location.href", false).await {
            Ok(url) => Ok(url),
            Err(error) if is_transient_context_error(&error) => Ok(Value::Null),
            Err(error) => Err(error),
        }
    }

    async fn wait_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let (session, _) = self.page(args, 15_000).await?;
        let selector = optional_string(args, "selector");
        let text = optional_string(args, "text");
        let url_contains = optional_string(args, "url_contains");
        let expression = match (selector, text, url_contains) {
            (Some(selector), _, _) => format!("!!document.querySelector({})", js_string(&selector)),
            (None, Some(text), _) => format!(
                "(document.body ? document.body.innerText.includes({}) : false)",
                js_string(&text)
            ),
            (None, None, Some(fragment)) => {
                format!("location.href.includes({})", js_string(&fragment))
            }
            (None, None, None) => {
                return Err(CdpError::InvalidArgument(
                    "browser_wait needs selector, text or url_contains".into(),
                ))
            }
        };
        let timeout = integer(args, "timeout_ms", 10_000, 100, 120_000);
        let started = Instant::now();
        let deadline = started + Duration::from_millis(timeout);
        loop {
            let matched = match self.eval_value(&session, &expression, false).await {
                Ok(value) => value,
                Err(error) if is_transient_context_error(&error) => Value::Bool(false),
                Err(error) => return Err(error),
            };
            if matched.as_bool() == Some(true) {
                return Ok(json!({
                    "matched": true,
                    "waited_ms": started.elapsed().as_millis() as u64,
                    "url": self.current_url(&session).await?,
                }));
            }
            if Instant::now() >= deadline {
                return Ok(json!({
                    "matched": false,
                    "waited_ms": started.elapsed().as_millis() as u64,
                    "url": self.current_url(&session).await?,
                }));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn history_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let action = required_string(args, "action")?;
        let (session, target_id) = self.page(args, 15_000).await?;
        let history = self
            .send(
                "Page.getNavigationHistory",
                json!({}),
                Some(&session),
                None,
                Some(10_000),
            )
            .await?;
        let current = history
            .get("currentIndex")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let entries = history
            .get("entries")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let index = match action.as_str() {
            "back" => current - 1,
            "forward" => current + 1,
            "reload" => current,
            other => {
                return Err(CdpError::InvalidArgument(format!(
                    "unknown browser_history action {other}"
                )))
            }
        };
        if action != "reload" && (index < 0 || index as usize >= entries.len()) {
            let url = entries
                .get(current.max(0) as usize)
                .and_then(|entry| entry.get("url"))
                .cloned()
                .unwrap_or(Value::Null);
            return Ok(
                json!({ "action": action, "url": url, "loaded": false, "tab_id": target_id }),
            );
        }
        if action == "reload" {
            self.send("Page.reload", json!({}), Some(&session), None, Some(30_000))
                .await?;
        } else {
            let entry_id = entries
                .get(index as usize)
                .and_then(|entry| entry.get("id"))
                .cloned()
                .unwrap_or(Value::Null);
            self.send(
                "Page.navigateToHistoryEntry",
                json!({ "entryId": entry_id }),
                Some(&session),
                None,
                Some(30_000),
            )
            .await?;
        }
        self.wait_ready(&session, Duration::from_secs(30)).await?;
        let url = self.eval_value(&session, "location.href", false).await?;
        Ok(json!({ "action": action, "url": url, "loaded": true, "tab_id": target_id }))
    }

    async fn tab_list_tool(&self) -> Result<Value, CdpError> {
        let active = self.default_target_id();
        let tabs: Vec<Value> = self
            .targets()
            .into_iter()
            .enumerate()
            .map(|(index, target)| {
                json!({
                    "tab_id": target.target_id,
                    "index": index,
                    "active": active.as_deref() == Some(target.target_id.as_str()),
                    "url": target.url,
                    "title": target.title,
                    "status": target.status.unwrap_or_else(|| "complete".into()),
                })
            })
            .collect();
        Ok(json!({ "tabs": tabs }))
    }

    async fn tab_new_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let url = optional_string(args, "url").unwrap_or_else(|| "about:blank".into());
        let active = args.get("active").and_then(Value::as_bool).unwrap_or(true);
        let previous = self.default_target_id();
        let response = self
            .send(
                "Target.createTarget",
                json!({ "url": "about:blank", "background": !active }),
                None,
                None,
                Some(15_000),
            )
            .await?;
        let target_id = response
            .get("targetId")
            .and_then(Value::as_str)
            .ok_or_else(|| CdpError::Protocol("createTarget returned no targetId".into()))?
            .to_string();
        self.remember_page_target(&target_id, "about:blank");
        if active {
            let _ = self
                .send(
                    "Target.activateTarget",
                    json!({ "targetId": target_id }),
                    None,
                    None,
                    Some(10_000),
                )
                .await;
            let _ = self.set_active_target(&target_id);
        }
        // Navigate only after the new session is attached so the load event
        // belongs to the final document. Reading the committed URL instead of
        // echoing the requested one keeps the redirect policy gate honest.
        let selector = json!({ "tab_id": target_id.as_str() });
        let (session, _) = self.page(&selector, 30_000).await?;
        if url != "about:blank" {
            let since = self.events(None, Some(1), None, None, false).await.next_seq;
            let response = self
                .send(
                    "Page.navigate",
                    json!({ "url": url }),
                    Some(&session),
                    None,
                    Some(30_000),
                )
                .await?;
            if let Some(error) = response.get("errorText").and_then(Value::as_str) {
                return Err(CdpError::Protocol(error.to_string()));
            }
            self.wait_for_load_event(&session, since, Duration::from_secs(30))
                .await?;
        }
        let loaded_url = self
            .wait_loaded_url(&session, &url, Duration::from_secs(5))
            .await?;
        self.remember_page_target(&target_id, &loaded_url);
        Ok(json!({
            "tab_id": target_id,
            "url": loaded_url,
            "loaded": true,
            "previous_tab_id": previous,
        }))
    }

    async fn tab_close_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let target_id = required_string(args, "tab_id")?;
        if !self.is_page_target(&target_id) {
            return Err(CdpError::TargetNotFound(target_id));
        }
        self.send(
            "Target.closeTarget",
            json!({ "targetId": target_id }),
            None,
            None,
            Some(15_000),
        )
        .await?;
        self.forget_target(&target_id);
        let next = self.default_target_id().or_else(|| {
            self.targets()
                .into_iter()
                .next()
                .map(|target| target.target_id)
        });
        Ok(json!({
            "closed": true,
            "tab_id": target_id,
            "next_tab_id": next,
        }))
    }

    async fn tab_switch_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let target_id = required_string(args, "tab_id")?;
        let target = self
            .targets()
            .into_iter()
            .find(|target| target.target_id == target_id)
            .ok_or_else(|| CdpError::TargetNotFound(target_id.clone()))?;
        self.send(
            "Target.activateTarget",
            json!({ "targetId": target_id }),
            None,
            None,
            Some(10_000),
        )
        .await?;
        if !self.set_active_target(&target_id) {
            return Err(CdpError::TargetNotFound(target_id));
        }
        Ok(json!({
            "tab_id": target.target_id,
            "url": target.url,
            "title": target.title,
            "active": true,
        }))
    }

    async fn net_capture_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let action = required_string(args, "action")?;
        let (session, target_id) = self.page(args, 15_000).await?;
        match action.as_str() {
            "start" => {
                self.send(
                    "Network.enable",
                    json!({}),
                    Some(&session),
                    None,
                    Some(10_000),
                )
                .await?;
                Ok(json!({
                    "capturing": true,
                    "tab_id": target_id,
                    "started_at": now_ms(),
                    "capacity": EVENT_CAPACITY,
                }))
            }
            "stop" => {
                self.send(
                    "Network.disable",
                    json!({}),
                    Some(&session),
                    None,
                    Some(10_000),
                )
                .await?;
                let events = self.events(Some(0), Some(500), None, None, false).await;
                let entries = events
                    .events
                    .iter()
                    .filter(|event| event.method.starts_with("Network."))
                    .count();
                Ok(json!({ "capturing": false, "tab_id": target_id, "entries": entries }))
            }
            "status" => {
                let events = self.events(Some(0), Some(500), None, None, false).await;
                let entries = events
                    .events
                    .iter()
                    .filter(|event| event.method.starts_with("Network."))
                    .count();
                Ok(json!({
                    "capturing": self.is_connected(),
                    "tab_id": target_id,
                    "started_at": self.status().connected_since_ms.unwrap_or_else(now_ms),
                    "entries": entries,
                }))
            }
            other => Err(CdpError::InvalidArgument(format!(
                "unknown net_capture action {other}"
            ))),
        }
    }

    async fn net_log_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let since_seq = args.get("since_seq").and_then(Value::as_u64).unwrap_or(0);
        let limit = integer(args, "limit", 100, 1, 1000) as usize;
        let include_headers = args
            .get("include_headers")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let method_filter = optional_string(args, "method").map(|value| value.to_uppercase());
        let url_filter = optional_string(args, "url_contains");
        let type_filter = optional_string(args, "resource_type");
        let status_min = integer(args, "status_min", 0, 0, 599);
        let status_max = integer(args, "status_max", 0, 0, 599);
        let events = self
            .events(Some(since_seq), Some(500), None, None, false)
            .await;
        let mut entries: Vec<(u64, Value)> = Vec::new();
        let mut starts: HashMap<String, f64> = HashMap::new();
        for event in &events.events {
            let Some(request_id) = event
                .params
                .get("requestId")
                .and_then(Value::as_str)
                .map(str::to_string)
            else {
                continue;
            };
            match event.method.as_str() {
                "Network.requestWillBeSent" => {
                    let request = event.params.get("request").cloned().unwrap_or_default();
                    let timestamp = event
                        .params
                        .get("timestamp")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0);
                    starts.insert(request_id.clone(), timestamp);
                    let entry = json!({
                        "seq": event.seq,
                        "ts": event.ts,
                        "tab_id": self.target_for_session(event.session_id.as_deref()).unwrap_or_else(|| "active".into()),
                        "request_id": request_id,
                        "method": request.get("method").cloned().unwrap_or(Value::Null),
                        "url": request.get("url").cloned().unwrap_or(Value::Null),
                        "status": Value::Null,
                        "mime": Value::Null,
                        "resource_type": event.params.get("type").cloned().unwrap_or(Value::Null),
                        "from_cache": false,
                        "duration_ms": 0,
                        "error": Value::Null,
                        "intercepted": Value::Null,
                        "request_headers": if include_headers { request.get("headers").cloned().unwrap_or(Value::Null) } else { Value::Null },
                        "response_headers": Value::Null,
                    });
                    entries.push((event.seq, entry));
                }
                "Network.responseReceived" => {
                    let response = event.params.get("response").cloned().unwrap_or_default();
                    let timestamp = event
                        .params
                        .get("timestamp")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0);
                    let started = starts.get(&request_id).copied().unwrap_or(timestamp);
                    if let Some((_, entry)) = entries.iter_mut().find(|(_, entry)| {
                        entry.get("request_id").and_then(Value::as_str) == Some(request_id.as_str())
                    }) {
                        entry["status"] = response.get("status").cloned().unwrap_or(Value::Null);
                        entry["mime"] = response.get("mimeType").cloned().unwrap_or(Value::Null);
                        entry["from_cache"] = json!(
                            response
                                .get("fromDiskCache")
                                .and_then(Value::as_bool)
                                .unwrap_or(false)
                                || response
                                    .get("fromServiceWorker")
                                    .and_then(Value::as_bool)
                                    .unwrap_or(false)
                        );
                        entry["duration_ms"] =
                            json!(((timestamp - started).max(0.0) * 1000.0).round() as u64);
                        if include_headers {
                            entry["response_headers"] =
                                response.get("headers").cloned().unwrap_or(Value::Null);
                        }
                    }
                }
                "Network.loadingFailed" => {
                    if let Some((_, entry)) = entries.iter_mut().find(|(_, entry)| {
                        entry.get("request_id").and_then(Value::as_str) == Some(request_id.as_str())
                    }) {
                        entry["error"] = event
                            .params
                            .get("errorText")
                            .cloned()
                            .unwrap_or(Value::Null);
                    }
                }
                _ => {}
            }
        }
        let mut selected: Vec<Value> = entries
            .into_iter()
            .filter(|(_, entry)| {
                let method_ok = method_filter.as_ref().is_none_or(|wanted| {
                    entry.get("method").and_then(Value::as_str) == Some(wanted.as_str())
                });
                let url_ok = url_filter.as_ref().is_none_or(|fragment| {
                    entry
                        .get("url")
                        .and_then(Value::as_str)
                        .is_some_and(|url| url.contains(fragment))
                });
                let type_ok = type_filter.as_ref().is_none_or(|wanted| {
                    entry.get("resource_type").and_then(Value::as_str) == Some(wanted.as_str())
                });
                let status = entry.get("status").and_then(Value::as_u64).unwrap_or(0);
                let status_ok = (status_min == 0 || status >= status_min)
                    && (status_max == 0 || status <= status_max);
                method_ok && url_ok && type_ok && status_ok
            })
            .map(|(_, entry)| entry)
            .take(limit)
            .collect();
        selected.sort_by_key(|entry| entry.get("seq").and_then(Value::as_u64).unwrap_or(0));
        Ok(json!({
            "entries": selected,
            "next_seq": events.next_seq,
            "capturing": self.is_connected(),
        }))
    }

    async fn net_clear_tool(&self) -> Result<Value, CdpError> {
        let mut cleared = 0u64;
        for _ in 0..8 {
            let events = self.events(Some(0), Some(500), None, None, true).await;
            let count = events.events.len() as u64;
            cleared += count;
            if count < 500 {
                break;
            }
        }
        Ok(json!({ "cleared": cleared }))
    }

    async fn net_intercept_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let action = required_string(args, "action")?;
        let tab_id = optional_string(args, "tab_id");
        let scope = if tab_id.is_some() { "tab" } else { "browser" };
        match action.as_str() {
            "set" => {
                let rules = args
                    .get("rules")
                    .and_then(Value::as_array)
                    .cloned()
                    .ok_or_else(|| {
                        CdpError::InvalidArgument("rules is required for action set".into())
                    })?;
                if rules.is_empty() || rules.len() > 50 {
                    return Err(CdpError::InvalidArgument(
                        "rules must hold 1..50 entries".into(),
                    ));
                }
                let (session, _) = self.page(args, 15_000).await?;
                self.send(
                    "Fetch.enable",
                    json!({ "patterns": [{ "urlPattern": "*", "requestStage": "Request" }] }),
                    Some(&session),
                    None,
                    Some(10_000),
                )
                .await?;
                let start_task = {
                    let mut state = self
                        .intercept
                        .lock()
                        .map_err(|_| CdpError::Protocol("intercept state poisoned".into()))?;
                    state.rules = rules.clone();
                    let start = !state.running;
                    state.running = true;
                    start
                };
                if start_task {
                    let client = self.clone();
                    tokio::spawn(async move {
                        client.intercept_loop().await;
                    });
                }
                let mut response = json!({
                    "ok": true,
                    "scope": scope,
                    "rules": rules,
                    "applied": true,
                });
                if let Some(tab_id) = tab_id {
                    response["tab_id"] = json!(tab_id);
                }
                Ok(response)
            }
            "clear" => {
                if self.is_connected() {
                    if let Ok((session, _)) = self.page(args, 5_000).await {
                        let _ = self
                            .send(
                                "Fetch.disable",
                                json!({}),
                                Some(&session),
                                None,
                                Some(10_000),
                            )
                            .await;
                    }
                }
                if let Ok(mut state) = self.intercept.lock() {
                    state.rules.clear();
                    state.running = false;
                }
                let mut response = json!({
                    "ok": true,
                    "scope": scope,
                    "rules": [],
                    "applied": true,
                });
                if let Some(tab_id) = tab_id {
                    response["tab_id"] = json!(tab_id);
                }
                Ok(response)
            }
            other => Err(CdpError::InvalidArgument(format!(
                "unknown net_intercept action {other}"
            ))),
        }
    }

    async fn intercept_loop(&self) {
        let mut since = 0u64;
        loop {
            let (running, rules) = match self.intercept.lock() {
                Ok(state) => (state.running, state.rules.clone()),
                Err(_) => return,
            };
            if !running {
                return;
            }
            if !self.is_connected() {
                tokio::time::sleep(Duration::from_millis(250)).await;
                continue;
            }
            let events = self
                .events(
                    Some(since),
                    Some(50),
                    Some("Fetch.requestPaused"),
                    Some(500),
                    false,
                )
                .await;
            since = events.next_seq;
            for event in events.events {
                let request_id = event
                    .params
                    .get("requestId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if request_id.is_empty() {
                    continue;
                }
                let url = event
                    .params
                    .pointer("/request/url")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let method = event
                    .params
                    .pointer("/request/method")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let resource = event
                    .params
                    .get("resourceType")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let rule = rules.iter().find(|rule| {
                    let url_ok = rule
                        .get("url_regex")
                        .and_then(Value::as_str)
                        .is_some_and(|pattern| pattern_matches(pattern, url));
                    let method_ok = rule
                        .get("method")
                        .and_then(Value::as_str)
                        .is_none_or(|wanted| wanted.eq_ignore_ascii_case(method));
                    let type_ok = rule
                        .get("resource_type")
                        .and_then(Value::as_str)
                        .is_none_or(|wanted| wanted.eq_ignore_ascii_case(resource));
                    url_ok && method_ok && type_ok
                });
                let action = rule
                    .and_then(|rule| rule.get("action"))
                    .and_then(Value::as_str)
                    .unwrap_or("continue");
                let session = event.session_id.as_deref();
                let _ = match action {
                    "block" => {
                        self.send(
                            "Fetch.failRequest",
                            json!({ "requestId": request_id, "errorReason": "BlockedByClient" }),
                            session,
                            None,
                            Some(10_000),
                        )
                        .await
                    }
                    "mock" => {
                        let status = rule
                            .and_then(|rule| rule.get("status"))
                            .and_then(Value::as_u64)
                            .unwrap_or(200);
                        let mut params = Map::new();
                        params.insert("requestId".into(), json!(request_id));
                        params.insert("responseCode".into(), json!(status));
                        if let Some(headers) = rule
                            .and_then(|rule| rule.get("headers"))
                            .and_then(Value::as_object)
                        {
                            let headers: Vec<Value> = headers
                                .iter()
                                .map(|(name, value)| {
                                    json!({ "name": name, "value": value.as_str().unwrap_or_default() })
                                })
                                .collect();
                            params.insert("responseHeaders".into(), json!(headers));
                        }
                        if let Some(body) = rule
                            .and_then(|rule| rule.get("body_base64"))
                            .and_then(Value::as_str)
                        {
                            if let Ok(bytes) =
                                base64::engine::general_purpose::STANDARD.decode(body)
                            {
                                params.insert(
                                    "body".into(),
                                    json!(base64::engine::general_purpose::STANDARD.encode(bytes)),
                                );
                            }
                        }
                        self.send(
                            "Fetch.fulfillRequest",
                            Value::Object(params),
                            session,
                            None,
                            Some(10_000),
                        )
                        .await
                    }
                    _ => {
                        self.send(
                            "Fetch.continueRequest",
                            json!({ "requestId": request_id }),
                            session,
                            None,
                            Some(10_000),
                        )
                        .await
                    }
                };
            }
        }
    }

    async fn download_list_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let limit = integer(args, "limit", 50, 1, 200) as usize;
        let mut downloads = self.downloads().await;
        downloads.truncate(limit);
        Ok(json!({ "downloads": downloads }))
    }

    async fn download_wait_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let wanted = optional_string(args, "id");
        let timeout = integer(args, "timeout_ms", 30_000, 100, 120_000);
        let deadline = Instant::now() + Duration::from_millis(timeout);
        loop {
            let downloads = self.downloads().await;
            let selected = match &wanted {
                Some(id) => downloads
                    .iter()
                    .find(|entry| entry.get("id").and_then(Value::as_str) == Some(id.as_str()))
                    .cloned(),
                None => downloads.first().cloned(),
            };
            if let Some(entry) = selected {
                let state = entry.get("state").and_then(Value::as_str).unwrap_or("");
                if state == "complete" || state == "cancelled" {
                    return Ok(entry);
                }
            }
            if Instant::now() >= deadline {
                return Err(CdpError::Timeout(timeout));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn downloads(&self) -> Vec<Value> {
        let events = self.events(Some(0), Some(500), None, None, false).await;
        let mut order: Vec<String> = Vec::new();
        let mut entries: HashMap<String, Value> = HashMap::new();
        for event in events.events {
            let Some(guid) = event.params.get("guid").and_then(Value::as_str) else {
                continue;
            };
            match event.method.as_str() {
                "Browser.downloadWillBegin" => {
                    if !entries.contains_key(guid) {
                        order.push(guid.to_string());
                        entries.insert(
                            guid.to_string(),
                            json!({
                                "id": guid,
                                "filename": event.params.get("suggestedFilename").cloned().unwrap_or(Value::Null),
                                "url": event.params.get("url").cloned().unwrap_or(Value::Null),
                                "state": "inProgress",
                                "bytes_received": 0,
                                "total_bytes": 0,
                                "danger": "safe",
                                "exists": false,
                                "started_at": event.ts,
                                "tab_id": self.target_for_session(event.session_id.as_deref()).unwrap_or_else(|| "active".into()),
                            }),
                        );
                    }
                }
                "Browser.downloadProgress" => {
                    let entry = entries.entry(guid.to_string()).or_insert_with(|| {
                        order.push(guid.to_string());
                        json!({
                            "id": guid,
                            "filename": Value::Null,
                            "url": Value::Null,
                            "state": "inProgress",
                            "bytes_received": 0,
                            "total_bytes": 0,
                            "danger": "safe",
                            "exists": false,
                            "started_at": event.ts,
                            "tab_id": self.target_for_session(event.session_id.as_deref()).unwrap_or_else(|| "active".into()),
                        })
                    });
                    let state = event
                        .params
                        .get("state")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    entry["state"] = json!(match state {
                        "completed" => "complete",
                        "canceled" => "cancelled",
                        other => other,
                    });
                    entry["bytes_received"] = event
                        .params
                        .get("receivedBytes")
                        .cloned()
                        .unwrap_or(json!(0));
                    entry["total_bytes"] =
                        event.params.get("totalBytes").cloned().unwrap_or(json!(0));
                    entry["exists"] = json!(state == "completed");
                }
                _ => {}
            }
        }
        order
            .into_iter()
            .filter_map(|id| entries.remove(&id))
            .collect()
    }

    async fn upload_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let target = required_string(args, "target")?;
        if target.starts_with('@') {
            return Err(CdpError::Unsupported(
                "@element references require the extension transport".into(),
            ));
        }
        let files: Vec<String> = args
            .get("files")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        if files.is_empty() {
            return Err(CdpError::InvalidArgument("files must not be empty".into()));
        }
        let (session, target_id) = self.page(args, 15_000).await?;
        let document = self
            .send(
                "DOM.getDocument",
                json!({ "depth": -1, "pierce": true }),
                Some(&session),
                None,
                Some(15_000),
            )
            .await?;
        let root = document
            .pointer("/root/nodeId")
            .and_then(Value::as_u64)
            .ok_or_else(|| CdpError::Protocol("DOM.getDocument returned no root".into()))?;
        let query = self
            .send(
                "DOM.querySelector",
                json!({ "nodeId": root, "selector": target }),
                Some(&session),
                None,
                Some(15_000),
            )
            .await?;
        let node = query.get("nodeId").and_then(Value::as_u64).unwrap_or(0);
        if node == 0 {
            return Err(CdpError::TargetNotFound(target));
        }
        let attributes = self
            .send(
                "DOM.getAttributes",
                json!({ "nodeId": node }),
                Some(&session),
                None,
                Some(10_000),
            )
            .await
            .ok()
            .and_then(|value| value.get("attributes").cloned())
            .unwrap_or_else(|| json!([]));
        self.send(
            "DOM.setFileInputFiles",
            json!({ "files": files, "nodeId": node }),
            Some(&session),
            None,
            Some(15_000),
        )
        .await?;
        let attributes = attributes.as_array().cloned().unwrap_or_default();
        let mut accept = Value::Null;
        let mut multiple = false;
        for pair in attributes.chunks(2) {
            if let [name, value] = pair {
                match name.as_str() {
                    Some("accept") => accept = value.clone(),
                    Some("multiple") => multiple = true,
                    _ => {}
                }
            }
        }
        Ok(json!({
            "uploaded": true,
            "files": files,
            "target": target,
            "input": { "accept": accept, "multiple": multiple },
            "tab_id": target_id,
        }))
    }

    async fn console_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let (session, target_id) = self.page(args, 15_000).await?;
        self.send(
            "Runtime.enable",
            json!({}),
            Some(&session),
            None,
            Some(10_000),
        )
        .await?;
        tokio::time::sleep(Duration::from_millis(150)).await;
        let limit = integer(args, "limit", 100, 1, 500) as usize;
        let events = self.events(Some(0), Some(500), None, None, false).await;
        let mut entries = Vec::new();
        for event in events.events {
            if event.method == "Runtime.consoleAPICalled" {
                let values: Vec<Value> = event
                    .params
                    .get("args")
                    .and_then(Value::as_array)
                    .map(|values| {
                        values
                            .iter()
                            .map(|value| {
                                value
                                    .get("value")
                                    .cloned()
                                    .or_else(|| value.get("description").cloned())
                                    .unwrap_or(Value::Null)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                entries.push(json!({
                    "type": event.params.get("type").cloned().unwrap_or_else(|| json!("log")),
                    "ts": event.ts,
                    "tab_id": target_id,
                    "args": values,
                }));
            } else if event.method == "Runtime.exceptionThrown" {
                entries.push(json!({
                    "type": "exception",
                    "ts": event.ts,
                    "tab_id": target_id,
                    "text": event.params.pointer("/exceptionDetails/text").cloned().unwrap_or(Value::Null),
                }));
            }
        }
        let start = entries.len().saturating_sub(limit);
        Ok(json!({ "entries": entries[start..].to_vec() }))
    }

    async fn cookies_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let urls: Vec<String> = args
            .get("urls")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let result = if urls.is_empty() {
            self.send("Storage.getCookies", json!({}), None, None, Some(10_000))
                .await?
        } else {
            let (session, _) = self.page(args, 15_000).await?;
            self.send(
                "Network.getCookies",
                json!({ "urls": urls }),
                Some(&session),
                None,
                Some(10_000),
            )
            .await?
        };
        Ok(json!({ "cookies": result.get("cookies").cloned().unwrap_or_else(|| json!([])) }))
    }

    async fn dialog_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let accept = args.get("accept").and_then(Value::as_bool).unwrap_or(true);
        let (session, _) = self.page(args, 15_000).await?;
        let mut params = Map::new();
        params.insert("accept".into(), json!(accept));
        if let Some(text) = optional_string(args, "prompt_text") {
            params.insert("promptText".into(), json!(text));
        }
        self.send(
            "Page.handleJavaScriptDialog",
            Value::Object(params),
            Some(&session),
            None,
            Some(10_000),
        )
        .await
        .map_err(dialog_error)?;
        Ok(json!({ "handled": true, "accepted": accept }))
    }

    async fn emulate_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let (session, _) = self.page(args, 15_000).await?;
        let mut applied: Vec<&str> = Vec::new();
        if args.get("reset").and_then(Value::as_bool).unwrap_or(false) {
            for method in [
                "Emulation.clearDeviceMetricsOverride",
                "Emulation.clearGeolocationOverride",
                "Network.emulateNetworkConditions",
            ] {
                let params = if method == "Network.emulateNetworkConditions" {
                    json!({
                        "offline": false,
                        "latency": 0,
                        "downloadThroughput": -1,
                        "uploadThroughput": -1,
                    })
                } else {
                    json!({})
                };
                let _ = self
                    .send(method, params, Some(&session), None, Some(10_000))
                    .await;
            }
            return Ok(json!({ "applied": ["reset"] }));
        }
        if args.get("width").is_some() || args.get("height").is_some() {
            let width = required_integer(args, "width", 1, 100_000)?;
            let height = required_integer(args, "height", 1, 100_000)?;
            let scale = args
                .get("device_scale_factor")
                .and_then(Value::as_f64)
                .unwrap_or(1.0);
            let mobile = args.get("mobile").and_then(Value::as_bool).unwrap_or(false);
            self.send(
                "Emulation.setDeviceMetricsOverride",
                json!({
                    "width": width,
                    "height": height,
                    "deviceScaleFactor": scale,
                    "mobile": mobile,
                }),
                Some(&session),
                None,
                Some(10_000),
            )
            .await?;
            applied.push("device_metrics");
        }
        if args.get("offline").is_some()
            || args.get("latency_ms").is_some()
            || args.get("download_throughput").is_some()
            || args.get("upload_throughput").is_some()
        {
            self.send(
                "Network.emulateNetworkConditions",
                json!({
                    "offline": args.get("offline").and_then(Value::as_bool).unwrap_or(false),
                    "latency": args.get("latency_ms").and_then(Value::as_u64).unwrap_or(0),
                    "downloadThroughput": args.get("download_throughput").and_then(Value::as_f64).unwrap_or(-1.0),
                    "uploadThroughput": args.get("upload_throughput").and_then(Value::as_f64).unwrap_or(-1.0),
                }),
                Some(&session),
                None,
                Some(10_000),
            )
            .await?;
            applied.push("network");
        }
        if let Some(user_agent) = optional_string(args, "user_agent") {
            self.send(
                "Emulation.setUserAgentOverride",
                json!({ "userAgent": user_agent }),
                Some(&session),
                None,
                Some(10_000),
            )
            .await?;
            applied.push("user_agent");
        }
        if let Some(timezone) = optional_string(args, "timezone") {
            self.send(
                "Emulation.setTimezoneOverride",
                json!({ "timezoneId": timezone }),
                Some(&session),
                None,
                Some(10_000),
            )
            .await?;
            applied.push("timezone");
        }
        Ok(json!({ "applied": applied }))
    }

    async fn snapshot_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let (session, target_id) = self.page(args, 15_000).await?;
        let mode = Mode::parse(args.get("mode").and_then(Value::as_str))?;
        let format = Format::parse(args.get("format").and_then(Value::as_str))?;
        let options = Options {
            mode,
            format,
            max_nodes: integer(
                args,
                "max_nodes",
                SnapshotPolicy::DEFAULT_MAX_NODES as u64,
                SnapshotPolicy::MIN_MAX_NODES as u64,
                SnapshotPolicy::MAX_MAX_NODES as u64,
            ) as usize,
            viewport_only: args
                .get("viewport_only")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            include_bbox: args
                .get("include_bbox")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        };
        let (dom, page) = self.snapshot_page(&session).await?;
        let frame_tree = self
            .send(
                "Page.getFrameTree",
                json!({}),
                Some(&session),
                None,
                Some(15_000),
            )
            .await
            .ok();
        let frame_nodes = frame_tree
            .as_ref()
            .map(flatten_frame_tree)
            .unwrap_or_default();
        let mut frames_json = None;
        let snapshot = if frame_nodes.len() <= 1 {
            let ax = self
                .send(
                    "Accessibility.getFullAXTree",
                    json!({}),
                    Some(&session),
                    None,
                    Some(15_000),
                )
                .await?;
            snapshot::render(&ax, &dom, &page, &options)?
        } else {
            let strings: Vec<Value> = dom
                .pointer("/strings")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let documents: Vec<Value> = dom
                .pointer("/documents")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let _ = self
                .send(
                    "DOM.getDocument",
                    json!({ "depth": 0 }),
                    Some(&session),
                    None,
                    Some(10_000),
                )
                .await;
            let mut collected: Vec<FrameData> = Vec::new();
            let mut skipped: Vec<Value> = Vec::new();
            for node in &frame_nodes {
                let is_top = node.path.is_empty();
                if !is_top && !same_origin(&page.url, &node.url) {
                    skipped.push(json!({
                        "frame": node.path,
                        "url": node.url,
                        "reason": "cross_origin",
                    }));
                    continue;
                }
                let document_index = snapshot_document_index(&documents, &strings, &node.frame_id);
                let (origin, scroll) = if is_top {
                    let scroll = document_index
                        .map(|index| document_scroll(&documents[index]))
                        .unwrap_or((page.viewport.scroll_x, page.viewport.scroll_y));
                    ((0.0, 0.0), scroll)
                } else {
                    let Some(document_index) = document_index else {
                        skipped.push(json!({
                            "frame": node.path,
                            "url": node.url,
                            "reason": "geometry_unavailable",
                        }));
                        continue;
                    };
                    match self.frame_owner_origin(&session, &node.frame_id).await {
                        Ok(Some(origin)) => (origin, document_scroll(&documents[document_index])),
                        _ => {
                            skipped.push(json!({
                                "frame": node.path,
                                "url": node.url,
                                "reason": "geometry_unavailable",
                            }));
                            continue;
                        }
                    }
                };
                let ax = match self
                    .send(
                        "Accessibility.getFullAXTree",
                        json!({ "frameId": node.frame_id }),
                        Some(&session),
                        None,
                        Some(15_000),
                    )
                    .await
                {
                    Ok(ax) => ax,
                    Err(error) if is_top => return Err(error),
                    Err(_) => {
                        skipped.push(json!({
                            "frame": node.path,
                            "url": node.url,
                            "reason": "unavailable",
                        }));
                        continue;
                    }
                };
                collected.push(FrameData {
                    path: node.path.clone(),
                    ax,
                    document_index,
                    origin,
                    scroll,
                });
            }
            frames_json = frames_summary(&frame_nodes, &collected, skipped);
            let inputs: Vec<snapshot::FrameInput<'_>> = collected
                .iter()
                .map(|frame| snapshot::FrameInput {
                    path: &frame.path,
                    ax: &frame.ax,
                    document: frame.document_index.and_then(|index| documents.get(index)),
                    strings: Some(strings.as_slice()),
                    origin: frame.origin,
                    scroll: frame.scroll,
                })
                .collect();
            snapshot::render_frames(&inputs, &page, &options)?
        };
        let viewport = page.viewport.as_json();
        let mut result = match format {
            Format::Compact => json!({
                "format": "compact",
                "tab_id": target_id,
                "url": page.url,
                "title": page.title,
                "viewport": viewport,
                "mode": mode.as_str(),
                "node_count": snapshot.node_count,
                "truncated": snapshot.truncated,
                "text": snapshot.text,
            }),
            Format::Json => json!({
                "format": "json",
                "tab_id": target_id,
                "url": page.url,
                "title": page.title,
                "viewport": viewport,
                "mode": mode.as_str(),
                "node_count": snapshot.node_count,
                "truncated": snapshot.truncated,
                "nodes": snapshot.nodes,
            }),
        };
        if let Some(frames) = frames_json {
            result["frames"] = frames;
        }
        Ok(result)
    }

    async fn find_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let query = required_string(args, "query")?;
        let role = optional_string(args, "role");
        let regex = args.get("regex").and_then(Value::as_bool).unwrap_or(false);
        let limit = integer(args, "limit", 20, 1, 100) as usize;
        let (session, _) = self.page(args, 15_000).await?;
        let (ax, dom, page) = self.snapshot_inputs(&session).await?;
        let (matches, _truncated) =
            snapshot::find(&ax, &dom, &query, role.as_deref(), regex, limit)?;
        let matches: Vec<Value> = matches
            .into_iter()
            .map(|found| {
                json!({
                    "ref": found.reference,
                    "tag": found.tag,
                    "role": found.role,
                    "text": found.text,
                })
            })
            .collect();
        Ok(json!({
            "url": page.url,
            "count": matches.len(),
            "matches": matches,
        }))
    }

    async fn screenshot_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let (session, target_id) = self.page(args, 15_000).await?;
        let target = args
            .get("target")
            .and_then(Value::as_str)
            .unwrap_or("viewport");
        let format = args.get("format").and_then(Value::as_str).unwrap_or("webp");
        let mime_type = match format {
            "webp" => "image/webp",
            "png" => "image/png",
            "jpeg" => "image/jpeg",
            other => {
                return Err(CdpError::InvalidArgument(format!(
                    "browser_screenshot format must be webp, png or jpeg, got {other}"
                )))
            }
        };
        let quality = integer(args, "quality", 80, 1, 100);
        let inline = args.get("inline").and_then(Value::as_bool).unwrap_or(false);
        let url = self
            .eval_value(&session, "location.href", false)
            .await
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_default();
        let mut params = json!({
            "format": format,
            "fromSurface": true,
        });
        if format != "png" {
            params["quality"] = json!(quality);
        }
        let bbox = match target {
            "viewport" => {
                params["captureBeyondViewport"] = json!(false);
                None
            }
            "full_page" => {
                let metrics = self
                    .send(
                        "Page.getLayoutMetrics",
                        json!({}),
                        Some(&session),
                        None,
                        Some(10_000),
                    )
                    .await?;
                let size = metrics
                    .get("cssContentSize")
                    .or_else(|| metrics.get("contentSize"))
                    .ok_or_else(|| CdpError::Protocol("page content size is unavailable".into()))?;
                let width = json_f64(size.get("width"));
                let height = json_f64(size.get("height"));
                if width <= 0.0 || height <= 0.0 {
                    return Err(CdpError::Protocol(
                        "page content size is not positive".into(),
                    ));
                }
                params["clip"] = json!({
                    "x": 0.0,
                    "y": 0.0,
                    "width": width,
                    "height": height,
                    "scale": 1.0,
                });
                params["captureBeyondViewport"] = json!(true);
                None
            }
            "element" => {
                let reference = required_string(args, "ref")?;
                let clip = self.screenshot_element_clip(&session, &reference).await?;
                params["clip"] = json!({
                    "x": clip.0,
                    "y": clip.1,
                    "width": clip.2,
                    "height": clip.3,
                    "scale": 1.0,
                });
                params["captureBeyondViewport"] = json!(true);
                Some(json!({
                    "x": clip.0,
                    "y": clip.1,
                    "width": clip.2,
                    "height": clip.3,
                }))
            }
            other => {
                return Err(CdpError::InvalidArgument(format!(
                    "browser_screenshot target must be viewport, full_page or element, got {other}"
                )))
            }
        };
        let response = self
            .send(
                "Page.captureScreenshot",
                params,
                Some(&session),
                None,
                Some(15_000),
            )
            .await?;
        let data = response
            .get("data")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                CdpError::Protocol("Page.captureScreenshot returned no image data".into())
            })?
            .to_string();
        let mut result = json!({
            "format": format,
            "mime_type": mime_type,
            "target": target,
            "tab_id": target_id,
            "url": url,
            "inline": inline,
            "data": data,
        });
        if let Some(bbox) = bbox {
            result["bbox"] = bbox;
        }
        Ok(result)
    }

    async fn screenshot_element_clip(
        &self,
        session: &str,
        reference: &str,
    ) -> Result<(f64, f64, f64, f64), CdpError> {
        let backend_id = screenshot_backend_id(reference)?;
        let response = match self
            .send(
                "DOM.getBoxModel",
                json!({ "backendNodeId": backend_id }),
                Some(session),
                None,
                Some(10_000),
            )
            .await
        {
            Ok(response) => response,
            Err(CdpError::Remote { .. }) => {
                return Err(CdpError::TargetNotFound(reference.to_string()))
            }
            Err(error) => return Err(error),
        };
        let model = response.get("model").cloned().unwrap_or(Value::Null);
        let scroll = self
            .eval_value(session, "({ x: window.scrollX, y: window.scrollY })", false)
            .await?;
        Self::element_clip_from_box_model(
            &model,
            reference,
            json_f64(scroll.get("x")),
            json_f64(scroll.get("y")),
        )
    }

    fn element_clip_from_box_model(
        model: &Value,
        reference: &str,
        scroll_x: f64,
        scroll_y: f64,
    ) -> Result<(f64, f64, f64, f64), CdpError> {
        let quad = model
            .get("border")
            .and_then(Value::as_array)
            .ok_or_else(|| CdpError::TargetNotFound(reference.to_string()))?;
        if quad.len() < 8 {
            return Err(CdpError::TargetNotFound(reference.to_string()));
        }
        let mut min_x = f64::MAX;
        let mut min_y = f64::MAX;
        let mut max_x = f64::MIN;
        let mut max_y = f64::MIN;
        for pair in quad.as_chunks::<2>().0 {
            let [x, y] = pair;
            let x = x
                .as_f64()
                .ok_or_else(|| CdpError::TargetNotFound(reference.to_string()))?;
            let y = y
                .as_f64()
                .ok_or_else(|| CdpError::TargetNotFound(reference.to_string()))?;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
        let width = max_x - min_x;
        let height = max_y - min_y;
        if width <= 0.0 || height <= 0.0 {
            return Err(CdpError::TargetNotFound(reference.to_string()));
        }
        Ok((min_x + scroll_x, min_y + scroll_y, width, height))
    }

    async fn snapshot_inputs(&self, session: &str) -> Result<(Value, Value, PageInfo), CdpError> {
        let (dom, page) = self.snapshot_page(session).await?;
        let ax = self
            .send(
                "Accessibility.getFullAXTree",
                json!({}),
                Some(session),
                None,
                Some(15_000),
            )
            .await?;
        Ok((ax, dom, page))
    }

    async fn snapshot_page(&self, session: &str) -> Result<(Value, PageInfo), CdpError> {
        let dom = self
            .send(
                "DOMSnapshot.captureSnapshot",
                json!({
                    "computedStyles": [],
                    "includePaintOrder": false,
                    "includeDOMRects": true,
                }),
                Some(session),
                None,
                Some(15_000),
            )
            .await?;
        let meta = self
            .eval_value(
                session,
                "(() => ({ url: location.href, title: document.title, width: window.innerWidth, height: window.innerHeight, scrollX: window.scrollX, scrollY: window.scrollY }))()",
                false,
            )
            .await?;
        let viewport = Viewport {
            width: json_i64(meta.get("width")),
            height: json_i64(meta.get("height")),
            scroll_x: json_f64(meta.get("scrollX")),
            scroll_y: json_f64(meta.get("scrollY")),
        };
        let page = PageInfo {
            url: meta
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            title: meta
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            viewport,
        };
        Ok((dom, page))
    }

    async fn frame_owner_origin(
        &self,
        session: &str,
        frame_id: &str,
    ) -> Result<Option<(f64, f64)>, CdpError> {
        let owner = self
            .send(
                "DOM.getFrameOwner",
                json!({ "frameId": frame_id }),
                Some(session),
                None,
                Some(10_000),
            )
            .await?;
        let backend_id = owner
            .get("backendNodeId")
            .and_then(Value::as_i64)
            .or_else(|| owner.pointer("/node/backendNodeId").and_then(Value::as_i64));
        let Some(backend_id) = backend_id else {
            return Ok(None);
        };
        let model = self
            .send(
                "DOM.getBoxModel",
                json!({ "backendNodeId": backend_id }),
                Some(session),
                None,
                Some(10_000),
            )
            .await?;
        let content = model.pointer("/model/content").and_then(Value::as_array);
        let x = content
            .and_then(|content| content.first())
            .and_then(Value::as_f64);
        let y = content
            .and_then(|content| content.get(1))
            .and_then(Value::as_f64);
        match (x, y) {
            (Some(x), Some(y)) if x.is_finite() && y.is_finite() => Ok(Some((x, y))),
            _ => Ok(None),
        }
    }

    async fn click_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let target = required_string(args, "target")?;
        let (session, _) = self.page(args, 15_000).await?;
        let (object_id, info) = self.resolve_element(&session, &target).await?;
        ensure_interactable(&info)?;
        self.scroll_into_view(&session, &object_id).await?;
        let info = self.element_info(&session, &object_id).await?;
        ensure_interactable(&info)?;
        let point = self.box_center(&session, &object_id).await?;
        if !self.hit_test(&session, &object_id, point).await? {
            return Err(CdpError::NotInteractable(
                "element is covered by another element".into(),
            ));
        }
        for (event, buttons) in [("mouseMoved", 0), ("mousePressed", 1), ("mouseReleased", 0)] {
            self.send(
                "Input.dispatchMouseEvent",
                json!({
                    "type": event,
                    "x": point.0,
                    "y": point.1,
                    "button": "left",
                    "buttons": buttons,
                    "clickCount": 1,
                }),
                Some(&session),
                None,
                Some(10_000),
            )
            .await?;
        }
        let url = self.current_url(&session).await?;
        Ok(json!({
            "clicked": true,
            "ref": target,
            "selector": target,
            "tag": info.tag,
            "role": info.role,
            "name": info.name,
            "bbox": info.rect.as_json(),
            "point": { "x": point.0, "y": point.1 },
            "events": ["mousemove", "mousedown", "mouseup", "click"],
            "url": url,
        }))
    }

    async fn hover_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let target = required_string(args, "target")?;
        let (session, _) = self.page(args, 15_000).await?;
        let (object_id, info) = self.resolve_element(&session, &target).await?;
        ensure_interactable(&info)?;
        self.scroll_into_view(&session, &object_id).await?;
        let info = self.element_info(&session, &object_id).await?;
        ensure_interactable(&info)?;
        let point = self.box_center(&session, &object_id).await?;
        self.send(
            "Input.dispatchMouseEvent",
            json!({
                "type": "mouseMoved",
                "x": point.0,
                "y": point.1,
                "button": "none",
                "buttons": 0,
            }),
            Some(&session),
            None,
            Some(10_000),
        )
        .await?;
        let url = self.current_url(&session).await?;
        Ok(json!({
            "hovered": true,
            "ref": target,
            "url": url,
            "target": { "ref": target, "tag": info.tag, "role": info.role, "text": info.name },
        }))
    }

    async fn type_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let target = required_string(args, "target")?;
        let text = required_string_allow_empty(args, "text")?;
        let clear = args.get("clear").and_then(Value::as_bool).unwrap_or(true);
        let blur = args.get("blur").and_then(Value::as_bool).unwrap_or(false);
        let (session, _) = self.page(args, 15_000).await?;
        let (object_id, info) = self.resolve_element(&session, &target).await?;
        ensure_interactable(&info)?;
        self.scroll_into_view(&session, &object_id).await?;
        self.send(
            "DOM.focus",
            json!({ "objectId": object_id }),
            Some(&session),
            None,
            Some(10_000),
        )
        .await?;
        let prepare = if clear {
            "function() { this.focus(); if (this.select) { this.select(); } else if (this.isContentEditable) { const range = document.createRange(); range.selectNodeContents(this); const selection = getSelection(); selection.removeAllRanges(); selection.addRange(range); } }"
        } else {
            "function() { this.focus(); if (this.setSelectionRange && typeof this.value === 'string') { this.setSelectionRange(this.value.length, this.value.length); } }"
        };
        self.call_on(&session, &object_id, prepare, json!([]), false)
            .await?;
        self.send(
            "Input.insertText",
            json!({ "text": text }),
            Some(&session),
            None,
            Some(15_000),
        )
        .await?;
        if blur {
            self.call_on(
                &session,
                &object_id,
                "function() { this.blur(); }",
                json!([]),
                false,
            )
            .await?;
        }
        let url = self.current_url(&session).await?;
        let events = if blur {
            json!(["focus", "input", "change", "blur"])
        } else {
            json!(["focus", "input"])
        };
        Ok(json!({
            "typed": true,
            "ref": target,
            "selector": target,
            "tag": info.tag,
            "role": info.role,
            "cleared": clear,
            "events": events,
            "value_length": text.chars().count(),
            "url": url,
        }))
    }

    async fn press_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let raw_keys = match args.get("keys") {
            Some(Value::String(key)) if !key.is_empty() => vec![key.clone()],
            Some(Value::Array(keys)) if !keys.is_empty() => {
                let mut parsed = Vec::with_capacity(keys.len());
                for key in keys {
                    let key = key.as_str().filter(|key| !key.is_empty()).ok_or_else(|| {
                        CdpError::InvalidArgument("keys must contain non-empty strings".into())
                    })?;
                    parsed.push(key.to_string());
                }
                parsed
            }
            _ => {
                return Err(CdpError::InvalidArgument(
                    "keys must be a non-empty string or array of strings".into(),
                ))
            }
        };
        let (session, _) = self.page(args, 15_000).await?;
        if let Some(target) = optional_string(args, "target") {
            let (object_id, info) = self.resolve_element(&session, &target).await?;
            ensure_interactable(&info)?;
            self.scroll_into_view(&session, &object_id).await?;
            self.send(
                "DOM.focus",
                json!({ "objectId": object_id }),
                Some(&session),
                None,
                Some(10_000),
            )
            .await?;
        }
        for raw in &raw_keys {
            let (modifiers, stroke) = parse_key(raw)?;
            let mut params = json!({
                "type": "keyDown",
                "modifiers": modifiers,
                "key": stroke.key,
                "code": stroke.code,
                "windowsVirtualKeyCode": stroke.key_code,
                "nativeVirtualKeyCode": stroke.key_code,
            });
            if let Some(text) = &stroke.text {
                params["text"] = json!(text);
                params["unmodifiedText"] = json!(text);
            }
            self.send(
                "Input.dispatchKeyEvent",
                params,
                Some(&session),
                None,
                Some(10_000),
            )
            .await?;
            self.send(
                "Input.dispatchKeyEvent",
                json!({
                    "type": "keyUp",
                    "modifiers": modifiers,
                    "key": stroke.key,
                    "code": stroke.code,
                    "windowsVirtualKeyCode": stroke.key_code,
                    "nativeVirtualKeyCode": stroke.key_code,
                }),
                Some(&session),
                None,
                Some(10_000),
            )
            .await?;
        }
        let url = self.current_url(&session).await?;
        Ok(json!({ "pressed": raw_keys, "keys": raw_keys, "url": url }))
    }

    async fn scroll_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let direction = required_string(args, "direction")?;
        let (dx, dy) = match direction.as_str() {
            "up" => (0.0, -1.0),
            "down" => (0.0, 1.0),
            "left" => (-1.0, 0.0),
            "right" => (1.0, 0.0),
            other => {
                return Err(CdpError::InvalidArgument(format!(
                    "direction must be up, down, left or right, got {other}"
                )))
            }
        };
        let pages = args
            .get("pages")
            .and_then(Value::as_f64)
            .unwrap_or(1.0)
            .clamp(0.0, 50.0);
        let (session, target_id) = self.page(args, 15_000).await?;
        let (x, y, url) = if let Some(target) = optional_string(args, "target") {
            let (object_id, info) = self.resolve_element(&session, &target).await?;
            ensure_interactable(&info)?;
            let result = self
                .call_on(
                    &session,
                    &object_id,
                    "function(dx, dy) { this.scrollBy(dx * this.clientWidth, dy * this.clientHeight); return { x: this.scrollLeft, y: this.scrollTop }; }",
                    json!([{ "value": dx * pages }, { "value": dy * pages }]),
                    true,
                )
                .await?;
            let url = self.current_url(&session).await?;
            (json_i64(result.get("x")), json_i64(result.get("y")), url)
        } else {
            let expression = format!(
                "(() => {{ window.scrollBy({} * window.innerWidth, {} * window.innerHeight); return {{ x: window.scrollX, y: window.scrollY, url: location.href }}; }})()",
                dx * pages,
                dy * pages
            );
            let result = self.eval_value(&session, &expression, false).await?;
            (
                json_i64(result.get("x")),
                json_i64(result.get("y")),
                result.get("url").cloned().unwrap_or(Value::Null),
            )
        };
        Ok(json!({
            "scrolled": true,
            "x": x,
            "y": y,
            "url": url,
            "tab_id": target_id,
        }))
    }

    async fn select_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let target = required_string(args, "target")?;
        let values = match args.get("values") {
            Some(Value::Array(values)) if !values.is_empty() => values
                .iter()
                .map(|value| {
                    value.as_str().map(str::to_string).ok_or_else(|| {
                        CdpError::InvalidArgument("values must be an array of strings".into())
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
            _ => {
                return Err(CdpError::InvalidArgument(
                    "values must be a non-empty array of strings".into(),
                ))
            }
        };
        let (session, _) = self.page(args, 15_000).await?;
        let (object_id, info) = self.resolve_element(&session, &target).await?;
        ensure_interactable(&info)?;
        self.scroll_into_view(&session, &object_id).await?;
        let arguments: Vec<Value> = values
            .iter()
            .map(|value| json!({ "value": value }))
            .collect();
        let result = self
            .call_on(
                &session,
                &object_id,
                "function(...values) { if (!(this instanceof HTMLSelectElement)) { return { ok: false, code: 'constraint', reason: 'element is not a select' }; } const wanted = values.map(String); const options = Array.from(this.options); const missing = wanted.filter(value => !options.some(option => option.value === value)); if (missing.length > 0) { return { ok: false, code: 'missing', reason: missing.join(', ') }; } if (!this.multiple && wanted.length !== 1) { return { ok: false, code: 'constraint', reason: 'multiple values require a multiple select' }; } for (const option of options) { option.selected = wanted.includes(option.value); } this.dispatchEvent(new Event('input', { bubbles: true })); this.dispatchEvent(new Event('change', { bubbles: true })); return { ok: true, selected: this.multiple ? Array.from(this.selectedOptions).map(option => option.value) : this.value }; }",
                Value::Array(arguments),
                true,
            )
            .await?;
        if result.get("ok").and_then(Value::as_bool) != Some(true) {
            let reason = result
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("selection failed");
            if result.get("code").and_then(Value::as_str) == Some("missing") {
                return Err(CdpError::TargetNotFound(format!("option {reason}")));
            }
            return Err(CdpError::InvalidArgument(reason.to_string()));
        }
        let selected = result.get("selected").cloned().unwrap_or_else(|| json!([]));
        let url = self.current_url(&session).await?;
        Ok(json!({ "selected": selected, "value": selected, "url": url }))
    }

    async fn resolve_element(
        &self,
        session: &str,
        target: &str,
    ) -> Result<(String, ElementInfo), CdpError> {
        let object_id = self.element_object(session, target).await?;
        let info = self.element_info(session, &object_id).await?;
        Ok((object_id, info))
    }

    async fn element_object(&self, session: &str, target: &str) -> Result<String, CdpError> {
        if let Some(backend) = target.strip_prefix("@c") {
            let backend_id: i64 = backend.parse().map_err(|_| {
                CdpError::InvalidArgument(
                    "CDP element references are @c<backendNodeId>, for example @c42".into(),
                )
            })?;
            let response = self
                .send(
                    "DOM.resolveNode",
                    json!({ "backendNodeId": backend_id }),
                    Some(session),
                    None,
                    Some(10_000),
                )
                .await?;
            return response
                .pointer("/object/objectId")
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| CdpError::TargetNotFound(target.to_string()));
        }
        if target.starts_with('@') {
            return Err(CdpError::InvalidArgument(
                "extension references such as @e12 are not CDP node ids; use @c<backendNodeId> from page_snapshot, or omit tab_id and use the extension channel".into(),
            ));
        }
        let expression = format!("document.querySelector({})", js_string(target));
        let response = self
            .send(
                "Runtime.evaluate",
                json!({ "expression": expression, "returnByValue": false, "awaitPromise": false }),
                Some(session),
                None,
                Some(10_000),
            )
            .await?;
        if let Some(exception) = response.get("exceptionDetails") {
            return Err(CdpError::InvalidArgument(format!(
                "invalid CSS selector {target}: {}",
                exception
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("querySelector failed")
            )));
        }
        response
            .pointer("/result/objectId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| CdpError::TargetNotFound(format!("element selector {target}")))
    }

    async fn element_info(&self, session: &str, object_id: &str) -> Result<ElementInfo, CdpError> {
        let value = self
            .call_on(
                session,
                object_id,
                "function() { const el = this; const tag = (el.tagName || '').toLowerCase(); let role = el.getAttribute ? (el.getAttribute('role') || '') : ''; if (!role) { if (tag === 'a' && el.hasAttribute && el.hasAttribute('href')) { role = 'link'; } else if (tag === 'button') { role = 'button'; } else if (tag === 'select') { role = 'combobox'; } else if (tag === 'textarea') { role = 'textbox'; } else if (tag === 'input') { const type = (el.type || 'text').toLowerCase(); role = type === 'checkbox' ? 'checkbox' : type === 'radio' ? 'radio' : type === 'submit' || type === 'button' ? 'button' : 'textbox'; } } const text = (el.getAttribute && (el.getAttribute('aria-label') || el.getAttribute('title')) || el.innerText || el.value || '').trim().slice(0, 120); const rect = el.getBoundingClientRect ? el.getBoundingClientRect() : { x: 0, y: 0, width: 0, height: 0 }; let visible = true; try { if (el.checkVisibility) { visible = el.checkVisibility({ checkOpacity: true, checkVisibilityCSS: true }); } } catch (_) { visible = true; } const style = getComputedStyle(el); if (style.visibility === 'hidden' || style.display === 'none') { visible = false; } return { tag: tag, role: role, name: text, disabled: !!el.disabled || (el.getAttribute && el.getAttribute('aria-disabled') === 'true'), readOnly: !!el.readOnly, visible: visible, type: (el.type || '').toLowerCase(), rect: { x: rect.x, y: rect.y, width: rect.width, height: rect.height } }; }",
                json!([]),
                true,
            )
            .await?;
        let rect = value.get("rect").cloned().unwrap_or_else(|| json!({}));
        Ok(ElementInfo {
            tag: value
                .get("tag")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            role: value
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            name: value
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            disabled: value
                .get("disabled")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            visible: value
                .get("visible")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            rect: ElementBox {
                x: json_f64(rect.get("x")),
                y: json_f64(rect.get("y")),
                width: json_f64(rect.get("width")),
                height: json_f64(rect.get("height")),
            },
        })
    }

    async fn call_on(
        &self,
        session: &str,
        object_id: &str,
        function: &str,
        arguments: Value,
        return_by_value: bool,
    ) -> Result<Value, CdpError> {
        let response = self
            .send(
                "Runtime.callFunctionOn",
                json!({
                    "objectId": object_id,
                    "functionDeclaration": function,
                    "arguments": arguments,
                    "returnByValue": return_by_value,
                    "awaitPromise": true,
                }),
                Some(session),
                None,
                Some(15_000),
            )
            .await?;
        if let Some(exception) = response.get("exceptionDetails") {
            let message = exception
                .pointer("/exception/description")
                .and_then(Value::as_str)
                .or_else(|| exception.get("text").and_then(Value::as_str))
                .unwrap_or("element evaluation failed");
            return Err(CdpError::Protocol(message.to_string()));
        }
        if return_by_value {
            Ok(response
                .pointer("/result/value")
                .cloned()
                .unwrap_or(Value::Null))
        } else {
            Ok(response.get("result").cloned().unwrap_or(Value::Null))
        }
    }

    async fn scroll_into_view(&self, session: &str, object_id: &str) -> Result<(), CdpError> {
        self.send(
            "DOM.scrollIntoViewIfNeeded",
            json!({ "objectId": object_id }),
            Some(session),
            None,
            Some(10_000),
        )
        .await
        .map_err(interaction_error)?;
        Ok(())
    }

    async fn box_center(&self, session: &str, object_id: &str) -> Result<(f64, f64), CdpError> {
        let model = self
            .send(
                "DOM.getBoxModel",
                json!({ "objectId": object_id }),
                Some(session),
                None,
                Some(10_000),
            )
            .await
            .map_err(interaction_error)?;
        let quad = model
            .pointer("/model/content")
            .or_else(|| model.pointer("/model/border"))
            .and_then(Value::as_array)
            .filter(|quad| quad.len() >= 8)
            .ok_or_else(|| CdpError::NotInteractable("element has no layout box".into()))?;
        let numbers: Vec<f64> = quad
            .iter()
            .take(8)
            .map(|value| value.as_f64().unwrap_or(0.0))
            .collect();
        Ok((
            (numbers[0] + numbers[4]) / 2.0,
            (numbers[1] + numbers[5]) / 2.0,
        ))
    }

    async fn hit_test(
        &self,
        session: &str,
        object_id: &str,
        point: (f64, f64),
    ) -> Result<bool, CdpError> {
        let result = self
            .call_on(
                session,
                object_id,
                "function(x, y) { const hit = document.elementFromPoint(x, y); return !!hit && (hit === this || this.contains(hit) || hit.contains(this)); }",
                json!([{ "value": point.0 }, { "value": point.1 }]),
                true,
            )
            .await?;
        Ok(result.as_bool().unwrap_or(false))
    }

    /// Execute one P2 provider action with the frozen argument contract.
    pub async fn worker_action(
        &self,
        action: &str,
        args: &Value,
        timeout_ms: u64,
    ) -> Result<Value, P2ActionError> {
        let timeout = timeout_ms.clamp(1, 120_000);
        match action {
            "tabs.list" => self.p2_tabs_list(args, timeout).await,
            "tabs.open" => self.p2_tabs_open(args, timeout).await,
            "tabs.close" => self.p2_tabs_close(args, timeout).await,
            "page.navigate" => self.p2_page_navigate(args, timeout).await,
            "page.read" => self.p2_page_read(args, timeout).await,
            "page.snapshot" => self.p2_page_snapshot(args, timeout).await,
            "input.click" => self.p2_input_click(args, timeout).await,
            "input.type" => self.p2_input_type(args, timeout).await,
            "capture.viewport" => self.p2_capture_viewport(args, timeout).await,
            "system.info" => self.p2_system_info(args, timeout).await,
            _ => Err(P2ActionError::BadArgs("unknown action".into())),
        }
    }

    async fn p2_tabs_list(&self, args: &Value, timeout: u64) -> Result<Value, P2ActionError> {
        p2_check_args(args, &[])?;
        let active = self.default_target_id();
        let mut rows: Vec<(u64, String, Value)> = Vec::new();
        for target in self.targets() {
            let window_id = self
                .p2_window_id(&target.target_id, timeout.min(5_000))
                .await;
            rows.push((
                window_id,
                target.target_id.clone(),
                json!({
                    "tab_id": target.target_id,
                    "window_id": window_id,
                    "title": target.title,
                    "url": target.url,
                    "active": active.as_deref() == Some(target.target_id.as_str()),
                }),
            ));
        }
        rows.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
        let tabs: Vec<Value> = rows.into_iter().take(200).map(|row| row.2).collect();
        Ok(json!({ "tabs": tabs, "count": tabs.len() }))
    }

    async fn p2_tabs_open(&self, args: &Value, timeout: u64) -> Result<Value, P2ActionError> {
        p2_check_args(args, &["url", "active"])?;
        let url = p2_string(args, "url")?;
        if !p2_valid_url(&url) {
            return Err(P2ActionError::BadArgs(
                "url must be an http, https or about:blank address".into(),
            ));
        }
        let active = p2_bool(args, "active", true)?;
        let response = self
            .send(
                "Target.createTarget",
                json!({ "url": "about:blank", "background": !active }),
                None,
                None,
                Some(timeout),
            )
            .await
            .map_err(p2_error)?;
        let tab_id = response
            .get("targetId")
            .and_then(Value::as_str)
            .ok_or_else(|| P2ActionError::Internal("the browser did not open a tab".into()))?
            .to_string();
        self.remember_page_target(&tab_id, "about:blank");
        if active {
            let _ = self
                .send(
                    "Target.activateTarget",
                    json!({ "targetId": tab_id }),
                    None,
                    None,
                    Some(timeout.min(10_000)),
                )
                .await;
            let _ = self.set_active_target(&tab_id);
        }
        let (session, _) = self
            .page(&p2_page_args(Some(tab_id.as_str())), timeout)
            .await
            .map_err(p2_error)?;
        let loaded_url = if url == "about:blank" {
            "about:blank".to_string()
        } else {
            let page = self
                .p2_navigate_page(&session, &url, "load", timeout)
                .await?;
            page.get("url")
                .and_then(Value::as_str)
                .unwrap_or(&url)
                .to_string()
        };
        self.remember_page_target(&tab_id, &loaded_url);
        let window_id = self.p2_window_id(&tab_id, timeout.min(5_000)).await;
        Ok(json!({
            "tab_id": tab_id,
            "window_id": window_id,
            "url": loaded_url,
            "active": active,
        }))
    }

    async fn p2_tabs_close(&self, args: &Value, timeout: u64) -> Result<Value, P2ActionError> {
        p2_check_args(args, &["tab_id"])?;
        let tab_id = p2_string(args, "tab_id")?;
        if !self.is_page_target(&tab_id) {
            return Err(P2ActionError::NotFound(
                "the requested tab was not found".into(),
            ));
        }
        self.send(
            "Target.closeTarget",
            json!({ "targetId": tab_id }),
            None,
            None,
            Some(timeout),
        )
        .await
        .map_err(p2_error)?;
        self.forget_target(&tab_id);
        Ok(json!({ "tab_id": tab_id, "closed": true }))
    }

    async fn p2_page_navigate(&self, args: &Value, timeout: u64) -> Result<Value, P2ActionError> {
        p2_check_args(args, &["url", "tab_id", "wait_until", "timeout_ms"])?;
        let url = p2_string(args, "url")?;
        if !p2_valid_url(&url) {
            return Err(P2ActionError::BadArgs(
                "url must be an http, https or about:blank address".into(),
            ));
        }
        let tab_id = p2_optional_string(args, "tab_id")?;
        let wait_until =
            p2_optional_string(args, "wait_until")?.unwrap_or_else(|| "load".to_string());
        if wait_until != "load" && wait_until != "domcontentloaded" {
            return Err(P2ActionError::BadArgs(
                "wait_until must be load or domcontentloaded".into(),
            ));
        }
        let requested = p2_integer(args, "timeout_ms", 30_000, 1_000, 60_000)?;
        let navigation_timeout = timeout.min(requested);
        let (session, target_id) = self
            .page(&p2_page_args(tab_id.as_deref()), timeout)
            .await
            .map_err(p2_error)?;
        let page = self
            .p2_navigate_page(&session, &url, &wait_until, navigation_timeout)
            .await?;
        self.remember_page_target(
            &target_id,
            page.get("url").and_then(Value::as_str).unwrap_or(&url),
        );
        let mut result = page;
        result["tab_id"] = json!(target_id);
        Ok(result)
    }

    async fn p2_page_read(&self, args: &Value, timeout: u64) -> Result<Value, P2ActionError> {
        p2_check_args(args, &["tab_id", "mode", "selector", "max_chars"])?;
        let tab_id = p2_optional_string(args, "tab_id")?;
        let mode = p2_optional_string(args, "mode")?.unwrap_or_else(|| "text".to_string());
        if mode != "text" && mode != "html" {
            return Err(P2ActionError::BadArgs("mode must be text or html".into()));
        }
        let selector = p2_optional_string(args, "selector")?;
        let max_chars = p2_integer(args, "max_chars", 20_000, 1_000, 200_000)? as usize;
        let (session, target_id) = self
            .page(&p2_page_args(tab_id.as_deref()), timeout)
            .await
            .map_err(p2_error)?;
        let expression = if let Some(selector) = selector.as_deref() {
            let selector = js_string(selector);
            if mode == "text" {
                format!(
                    "(() => {{ const node = document.querySelector({selector}); if (!node) return null; return {{ url: location.href, content: (node.innerText ?? node.textContent ?? '') }}; }})()"
                )
            } else {
                format!(
                    "(() => {{ const node = document.querySelector({selector}); if (!node) return null; return {{ url: location.href, content: (node.outerHTML ?? '') }}; }})()"
                )
            }
        } else if mode == "text" {
            "(() => ({ url: location.href, content: (document.body ? (document.body.innerText ?? '') : '') }))()".to_string()
        } else {
            "(() => ({ url: location.href, content: (document.documentElement ? document.documentElement.outerHTML : '') }))()".to_string()
        };
        let value = self
            .eval_value(&session, &expression, false)
            .await
            .map_err(p2_error)?;
        if value.is_null() {
            return Err(P2ActionError::NotFound(
                "the selector did not match an element".into(),
            ));
        }
        let content = value
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let (content, truncated) = truncate(content, max_chars);
        let url = value
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        Ok(json!({
            "tab_id": target_id,
            "url": url,
            "mode": mode,
            "content": content,
            "truncated": truncated,
        }))
    }

    async fn p2_page_snapshot(&self, args: &Value, timeout: u64) -> Result<Value, P2ActionError> {
        p2_check_args(args, &["tab_id", "max_nodes", "include_hidden"])?;
        let tab_id = p2_optional_string(args, "tab_id")?;
        let max_nodes = p2_integer(args, "max_nodes", 500, 50, 2_000)? as usize;
        let include_hidden = p2_bool(args, "include_hidden", false)?;
        let mut snapshot_args = json!({
            "format": "json",
            "mode": "full",
            "include_bbox": true,
            "max_nodes": max_nodes,
        });
        if let Some(tab_id) = tab_id.as_deref() {
            snapshot_args["tab_id"] = json!(tab_id);
        }
        let snapshot = self.snapshot_tool(&snapshot_args).await.map_err(p2_error)?;
        let mut truncated = snapshot
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mut nodes: Vec<Value> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for node in snapshot
            .get("nodes")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
        {
            let Some(reference) = node.get("ref").and_then(Value::as_str) else {
                continue;
            };
            seen.insert(reference.to_string());
            let mut item = json!({
                "ref": reference,
                "role": node.get("role").cloned().unwrap_or_else(|| json!("")),
                "name": node.get("name").cloned().unwrap_or_else(|| json!("")),
            });
            if let Some(value) = node.get("value").and_then(Value::as_str) {
                if !value.is_empty() {
                    item["value"] = json!(value);
                }
            }
            if let Some(bbox) = node.get("bbox") {
                item["bbox"] = bbox.clone();
            }
            nodes.push(item);
            if nodes.len() >= max_nodes {
                if snapshot
                    .get("nodes")
                    .and_then(Value::as_array)
                    .is_some_and(|all| all.len() > nodes.len())
                {
                    truncated = true;
                }
                break;
            }
        }
        if include_hidden {
            let (session, _) = self
                .page(&p2_page_args(tab_id.as_deref()), timeout)
                .await
                .map_err(p2_error)?;
            let ax = self
                .send(
                    "Accessibility.getFullAXTree",
                    json!({}),
                    Some(&session),
                    None,
                    Some(timeout),
                )
                .await
                .map_err(p2_error)?;
            for node in ax
                .get("nodes")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
            {
                if nodes.len() >= max_nodes {
                    truncated = true;
                    break;
                }
                if !node
                    .get("ignored")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    continue;
                }
                let Some(backend_id) = node.get("backendDOMNodeId").and_then(Value::as_i64) else {
                    continue;
                };
                let reference = format!("@c{backend_id}");
                if !seen.insert(reference.clone()) {
                    continue;
                }
                let role = node
                    .pointer("/role/value")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if role == "script" || role == "style" {
                    continue;
                }
                let name = node
                    .pointer("/name/value")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let value = node
                    .pointer("/value/value")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if name.is_empty() && value.is_empty() {
                    continue;
                }
                let mut item = json!({ "ref": reference, "role": role, "name": name });
                if !value.is_empty() {
                    item["value"] = json!(value);
                }
                nodes.push(item);
            }
        }
        let url = snapshot
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let tab_id = snapshot
            .get("tab_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        Ok(json!({
            "tab_id": tab_id,
            "url": url,
            "nodes": nodes,
            "truncated": truncated,
        }))
    }

    async fn p2_input_click(&self, args: &Value, timeout: u64) -> Result<Value, P2ActionError> {
        p2_check_args(args, &["ref", "tab_id"])?;
        let reference = p2_string(args, "ref")?;
        let tab_id = p2_optional_string(args, "tab_id")?;
        let (_, target_id) = self
            .page(&p2_page_args(tab_id.as_deref()), timeout)
            .await
            .map_err(p2_error)?;
        let click_args = json!({ "target": reference, "tab_id": target_id });
        self.click_tool(&click_args).await.map_err(p2_error)?;
        Ok(json!({
            "tab_id": target_id,
            "ref": reference,
            "clicked": true,
        }))
    }

    async fn p2_input_type(&self, args: &Value, timeout: u64) -> Result<Value, P2ActionError> {
        p2_check_args(args, &["ref", "text", "clear", "submit", "tab_id"])?;
        let reference = p2_string(args, "ref")?;
        let text = args
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| P2ActionError::BadArgs("text must be a string".into()))?
            .to_string();
        if text.encode_utf16().count() > 20_000 {
            return Err(P2ActionError::BadArgs(
                "text must be at most 20000 UTF-16 code units".into(),
            ));
        }
        let clear = p2_bool(args, "clear", true)?;
        let submit = p2_bool(args, "submit", false)?;
        let tab_id = p2_optional_string(args, "tab_id")?;
        let (session, target_id) = self
            .page(&p2_page_args(tab_id.as_deref()), timeout)
            .await
            .map_err(p2_error)?;
        let typed_chars = text.encode_utf16().count();
        let type_args = json!({
            "target": reference,
            "text": text,
            "clear": clear,
            "tab_id": target_id,
        });
        self.type_tool(&type_args).await.map_err(p2_error)?;
        let mut submitted = false;
        if submit {
            for event in ["keyDown", "keyUp"] {
                self.send(
                    "Input.dispatchKeyEvent",
                    json!({
                        "type": event,
                        "key": "Enter",
                        "code": "Enter",
                        "windowsVirtualKeyCode": 13,
                        "nativeVirtualKeyCode": 13,
                    }),
                    Some(&session),
                    None,
                    Some(timeout.min(10_000)),
                )
                .await
                .map_err(p2_error)?;
            }
            submitted = true;
        }
        Ok(json!({
            "tab_id": target_id,
            "ref": reference,
            "typed_chars": typed_chars,
            "submitted": submitted,
        }))
    }

    async fn p2_capture_viewport(
        &self,
        args: &Value,
        timeout: u64,
    ) -> Result<Value, P2ActionError> {
        p2_check_args(args, &["tab_id", "format", "quality"])?;
        let tab_id = p2_optional_string(args, "tab_id")?;
        let format = p2_optional_string(args, "format")?.unwrap_or_else(|| "png".to_string());
        if format != "png" && format != "jpeg" {
            return Err(P2ActionError::BadArgs("format must be png or jpeg".into()));
        }
        let quality = p2_integer(args, "quality", 80, 1, 100)?;
        let budget = timeout.min(15_000);
        let (session, target_id) = self
            .page(&p2_page_args(tab_id.as_deref()), budget)
            .await
            .map_err(p2_error)?;
        let url = self
            .eval_value(&session, "location.href", false)
            .await
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_default();
        let mut params = json!({
            "format": format,
            "fromSurface": true,
            "captureBeyondViewport": false,
        });
        if format != "png" {
            params["quality"] = json!(quality);
        }
        let captured = self
            .send(
                "Page.captureScreenshot",
                params,
                Some(&session),
                None,
                Some(budget),
            )
            .await
            .map_err(p2_error)?;
        let data = captured
            .get("data")
            .and_then(Value::as_str)
            .ok_or_else(|| P2ActionError::Internal("the browser returned no image".into()))?
            .to_string();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&data)
            .map_err(|_| P2ActionError::Internal("the browser returned an invalid image".into()))?;
        if bytes.len() > 8_388_608 {
            return Err(P2ActionError::Internal(
                "the screenshot exceeds the size limit".into(),
            ));
        }
        let (width, height) = p2_image_dimensions(&format, &bytes).ok_or_else(|| {
            P2ActionError::Internal("the screenshot dimensions are unavailable".into())
        })?;
        let mime_type = if format == "png" {
            "image/png"
        } else {
            "image/jpeg"
        };
        Ok(json!({
            "tab_id": target_id,
            "url": url,
            "format": format,
            "mime_type": mime_type,
            "width": width,
            "height": height,
            "data": data,
        }))
    }

    async fn p2_system_info(&self, args: &Value, timeout: u64) -> Result<Value, P2ActionError> {
        p2_check_args(args, &[])?;
        let version = self
            .send(
                "Browser.getVersion",
                json!({}),
                None,
                None,
                Some(timeout.min(5_000)),
            )
            .await
            .map_err(p2_error)?;
        let product = version
            .get("product")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let (family, browser_version) = product.split_once('/').unwrap_or((product, "unknown"));
        Ok(json!({
            "browser_family": family,
            "browser_version": browser_version,
            "capabilities": p2_capability_groups(),
            "limits": {
                "max_invoke_bytes": 1_048_576,
                "max_result_bytes": 8_388_608,
            },
        }))
    }

    async fn p2_window_id(&self, target_id: &str, timeout: u64) -> u64 {
        self.send(
            "Browser.getWindowForTarget",
            json!({ "targetId": target_id }),
            None,
            None,
            Some(timeout),
        )
        .await
        .ok()
        .and_then(|value| value.get("windowId").and_then(Value::as_u64))
        .unwrap_or(0)
    }

    async fn p2_navigate_page(
        &self,
        session: &str,
        url: &str,
        wait_until: &str,
        timeout: u64,
    ) -> Result<Value, P2ActionError> {
        let response = self
            .send(
                "Page.navigate",
                json!({ "url": url }),
                Some(session),
                None,
                Some(timeout),
            )
            .await
            .map_err(p2_navigation_error)?;
        if response
            .get("errorText")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.is_empty())
        {
            return Err(P2ActionError::NavigationFailed(
                "page navigation failed".into(),
            ));
        }
        let state = "(() => ({ readyState: document.readyState, url: location.href, title: document.title }))()";
        let deadline = Instant::now() + Duration::from_millis(timeout);
        loop {
            match self.eval_value(session, state, false).await {
                Ok(value) => {
                    let ready = value
                        .get("readyState")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let complete = if wait_until == "domcontentloaded" {
                        ready == "interactive" || ready == "complete"
                    } else {
                        ready == "complete"
                    };
                    if complete {
                        let final_url = value
                            .get("url")
                            .and_then(Value::as_str)
                            .unwrap_or(url)
                            .to_string();
                        let title = value
                            .get("title")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        let mut result = json!({ "url": final_url, "status": "loaded" });
                        if !title.is_empty() {
                            result["title"] = json!(title);
                        }
                        return Ok(result);
                    }
                }
                Err(error) if is_transient_context_error(&error) => {}
                Err(error) => return Err(p2_navigation_error(error)),
            }
            if Instant::now() >= deadline {
                return Err(P2ActionError::Timeout("page navigation timed out".into()));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn page(&self, args: &Value, timeout: u64) -> Result<(String, String), CdpError> {
        if !self.is_connected() {
            return Err(CdpError::NotConnected);
        }
        let target =
            match optional_string(args, "tab_id").or_else(|| optional_string(args, "target_id")) {
                Some(target) => {
                    if !self.is_page_target(&target) {
                        return Err(CdpError::TargetNotFound(target));
                    }
                    target
                }
                None => self.default_target_id().ok_or_else(|| {
                    CdpError::TargetNotFound("no page target is available".into())
                })?,
            };
        let session = self
            .attach(&target, Some(timeout))
            .await?
            .ok_or_else(|| CdpError::TargetNotFound(target.clone()))?;
        Ok((session, target))
    }

    async fn eval_value(
        &self,
        session: &str,
        expression: &str,
        await_promise: bool,
    ) -> Result<Value, CdpError> {
        let response = self
            .send(
                "Runtime.evaluate",
                json!({
                    "expression": expression,
                    "returnByValue": true,
                    "awaitPromise": await_promise,
                }),
                Some(session),
                None,
                Some(15_000),
            )
            .await?;
        if let Some(exception) = response.get("exceptionDetails") {
            let message = exception
                .pointer("/exception/description")
                .and_then(Value::as_str)
                .or_else(|| exception.get("text").and_then(Value::as_str))
                .unwrap_or("evaluation failed");
            return Err(CdpError::Protocol(message.to_string()));
        }
        Ok(response
            .pointer("/result/value")
            .cloned()
            .unwrap_or(Value::Null))
    }

    async fn wait_for_load_event(
        &self,
        session: &str,
        since: u64,
        timeout: Duration,
    ) -> Result<(), CdpError> {
        let deadline = Instant::now() + timeout;
        let mut cursor = since;
        loop {
            let batch = self
                .events(
                    Some(cursor),
                    Some(100),
                    Some("Page.loadEventFired"),
                    Some(200),
                    false,
                )
                .await;
            cursor = batch.next_seq;
            if batch
                .events
                .iter()
                .any(|event| event.session_id.as_deref() == Some(session))
            {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(CdpError::Timeout(timeout.as_millis() as u64));
            }
        }
    }

    /// Wait for a document that has fully loaded and settled on its final URL.
    ///
    /// `Target.createTarget` can report the requested URL while the initial
    /// about:blank document is still current and the navigation is in flight.
    /// Redirects then replace the requested URL, so tab_new must wait for a
    /// complete document whose URL is stable across two polls.
    async fn wait_loaded_url(
        &self,
        session: &str,
        requested: &str,
        timeout: Duration,
    ) -> Result<String, CdpError> {
        let deadline = Instant::now() + timeout;
        let mut stable: Option<String> = None;
        loop {
            let state = self
                .eval_value(session, "document.readyState", false)
                .await
                .unwrap_or(Value::Null);
            let current = self
                .eval_value(session, "location.href", false)
                .await
                .unwrap_or(Value::Null);
            let current = current.as_str().unwrap_or_default();
            let finished = state.as_str() == Some("complete")
                && !current.is_empty()
                && (current != "about:blank" || requested == "about:blank");
            if finished {
                if stable.as_deref() == Some(current) {
                    return Ok(current.to_string());
                }
                stable = Some(current.to_string());
            } else {
                stable = None;
            }
            if Instant::now() >= deadline {
                return Err(CdpError::Timeout(timeout.as_millis() as u64));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn wait_ready(&self, session: &str, timeout: Duration) -> Result<(), CdpError> {
        let deadline = Instant::now() + timeout;
        loop {
            let state = self
                .eval_value(session, "document.readyState", false)
                .await
                .unwrap_or(Value::Null);
            if state
                .as_str()
                .is_some_and(|state| state == "complete" || state == "interactive")
                || Instant::now() >= deadline
            {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn page_pdf_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let paper_width = bounded_number(args, "paper_width", 8.5, 1.0, 100.0)?;
        let paper_height = bounded_number(args, "paper_height", 11.0, 1.0, 100.0)?;
        let landscape = args
            .get("landscape")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let print_background = args
            .get("print_background")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let scale = bounded_number(args, "scale", 1.0, 0.1, 2.0)?;
        let (session, target_id) = self.page(args, 30_000).await?;
        let url = self.current_url(&session).await?;
        let response = self
            .send(
                "Page.printToPDF",
                json!({
                    "printBackground": print_background,
                    "landscape": landscape,
                    "paperWidth": paper_width,
                    "paperHeight": paper_height,
                    "scale": scale,
                    "marginTop": 0.4,
                    "marginBottom": 0.4,
                    "marginLeft": 0.4,
                    "marginRight": 0.4,
                    "preferCSSPageSize": false,
                    "transferMode": "ReturnAsBase64",
                }),
                Some(&session),
                None,
                Some(60_000),
            )
            .await
            .map_err(pdf_error)?;
        let data = response
            .get("data")
            .and_then(Value::as_str)
            .ok_or_else(|| CdpError::Protocol("Page.printToPDF returned no data".into()))?;
        Ok(json!({
            "format": "pdf",
            "mime_type": "application/pdf",
            "tab_id": target_id,
            "url": url,
            "data": data,
            "paper_width": paper_width,
            "paper_height": paper_height,
            "landscape": landscape,
            "scale": scale,
        }))
    }

    async fn page_links_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let selector = optional_string(args, "selector");
        if selector
            .as_ref()
            .is_some_and(|value| value.chars().count() > 512)
        {
            return Err(CdpError::InvalidArgument(
                "selector must be at most 512 characters".into(),
            ));
        }
        let max_results = integer(args, "max_results", 200, 1, 1000);
        let include_hidden = args
            .get("include_hidden")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let (session, target_id) = self.page(args, 15_000).await?;
        let expression = PAGE_LINKS_JS
            .replace(
                "__SELECTOR__",
                &selector
                    .as_deref()
                    .map(js_string)
                    .unwrap_or_else(|| "null".into()),
            )
            .replace("__MAX__", &max_results.to_string())
            .replace("__HIDDEN__", if include_hidden { "true" } else { "false" });
        let value = self.eval_value(&session, &expression, false).await?;
        if value.get("missing").and_then(Value::as_bool) == Some(true) {
            return Err(CdpError::TargetNotFound(
                selector.unwrap_or_else(|| "selector".into()),
            ));
        }
        let links = value
            .get("links")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(json!({
            "tab_id": target_id,
            "url": value.get("url").cloned().unwrap_or(Value::Null),
            "count": links.len(),
            "truncated": value.get("truncated").and_then(Value::as_bool).unwrap_or(false),
            "links": links,
        }))
    }

    async fn page_forms_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let selector = optional_string(args, "selector");
        if selector
            .as_ref()
            .is_some_and(|value| value.chars().count() > 512)
        {
            return Err(CdpError::InvalidArgument(
                "selector must be at most 512 characters".into(),
            ));
        }
        let include_values = args
            .get("include_values")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let max_forms = integer(args, "max_forms", 20, 1, 100);
        let (session, target_id) = self.page(args, 15_000).await?;
        let expression = PAGE_FORMS_JS
            .replace(
                "__SELECTOR__",
                &selector
                    .as_deref()
                    .map(js_string)
                    .unwrap_or_else(|| "null".into()),
            )
            .replace("__MAX__", &max_forms.to_string())
            .replace("__VALUES__", if include_values { "true" } else { "false" });
        let value = self.eval_value(&session, &expression, false).await?;
        if value.get("missing").and_then(Value::as_bool) == Some(true) {
            return Err(CdpError::TargetNotFound(
                selector.unwrap_or_else(|| "selector".into()),
            ));
        }
        let forms = value
            .get("forms")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(json!({
            "tab_id": target_id,
            "url": value.get("url").cloned().unwrap_or(Value::Null),
            "count": forms.len(),
            "truncated": value.get("truncated").and_then(Value::as_bool).unwrap_or(false),
            "forms": forms,
        }))
    }

    async fn page_accessibility_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let root = optional_string(args, "root");
        if root
            .as_ref()
            .is_some_and(|value| value.chars().count() > 512)
        {
            return Err(CdpError::InvalidArgument(
                "root must be at most 512 characters".into(),
            ));
        }
        let max_nodes = integer(args, "max_nodes", 500, 1, 2000);
        let max_depth = integer(args, "max_depth", 12, 1, 50);
        let (session, target_id) = self.page(args, 30_000).await?;
        let url = self.current_url(&session).await?;
        let response = if let Some(root) = &root {
            let object_id = self.element_object(&session, root).await?;
            self.send(
                "Accessibility.getPartialAXTree",
                json!({ "objectId": object_id, "fetchRelatives": false }),
                Some(&session),
                None,
                Some(20_000),
            )
            .await
            .map_err(accessibility_error)?
        } else {
            self.send(
                "Accessibility.getFullAXTree",
                json!({ "depth": max_depth }),
                Some(&session),
                None,
                Some(30_000),
            )
            .await
            .map_err(accessibility_error)?
        };
        let raw = response
            .get("nodes")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut depths: HashMap<String, u64> = HashMap::new();
        let mut nodes = Vec::new();
        let mut truncated = false;
        let mut filtered = 0usize;
        for node in &raw {
            let depth = node
                .get("parentId")
                .and_then(Value::as_str)
                .and_then(|parent| depths.get(parent).copied())
                .map_or(0, |parent| parent + 1);
            if let Some(id) = node.get("nodeId").and_then(Value::as_str) {
                depths.insert(id.to_string(), depth);
            }
            if node
                .get("ignored")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                continue;
            }
            let role = node
                .pointer("/role/value")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if role == "none" || role == "presentation" {
                continue;
            }
            filtered += 1;
            if depth >= max_depth {
                if node
                    .get("childIds")
                    .and_then(Value::as_array)
                    .is_some_and(|children| !children.is_empty())
                {
                    truncated = true;
                }
                continue;
            }
            if nodes.len() >= max_nodes as usize {
                truncated = true;
                continue;
            }
            let mut entry = Map::new();
            entry.insert("role".into(), json!(role));
            entry.insert("name".into(), json!(ax_string(node, "name")));
            for (key, pointer) in [
                ("value", "/value/value"),
                ("description", "/description/value"),
            ] {
                if let Some(text) = node.pointer(pointer).and_then(Value::as_str) {
                    entry.insert(key.into(), json!(cap_chars(text, 200)));
                }
            }
            for property in node
                .get("properties")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let name = property
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if ["disabled", "checked", "expanded", "selected", "level"].contains(&name) {
                    if let Some(value) = property.pointer("/value/value") {
                        entry.insert(name.into(), value.clone());
                    }
                }
            }
            entry.insert("depth".into(), json!(depth));
            nodes.push(Value::Object(entry));
        }
        if filtered > nodes.len() {
            truncated = true;
        }
        Ok(json!({
            "tab_id": target_id,
            "url": url,
            "count": nodes.len(),
            "truncated": truncated,
            "nodes": nodes,
        }))
    }

    async fn drag_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let from_ref = required_string(args, "from_ref")?;
        let to_ref = required_string(args, "to_ref")?;
        if from_ref == to_ref {
            return Err(CdpError::InvalidArgument(
                "from_ref and to_ref must be different elements".into(),
            ));
        }
        let steps = integer(args, "steps", 10, 1, 50);
        let (session, target_id) = self.page(args, 15_000).await?;
        let (from_object, _) = self.resolve_element(&session, &from_ref).await?;
        let (to_object, _) = self.resolve_element(&session, &to_ref).await?;
        self.scroll_into_view(&session, &to_object).await?;
        let from = self.border_center(&session, &from_object).await?;
        let to = self.border_center(&session, &to_object).await?;
        self.dispatch_mouse(&session, "mouseMoved", from.0, from.1, "none", 0, None)
            .await?;
        self.dispatch_mouse(&session, "mousePressed", from.0, from.1, "left", 1, Some(1))
            .await?;
        for step in 1..=steps {
            let fraction = step as f64 / steps as f64;
            let x = from.0 + (to.0 - from.0) * fraction;
            let y = from.1 + (to.1 - from.1) * fraction;
            self.dispatch_mouse(&session, "mouseMoved", x, y, "left", 1, None)
                .await?;
        }
        self.dispatch_mouse(&session, "mouseReleased", to.0, to.1, "left", 0, Some(1))
            .await?;
        let url = self.current_url(&session).await?;
        Ok(json!({
            "dragged": true,
            "from": { "ref": from_ref, "x": round2(from.0), "y": round2(from.1) },
            "to": { "ref": to_ref, "x": round2(to.0), "y": round2(to.1) },
            "tab_id": target_id,
            "url": url,
        }))
    }

    async fn focus_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let reference = required_string(args, "ref")?;
        let (session, target_id) = self.page(args, 15_000).await?;
        let (object_id, _) = self.resolve_element(&session, &reference).await?;
        self.scroll_into_view(&session, &object_id).await?;
        self.send(
            "DOM.focus",
            json!({ "objectId": object_id }),
            Some(&session),
            None,
            Some(10_000),
        )
        .await
        .map_err(interaction_error)?;
        let url = self.current_url(&session).await?;
        Ok(json!({
            "focused": true,
            "ref": reference,
            "tab_id": target_id,
            "url": url,
        }))
    }

    async fn highlight_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let reference = optional_string(args, "ref");
        let selector = optional_string(args, "selector");
        if reference.is_some() == selector.is_some() {
            return Err(CdpError::InvalidArgument(
                "browser_highlight requires exactly one of ref or selector".into(),
            ));
        }
        let duration_ms = integer(args, "duration_ms", 1200, 0, 10_000);
        let color = args
            .get("color")
            .and_then(Value::as_str)
            .unwrap_or("#ffd400")
            .to_string();
        let (r, g, b) = parse_hex_color(&color)?;
        let (session, target_id) = self.page(args, 15_000).await?;
        let mut params = Map::new();
        if let Some(reference) = &reference {
            params.insert(
                "backendNodeId".into(),
                json!(screenshot_backend_id(reference)?),
            );
        } else if let Some(selector) = &selector {
            let object_id = self.element_object(&session, selector).await?;
            let response = self
                .send(
                    "DOM.describeNode",
                    json!({ "objectId": object_id, "depth": 0 }),
                    Some(&session),
                    None,
                    Some(10_000),
                )
                .await?;
            let backend_id = response
                .pointer("/node/backendNodeId")
                .and_then(Value::as_i64)
                .ok_or_else(|| CdpError::TargetNotFound(selector.clone()))?;
            params.insert("backendNodeId".into(), json!(backend_id));
        }
        params.insert(
            "highlightConfig".into(),
            json!({
                "contentColor": { "r": r, "g": g, "b": b, "a": 0.35 },
                "borderColor": { "r": r, "g": g, "b": b, "a": 0.9 },
                "showInfo": false,
            }),
        );
        let _ = self
            .send(
                "Overlay.enable",
                json!({}),
                Some(&session),
                None,
                Some(10_000),
            )
            .await;
        self.send(
            "Overlay.highlightNode",
            Value::Object(params),
            Some(&session),
            None,
            Some(10_000),
        )
        .await?;
        if duration_ms == 0 {
            let _ = self
                .send(
                    "Overlay.hideHighlight",
                    json!({}),
                    Some(&session),
                    None,
                    Some(10_000),
                )
                .await;
        } else {
            tokio::time::sleep(Duration::from_millis(duration_ms)).await;
            let _ = self
                .send(
                    "Overlay.hideHighlight",
                    json!({}),
                    Some(&session),
                    None,
                    Some(10_000),
                )
                .await;
        }
        let url = self.current_url(&session).await?;
        let mut document = json!({
            "highlighted": true,
            "duration_ms": duration_ms,
            "tab_id": target_id,
            "url": url,
        });
        if let Some(reference) = reference {
            document["ref"] = json!(reference);
        }
        if let Some(selector) = selector {
            document["selector"] = json!(selector);
        }
        Ok(document)
    }

    async fn cookie_set_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let name = required_string(args, "name")?;
        if name.chars().count() > 256 {
            return Err(CdpError::InvalidArgument(
                "name must be at most 256 characters".into(),
            ));
        }
        let value = required_string_allow_empty(args, "value")?;
        if value.chars().count() > 4096 {
            return Err(CdpError::InvalidArgument(
                "value must be at most 4096 characters".into(),
            ));
        }
        let (session, target_id) = self.page(args, 15_000).await?;
        let current = self.current_url(&session).await?;
        let current = current.as_str();
        let domain = optional_string(args, "domain");
        let mut url = optional_string(args, "url");
        if url.is_none() {
            if let Some(domain) = &domain {
                let scheme = current.and_then(web_scheme).unwrap_or("https");
                url = Some(format!("{scheme}://{domain}/"));
            } else {
                url = current.map(str::to_string);
            }
        }
        let Some(url) = url else {
            return Err(CdpError::InvalidArgument(
                "cookie_set needs url, domain or a web tab URL".into(),
            ));
        };
        if !is_web_url(&url) {
            return Err(CdpError::InvalidArgument(
                "cookie_set requires an http or https URL".into(),
            ));
        }
        let path = optional_string(args, "path").unwrap_or_else(|| "/".into());
        let secure = args.get("secure").and_then(Value::as_bool).unwrap_or(false);
        let http_only = args
            .get("http_only")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let same_site = args
            .get("same_site")
            .and_then(Value::as_str)
            .unwrap_or("lax")
            .to_string();
        let browser_same_site = match same_site.as_str() {
            "strict" => "Strict",
            "lax" => "Lax",
            "none" => "None",
            other => {
                return Err(CdpError::InvalidArgument(format!(
                    "same_site must be strict, lax or none, got {other}"
                )))
            }
        };
        let expires = args.get("expires").and_then(Value::as_f64).unwrap_or(0.0);
        if expires < 0.0 || !expires.is_finite() {
            return Err(CdpError::InvalidArgument(
                "expires must be a non-negative number".into(),
            ));
        }
        let mut params = Map::new();
        params.insert("name".into(), json!(name));
        params.insert("value".into(), json!(value));
        params.insert("url".into(), json!(url));
        params.insert("path".into(), json!(path));
        params.insert("secure".into(), json!(secure));
        params.insert("httpOnly".into(), json!(http_only));
        params.insert("sameSite".into(), json!(browser_same_site));
        if let Some(domain) = &domain {
            params.insert("domain".into(), json!(domain));
        }
        if expires > 0.0 {
            params.insert("expires".into(), json!(expires));
        }
        let response = self
            .send(
                "Network.setCookie",
                Value::Object(params),
                Some(&session),
                None,
                Some(10_000),
            )
            .await
            .map_err(cookie_error)?;
        if response.get("success").and_then(Value::as_bool) == Some(false) {
            return Err(CdpError::Protocol("the browser rejected the cookie".into()));
        }
        let cookie_domain = response
            .get("domain")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or(domain)
            .unwrap_or_default();
        Ok(json!({
            "set": true,
            "name": name,
            "domain": cookie_domain,
            "path": path,
            "secure": secure,
            "http_only": http_only,
            "same_site": same_site,
            "expires": expires as i64,
            "tab_id": target_id,
            "url": url,
        }))
    }

    async fn cookie_clear_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let (session, _) = self.page(args, 15_000).await?;
        let current = self.current_url(&session).await?;
        let current = current.as_str();
        let mut url = optional_string(args, "url");
        let domain = optional_string(args, "domain");
        let name = optional_string(args, "name");
        if url.is_none() && domain.is_none() {
            url = current.map(str::to_string);
            if url.is_none() {
                return Err(CdpError::InvalidArgument(
                    "cookie_clear needs url, domain or a usable tab URL".into(),
                ));
            }
        }
        if let Some(url) = &url {
            if !is_web_url(url) {
                return Err(CdpError::InvalidArgument(
                    "cookie_clear requires an http or https URL".into(),
                ));
            }
        }
        let response = match &url {
            Some(url) => self
                .send(
                    "Network.getCookies",
                    json!({ "urls": [url] }),
                    Some(&session),
                    None,
                    Some(10_000),
                )
                .await
                .map_err(cookie_error)?,
            None => self
                .send("Storage.getCookies", json!({}), None, None, Some(10_000))
                .await
                .map_err(cookie_error)?,
        };
        let cookies = response
            .get("cookies")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut names = Vec::new();
        for cookie in cookies {
            let cookie_name = cookie
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if name.as_deref().is_some_and(|wanted| wanted != cookie_name) {
                continue;
            }
            let cookie_domain = cookie
                .get("domain")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if domain.as_deref().is_some_and(|wanted| {
                cookie_domain != wanted && !cookie_domain.ends_with(&format!(".{wanted}"))
            }) {
                continue;
            }
            let mut params = Map::new();
            params.insert("name".into(), json!(cookie_name));
            if !cookie_domain.is_empty() {
                params.insert("domain".into(), json!(cookie_domain));
            }
            if let Some(path) = cookie.get("path").and_then(Value::as_str) {
                params.insert("path".into(), json!(path));
            }
            self.send(
                "Network.deleteCookies",
                Value::Object(params),
                Some(&session),
                None,
                Some(10_000),
            )
            .await
            .map_err(cookie_error)?;
            if names.len() < 100 && !names.iter().any(|existing| existing == cookie_name) {
                names.push(cookie_name.to_string());
            }
        }
        let mut document = json!({
            "cleared": names.len(),
            "names": names,
        });
        if let Some(url) = url {
            document["url"] = json!(url);
        }
        if let Some(domain) = domain {
            document["domain"] = json!(domain);
        }
        if let Some(name) = name {
            document["name"] = json!(name);
        }
        Ok(document)
    }

    async fn storage_get_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let (session, _) = self.page(args, 15_000).await?;
        let area = storage_area(args)?;
        let keys = optional_string_array(args, "keys");
        let expression = STORAGE_GET_JS
            .replace("__AREA__", &js_string(&area))
            .replace(
                "__KEYS__",
                &keys
                    .as_ref()
                    .map(|values| json!(values).to_string())
                    .unwrap_or_else(|| "null".into()),
            );
        let value = self.eval_value(&session, &expression, false).await?;
        let origin = value
            .get("origin")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if origin.is_empty() || !is_web_url(&origin) {
            return Err(CdpError::InvalidArgument(
                "storage tools require an http or https page".into(),
            ));
        }
        let raw = value
            .get("entries")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut entries = Vec::new();
        let mut total = 0usize;
        let mut truncated = false;
        for entry in raw {
            if entries.len() >= 200 {
                truncated = true;
                break;
            }
            let Some(key) = entry.get("key").and_then(Value::as_str) else {
                continue;
            };
            let mut value = entry
                .get("value")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if value.chars().count() > 4096 {
                value = cap_chars(&value, 4096);
                truncated = true;
            }
            let length = key.chars().count() + value.chars().count();
            if total + length > 65_536 {
                truncated = true;
                break;
            }
            total += length;
            entries.push(json!({ "key": key, "value": value }));
        }
        Ok(json!({
            "area": area,
            "origin": origin,
            "count": entries.len(),
            "truncated": truncated,
            "entries": entries,
        }))
    }

    async fn storage_set_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let entries = args
            .get("entries")
            .and_then(Value::as_object)
            .cloned()
            .ok_or_else(|| CdpError::InvalidArgument("entries must be an object".into()))?;
        let (session, _) = self.page(args, 15_000).await?;
        let area = storage_area(args)?;
        let expression = STORAGE_SET_JS
            .replace("__AREA__", &js_string(&area))
            .replace("__ENTRIES__", &Value::Object(entries).to_string());
        let value = self.eval_value(&session, &expression, false).await?;
        if value.get("ok").and_then(Value::as_bool) != Some(true) {
            return Err(CdpError::Protocol(format!(
                "web storage write failed: {}",
                value
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error")
            )));
        }
        let origin = value
            .get("origin")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if !is_web_url(&origin) {
            return Err(CdpError::InvalidArgument(
                "storage tools require an http or https page".into(),
            ));
        }
        let keys = value
            .get("keys")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(json!({
            "area": area,
            "origin": origin,
            "set": keys.len(),
            "keys": keys,
        }))
    }

    async fn storage_clear_tool(&self, args: &Value) -> Result<Value, CdpError> {
        let (session, _) = self.page(args, 15_000).await?;
        let area = storage_area(args)?;
        let keys = optional_string_array(args, "keys");
        let expression = STORAGE_CLEAR_JS
            .replace("__AREA__", &js_string(&area))
            .replace(
                "__KEYS__",
                &keys
                    .as_ref()
                    .map(|values| json!(values).to_string())
                    .unwrap_or_else(|| "null".into()),
            );
        let value = self.eval_value(&session, &expression, false).await?;
        let origin = value
            .get("origin")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if !is_web_url(&origin) {
            return Err(CdpError::InvalidArgument(
                "storage tools require an http or https page".into(),
            ));
        }
        Ok(json!({
            "area": area,
            "origin": origin,
            "cleared": value.get("cleared").and_then(Value::as_u64).unwrap_or(0),
        }))
    }

    async fn border_center(&self, session: &str, object_id: &str) -> Result<(f64, f64), CdpError> {
        let model = self
            .send(
                "DOM.getBoxModel",
                json!({ "objectId": object_id }),
                Some(session),
                None,
                Some(10_000),
            )
            .await
            .map_err(interaction_error)?;
        let quad = model
            .pointer("/model/border")
            .and_then(Value::as_array)
            .filter(|quad| quad.len() >= 8)
            .ok_or_else(|| CdpError::NotInteractable("element has no layout box".into()))?;
        let numbers: Vec<f64> = quad
            .iter()
            .take(8)
            .map(|value| value.as_f64().unwrap_or(0.0))
            .collect();
        Ok((
            (numbers[0] + numbers[4]) / 2.0,
            (numbers[1] + numbers[5]) / 2.0,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    async fn dispatch_mouse(
        &self,
        session: &str,
        event: &str,
        x: f64,
        y: f64,
        button: &str,
        buttons: i64,
        click_count: Option<i64>,
    ) -> Result<(), CdpError> {
        let mut params = json!({
            "type": event,
            "x": x,
            "y": y,
            "button": button,
            "buttons": buttons,
        });
        if let Some(click_count) = click_count {
            params["clickCount"] = json!(click_count);
        }
        self.send(
            "Input.dispatchMouseEvent",
            params,
            Some(session),
            None,
            Some(10_000),
        )
        .await?;
        Ok(())
    }

    fn target_for_session(&self, session: Option<&str>) -> Option<String> {
        let session = session?;
        self.targets()
            .into_iter()
            .find(|target| target.session_id.as_deref() == Some(session))
            .map(|target| target.target_id)
    }
}

#[derive(Clone, Debug)]
struct FrameNode {
    path: String,
    frame_id: String,
    url: String,
}

#[derive(Clone, Debug)]
struct FrameData {
    path: String,
    ax: Value,
    document_index: Option<usize>,
    origin: (f64, f64),
    scroll: (f64, f64),
}

fn frames_summary(
    frame_nodes: &[FrameNode],
    included: &[FrameData],
    skipped: Vec<Value>,
) -> Option<Value> {
    let total = frame_nodes
        .iter()
        .filter(|node| !node.path.is_empty())
        .filter(|node| {
            let parent = node.path.rsplit_once('.').map_or("", |(parent, _)| parent);
            included.iter().any(|frame| frame.path == parent)
        })
        .count();
    if total == 0 {
        return None;
    }
    Some(json!({
        "total": total,
        "included": included.iter().filter(|frame| !frame.path.is_empty()).count(),
        "skipped": skipped,
    }))
}

fn flatten_frame_tree(tree: &Value) -> Vec<FrameNode> {
    fn visit(node: &Value, path: &str, out: &mut Vec<FrameNode>) {
        let frame_id = node
            .pointer("/frame/id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let url = node
            .pointer("/frame/url")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        out.push(FrameNode {
            path: path.to_string(),
            frame_id,
            url,
        });
        let Some(children) = node.get("childFrames").and_then(Value::as_array) else {
            return;
        };
        for (index, child) in children.iter().enumerate() {
            let child_path = if path.is_empty() {
                format!("f{index}")
            } else {
                format!("{path}.{index}")
            };
            visit(child, &child_path, out);
        }
    }
    let mut out = Vec::new();
    if let Some(root) = tree.get("frameTree") {
        visit(root, "", &mut out);
    }
    out
}

fn snapshot_string(strings: &[Value], value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => Some(text.clone()),
        Value::Number(_) => value
            .and_then(Value::as_u64)
            .and_then(|index| strings.get(index as usize))
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => None,
    }
}

fn snapshot_document_index(
    documents: &[Value],
    strings: &[Value],
    frame_id: &str,
) -> Option<usize> {
    documents.iter().position(|document| {
        snapshot_string(strings, document.get("frameId")).as_deref() == Some(frame_id)
    })
}

fn document_scroll(document: &Value) -> (f64, f64) {
    (
        document
            .get("scrollOffsetX")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        document
            .get("scrollOffsetY")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
    )
}

fn url_origin(url: &str) -> Option<&str> {
    let (_, rest) = url.split_once("://")?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let prefix = url.len() - rest.len();
    Some(&url[..prefix + end])
}

fn same_origin(left: &str, right: &str) -> bool {
    if left.is_empty() || right.is_empty() {
        return true;
    }
    if left.starts_with("about:") || right.starts_with("about:") {
        return true;
    }
    match (url_origin(left), url_origin(right)) {
        (Some(left), Some(right)) => left.eq_ignore_ascii_case(right),
        _ => true,
    }
}

#[derive(Clone, Debug)]
struct ElementInfo {
    tag: String,
    role: String,
    name: String,
    disabled: bool,
    visible: bool,
    rect: ElementBox,
}

#[derive(Clone, Copy, Debug)]
struct ElementBox {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl ElementBox {
    fn as_json(self) -> Value {
        json!({
            "x": self.x,
            "y": self.y,
            "width": self.width,
            "height": self.height,
        })
    }
}

fn ensure_interactable(info: &ElementInfo) -> Result<(), CdpError> {
    if !info.visible || info.rect.width <= 0.0 || info.rect.height <= 0.0 {
        return Err(CdpError::NotInteractable("element is not visible".into()));
    }
    if info.disabled {
        return Err(CdpError::NotInteractable("element is disabled".into()));
    }
    Ok(())
}

/// A box-model failure means the element cannot be clicked. Transport and
/// timeout failures keep their own identity so callers do not mistake a dead
/// connection for a non-interactable element.
fn interaction_error(error: CdpError) -> CdpError {
    match error {
        CdpError::Transport(_) | CdpError::NotConnected | CdpError::Timeout(_) => error,
        other => CdpError::NotInteractable(other.to_string()),
    }
}

fn json_i64(value: Option<&Value>) -> i64 {
    value
        .and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_f64().map(|number| number as i64))
        })
        .unwrap_or(0)
}

fn json_f64(value: Option<&Value>) -> f64 {
    value
        .and_then(|value| value.as_f64().or_else(|| value.as_i64().map(|n| n as f64)))
        .unwrap_or(0.0)
}

#[derive(Clone, Debug)]
struct KeyStroke {
    key: String,
    code: String,
    key_code: i64,
    text: Option<String>,
}

/// Parse one key specification such as Enter, Control+a or Shift+2.
fn parse_key(spec: &str) -> Result<(i64, KeyStroke), CdpError> {
    let mut parts: Vec<&str> = spec.split('+').collect();
    let raw_key = parts.pop().unwrap_or_default();
    if raw_key.is_empty() {
        return Err(CdpError::InvalidArgument(format!(
            "key {spec} does not name a key"
        )));
    }
    let mut modifiers = 0i64;
    for modifier in parts {
        match modifier.to_ascii_lowercase().as_str() {
            "alt" => modifiers |= 1,
            "ctrl" | "control" => modifiers |= 2,
            "meta" | "cmd" | "command" => modifiers |= 4,
            "shift" => modifiers |= 8,
            other => {
                return Err(CdpError::InvalidArgument(format!(
                    "key {spec} has unsupported modifier {other}"
                )))
            }
        }
    }
    let lower = raw_key.to_ascii_lowercase();
    let named = match lower.as_str() {
        "enter" => Some(("Enter", "Enter", 13, Some("\r".to_string()))),
        "tab" => Some(("Tab", "Tab", 9, None)),
        "escape" | "esc" => Some(("Escape", "Escape", 27, None)),
        "backspace" => Some(("Backspace", "Backspace", 8, None)),
        "delete" => Some(("Delete", "Delete", 46, None)),
        "space" | " " => Some((" ", "Space", 32, Some(" ".to_string()))),
        "arrowup" | "up" => Some(("ArrowUp", "ArrowUp", 38, None)),
        "arrowdown" | "down" => Some(("ArrowDown", "ArrowDown", 40, None)),
        "arrowleft" | "left" => Some(("ArrowLeft", "ArrowLeft", 37, None)),
        "arrowright" | "right" => Some(("ArrowRight", "ArrowRight", 39, None)),
        "pageup" => Some(("PageUp", "PageUp", 33, None)),
        "pagedown" => Some(("PageDown", "PageDown", 34, None)),
        "home" => Some(("Home", "Home", 36, None)),
        "end" => Some(("End", "End", 35, None)),
        _ => None,
    };
    if let Some((key, code, key_code, text)) = named {
        let text = text.filter(|_| modifiers & (1 | 2 | 4) == 0);
        return Ok((
            modifiers,
            KeyStroke {
                key: key.to_string(),
                code: code.to_string(),
                key_code,
                text,
            },
        ));
    }
    if raw_key.chars().count() != 1 {
        return Err(CdpError::InvalidArgument(format!(
            "key {spec} is not supported"
        )));
    }
    let character = raw_key.chars().next().unwrap_or_default();
    let (mut key, code, key_code, mut text) = if character.is_ascii_alphabetic() {
        let upper = character.to_ascii_uppercase();
        (
            upper.to_string(),
            format!("Key{upper}"),
            upper as i64,
            Some(character.to_string()),
        )
    } else if character.is_ascii_digit() {
        let shifted = match character {
            '1' => Some('!'),
            '2' => Some('@'),
            '3' => Some('#'),
            '4' => Some('$'),
            '5' => Some('%'),
            '6' => Some('^'),
            '7' => Some('&'),
            '8' => Some('*'),
            '9' => Some('('),
            '0' => Some(')'),
            _ => None,
        };
        let rendered = if modifiers & 8 != 0 {
            shifted.unwrap_or(character)
        } else {
            character
        };
        (
            rendered.to_string(),
            format!("Digit{character}"),
            character as i64,
            Some(rendered.to_string()),
        )
    } else {
        let (code, key_code) = match character {
            ';' => ("Semicolon", 186),
            '=' => ("Equal", 187),
            ',' => ("Comma", 188),
            '-' => ("Minus", 189),
            '.' => ("Period", 190),
            '/' => ("Slash", 191),
            '\x60' => ("Backquote", 192),
            '[' => ("BracketLeft", 219),
            '\\' => ("Backslash", 220),
            ']' => ("BracketRight", 221),
            '\'' => ("Quote", 222),
            _ => {
                return Err(CdpError::InvalidArgument(format!(
                    "key {spec} is not supported"
                )))
            }
        };
        (
            character.to_string(),
            code.to_string(),
            key_code,
            Some(character.to_string()),
        )
    };
    if modifiers & (1 | 2 | 4) != 0 {
        text = None;
    }
    if modifiers & 8 != 0 && key.len() == 1 {
        key = key.to_ascii_uppercase();
        if let Some(value) = text.as_mut() {
            *value = value.to_ascii_uppercase();
        }
    }
    Ok((
        modifiers,
        KeyStroke {
            key,
            code,
            key_code,
            text,
        },
    ))
}

/// Chrome reports an absent dialog as a protocol error. That is an expected
/// negative result, not an internal failure; every other protocol error is
/// left untouched so callers can classify it normally.
fn dialog_error(error: CdpError) -> CdpError {
    if let CdpError::Remote { code, message } = &error {
        if *code == -32602
            && message
                .to_ascii_lowercase()
                .contains("no dialog is showing")
        {
            return CdpError::DialogNotFound;
        }
    }
    error
}

/// Navigation can destroy the execution context while a page tool is
/// polling. Anything else, including a closed transport, is a real failure.
fn is_transient_context_error(error: &CdpError) -> bool {
    match error {
        CdpError::Remote { message, .. } => {
            let message = message.to_ascii_lowercase();
            message.contains("execution context")
                || message.contains("cannot find context")
                || message.contains("context with specified id")
        }
        _ => false,
    }
}

fn p2_check_args(args: &Value, allowed: &[&str]) -> Result<(), P2ActionError> {
    let object = args
        .as_object()
        .ok_or_else(|| P2ActionError::BadArgs("arguments must be an object".into()))?;
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(P2ActionError::BadArgs("unknown argument".into()));
    }
    Ok(())
}

fn p2_string(args: &Value, key: &str) -> Result<String, P2ActionError> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| P2ActionError::BadArgs(format!("{key} must be a non-empty string")))
}

fn p2_optional_string(args: &Value, key: &str) -> Result<Option<String>, P2ActionError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if !value.is_empty() => Ok(Some(value.clone())),
        Some(_) => Err(P2ActionError::BadArgs(format!(
            "{key} must be a non-empty string"
        ))),
    }
}

fn p2_bool(args: &Value, key: &str, default: bool) -> Result<bool, P2ActionError> {
    match args.get(key) {
        None => Ok(default),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(P2ActionError::BadArgs(format!("{key} must be a boolean"))),
    }
}

fn p2_integer(
    args: &Value,
    key: &str,
    default: u64,
    minimum: u64,
    maximum: u64,
) -> Result<u64, P2ActionError> {
    let value = match args.get(key) {
        None => return Ok(default),
        Some(value) => value
            .as_u64()
            .ok_or_else(|| P2ActionError::BadArgs(format!("{key} must be an integer")))?,
    };
    if value < minimum || value > maximum {
        return Err(P2ActionError::BadArgs(format!(
            "{key} must be between {minimum} and {maximum}"
        )));
    }
    Ok(value)
}

fn p2_valid_url(url: &str) -> bool {
    if url == "about:blank" {
        return true;
    }
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return false;
    }
    if rest.is_empty()
        || rest
            .chars()
            .any(|value| value.is_whitespace() || value.is_control())
    {
        return false;
    }
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    !rest[..host_end].is_empty()
}

fn p2_page_args(tab_id: Option<&str>) -> Value {
    tab_id.map_or_else(|| json!({}), |tab_id| json!({ "tab_id": tab_id }))
}

fn p2_navigation_error(error: CdpError) -> P2ActionError {
    match error {
        CdpError::InvalidArgument(_) => P2ActionError::BadArgs("the argument is not valid".into()),
        CdpError::Timeout(_) => P2ActionError::Timeout("page navigation timed out".into()),
        CdpError::Transport(_) | CdpError::NotConnected => {
            P2ActionError::ProviderLost("browser connection is unavailable".into())
        }
        _ => P2ActionError::NavigationFailed("page navigation failed".into()),
    }
}

fn p2_error(error: CdpError) -> P2ActionError {
    match error {
        CdpError::InvalidArgument(_) => P2ActionError::BadArgs("the argument is not valid".into()),
        CdpError::InvalidEndpoint(_) => {
            P2ActionError::BadArgs("the browser endpoint is not usable".into())
        }
        CdpError::TargetNotFound(_) | CdpError::DialogNotFound => {
            P2ActionError::NotFound("the requested element or tab was not found".into())
        }
        CdpError::NotInteractable(_) => {
            P2ActionError::NotFound("the element cannot receive input".into())
        }
        CdpError::Timeout(_) => P2ActionError::Timeout("browser operation timed out".into()),
        CdpError::Transport(_) | CdpError::NotConnected => {
            P2ActionError::ProviderLost("browser connection is unavailable".into())
        }
        CdpError::Unsupported(_) => {
            P2ActionError::CapabilityUnavailable("the browser cannot perform this action".into())
        }
        CdpError::Protocol(_) | CdpError::Remote { .. } => {
            P2ActionError::Internal("browser operation failed".into())
        }
    }
}

fn p2_image_dimensions(format: &str, bytes: &[u8]) -> Option<(u32, u32)> {
    match format {
        "png" => p2_png_dimensions(bytes),
        "jpeg" => p2_jpeg_dimensions(bytes),
        _ => None,
    }
}

fn p2_png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || &bytes[0..8] != b"\x89PNG\r\n\x1a\n" || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    if width == 0 || height == 0 {
        return None;
    }
    Some((width, height))
}

fn p2_jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }
    let mut index = 2;
    while index + 4 <= bytes.len() {
        if bytes[index] != 0xFF {
            index += 1;
            continue;
        }
        let mut marker_index = index + 1;
        while marker_index < bytes.len() && bytes[marker_index] == 0xFF {
            marker_index += 1;
        }
        if marker_index >= bytes.len() {
            return None;
        }
        let marker = bytes[marker_index];
        index = marker_index + 1;
        if marker == 0xD8 || marker == 0xD9 || (0xD0..=0xD7).contains(&marker) {
            continue;
        }
        if index + 2 > bytes.len() {
            return None;
        }
        let length = u16::from_be_bytes([bytes[index], bytes[index + 1]]) as usize;
        if length < 2 || index + length > bytes.len() {
            return None;
        }
        if matches!(
            marker,
            0xC0 | 0xC1
                | 0xC2
                | 0xC3
                | 0xC5
                | 0xC6
                | 0xC7
                | 0xC9
                | 0xCA
                | 0xCB
                | 0xCD
                | 0xCE
                | 0xCF
        ) && index + 7 <= bytes.len()
        {
            let height = u16::from_be_bytes([bytes[index + 3], bytes[index + 4]]) as u32;
            let width = u16::from_be_bytes([bytes[index + 5], bytes[index + 6]]) as u32;
            if width > 0 && height > 0 {
                return Some((width, height));
            }
            return None;
        }
        index += length;
    }
    None
}

fn required_string(args: &Value, key: &str) -> Result<String, CdpError> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| CdpError::InvalidArgument(format!("{key} must be a non-empty string")))
}

fn required_string_allow_empty(args: &Value, key: &str) -> Result<String, CdpError> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| CdpError::InvalidArgument(format!("{key} must be a string")))
}

fn optional_string(args: &Value, key: &str) -> Option<String> {
    match args.get(key) {
        Some(Value::String(value)) if !value.is_empty() => Some(value.clone()),
        Some(Value::Number(value)) => Some(value.to_string()),
        _ => None,
    }
}

fn integer(args: &Value, key: &str, default: u64, minimum: u64, maximum: u64) -> u64 {
    args.get(key)
        .and_then(Value::as_u64)
        .unwrap_or(default)
        .clamp(minimum, maximum)
}

fn required_integer(args: &Value, key: &str, minimum: u64, maximum: u64) -> Result<u64, CdpError> {
    let value = args
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| CdpError::InvalidArgument(format!("{key} must be an integer")))?;
    if value < minimum || value > maximum {
        return Err(CdpError::InvalidArgument(format!(
            "{key} must be between {minimum} and {maximum}"
        )));
    }
    Ok(value)
}

fn timeout_ms(args: &Value, default: u64, maximum: u64) -> u64 {
    args.get("timeout_ms")
        .and_then(Value::as_u64)
        .unwrap_or(default)
        .clamp(1, maximum)
}

fn truncate(text: String, max_chars: usize) -> (String, bool) {
    if text.chars().count() <= max_chars {
        return (text, false);
    }
    let shortened: String = text.chars().take(max_chars).collect();
    (shortened, true)
}

fn js_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn pattern_matches(pattern: &str, url: &str) -> bool {
    let mut trimmed = pattern;
    if let Some(rest) = trimmed.strip_prefix('^') {
        trimmed = rest;
    }
    if let Some(rest) = trimmed.strip_suffix('$') {
        trimmed = rest;
    }
    let parts: Vec<String> = trimmed
        .split(".*")
        .filter(|part| !part.is_empty())
        .map(|part| part.replace("\\.", "."))
        .collect();
    if parts.is_empty() {
        return true;
    }
    let mut remaining = url;
    for part in parts {
        match remaining.find(&part) {
            Some(index) => remaining = &remaining[index + part.len()..],
            None => return false,
        }
    }
    true
}

const PAGE_LINKS_JS: &str = r##"(() => {
  const selector = __SELECTOR__;
  const max = __MAX__;
  const includeHidden = __HIDDEN__;
  const roots = [document];
  for (let index = 0; index < roots.length; index += 1) {
    const nodes = roots[index].querySelectorAll('*');
    for (const node of nodes) { if (node.shadowRoot) { roots.push(node.shadowRoot); } }
  }
  let scope = document;
  const links = [];
  const seen = new Set();
  let truncated = false;
  const hidden = (element) => {
    let node = element;
    while (node) {
      const style = getComputedStyle(node);
      if (style.display === 'none' || style.visibility === 'hidden' || Number(style.opacity) === 0) { return true; }
      node = node.parentElement || (node.getRootNode && node.getRootNode().host) || null;
    }
    const rect = element.getBoundingClientRect();
    return rect.width === 0 && rect.height === 0;
  };
  if (selector) {
    scope = document.querySelector(selector);
    if (!scope) { return { missing: true, url: location.href, links: [], truncated: false }; }
  }
  const scopes = [scope];
  if (scope.querySelectorAll) {
    for (let index = 0; index < scopes.length; index += 1) {
      const nodes = scopes[index].querySelectorAll('*');
      for (const node of nodes) { if (node.shadowRoot) { scopes.push(node.shadowRoot); } }
    }
  }
  for (const root of scopes) {
    for (const element of root.querySelectorAll('a[href], area[href]')) {
      if (!includeHidden && hidden(element)) { continue; }
      const raw = (element.getAttribute('href') || '').trim();
      if (!raw || raw === '#' || raw.toLowerCase().startsWith('javascript:')) { continue; }
      let resolved = '';
      try { resolved = new URL(raw, location.href).href; } catch (error) { continue; }
      if (!/^(https?|mailto|tel):/i.test(resolved)) { continue; }
      const text = (element.getAttribute('aria-label') || element.textContent || element.getAttribute('title') || '').trim().slice(0, 200);
      const key = resolved + '\n' + text;
      if (seen.has(key)) { continue; }
      seen.add(key);
      if (links.length >= max) { truncated = true; break; }
      const item = { text: text, url: resolved };
      const title = (element.getAttribute('title') || '').trim();
      if (title) { item.title = title; }
      const rel = (element.getAttribute('rel') || '').trim();
      if (rel) { item.rel = rel; }
      const target = (element.getAttribute('target') || '').trim();
      if (target) { item.target = target; }
      links.push(item);
    }
    if (truncated) { break; }
  }
  return { url: location.href, links: links, truncated: truncated };
})()"##;

const PAGE_FORMS_JS: &str = r##"(() => {
  const values = __VALUES__;
  const max = __MAX__;
  const selector = __SELECTOR__;
  const cssPath = (element) => {
    if (!element || element.nodeType !== 1) { return ''; }
    if (element.id) { return '#' + CSS.escape(element.id); }
    const parts = [];
    let node = element;
    while (node && node.nodeType === 1 && node.tagName.toLowerCase() !== 'html') {
      let part = node.tagName.toLowerCase();
      const parent = node.parentElement;
      if (parent) {
        let index = 1;
        for (const sibling of parent.children) {
          if (sibling === node) { break; }
          if (sibling.tagName === node.tagName) { index += 1; }
        }
        part += ':nth-of-type(' + index + ')';
      }
      parts.unshift(part);
      node = parent;
    }
    return parts.join(' > ');
  };
  const labelFor = (element) => {
    let text = '';
    if (element.labels && element.labels.length) {
      text = Array.from(element.labels).map((label) => label.innerText || '').join(' ');
    }
    if (!text) {
      const wrapping = element.closest ? element.closest('label') : null;
      if (wrapping) { text = wrapping.innerText || ''; }
    }
    if (!text) { text = element.getAttribute('aria-label') || ''; }
    if (!text) { text = element.getAttribute('placeholder') || ''; }
    return text.replace(/\s+/g, ' ').trim().slice(0, 120);
  };
  const roots = [document];
  for (let index = 0; index < roots.length; index += 1) {
    const nodes = roots[index].querySelectorAll('*');
    for (const node of nodes) { if (node.shadowRoot) { roots.push(node.shadowRoot); } }
  }
  let forms = [];
  if (selector) {
    const scope = document.querySelector(selector);
    if (!scope) { return { missing: true, url: location.href, forms: [], truncated: false }; }
    if (scope.tagName.toLowerCase() === 'form') { forms.push(scope); }
    for (const form of scope.querySelectorAll('form')) { forms.push(form); }
    for (const node of scope.querySelectorAll('*')) {
      if (node.shadowRoot) { for (const form of node.shadowRoot.querySelectorAll('form')) { forms.push(form); } }
    }
  } else {
    for (const root of roots) { for (const form of root.querySelectorAll('form')) { forms.push(form); } }
  }
  const output = [];
  let truncated = false;
  for (const form of forms) {
    if (output.length >= max) { truncated = true; break; }
    const fields = [];
    for (const element of form.querySelectorAll('input, select, textarea, button')) {
      const tag = element.tagName.toLowerCase();
      let type = (element.type || '').toLowerCase();
      if (tag === 'select') { type = 'select'; }
      if (tag === 'textarea') { type = 'textarea'; }
      if (tag === 'button' && !type) { type = 'button'; }
      const field = {
        name: element.name || '',
        type: type,
        selector: cssPath(element),
        required: !!element.required,
        disabled: !!element.disabled,
        readonly: !!element.readOnly,
      };
      const label = labelFor(element);
      if (label) { field.label = label; }
      if (type === 'checkbox' || type === 'radio') { field.checked = !!element.checked; }
      if (type === 'select') {
        field.options = Array.from(element.options || []).slice(0, 50).map((option) => ({
          value: option.value,
          label: (option.textContent || '').trim().slice(0, 120),
          selected: !!option.selected,
        }));
      }
      if (values && type !== 'password') {
        field.value = String(element.value === undefined ? '' : element.value).slice(0, 200);
      }
      fields.push(field);
    }
    let action = location.href;
    const rawAction = form.getAttribute('action');
    if (rawAction !== null && rawAction.trim() !== '') {
      try { action = new URL(rawAction, location.href).href; } catch (error) { action = location.href; }
    }
    let method = 'get';
    const rawMethod = form.getAttribute('method');
    if (rawMethod !== null && rawMethod.trim() !== '') { method = rawMethod.trim().toLowerCase(); }
    const item = {
      index: output.length,
      action: action,
      method: method,
      fields: fields,
    };
    const formName = form.getAttribute('name');
    const formId = form.getAttribute('id');
    if (formName) { item.name = formName; }
    if (formId) { item.id = formId; }
    output.push(item);
  }
  return { url: location.href, forms: output, truncated: truncated };
})()"##;

const STORAGE_GET_JS: &str = r##"(() => {
  try {
    const area = __AREA__;
    const storage = area === 'session' ? sessionStorage : localStorage;
    const keys = __KEYS__;
    const entries = [];
    const push = (key) => {
      const value = storage.getItem(key);
      if (value !== null) { entries.push({ key: key, value: value }); }
    };
    if (keys) {
      for (const key of keys) { push(key); }
    } else {
      for (let index = 0; index < storage.length; index += 1) {
        const key = storage.key(index);
        if (key !== null) { push(key); }
      }
    }
    return { origin: location.origin, entries: entries };
  } catch (error) {
    return { origin: location.origin, entries: [], error: String(error) };
  }
})()"##;

const STORAGE_SET_JS: &str = r##"(() => {
  try {
    const area = __AREA__;
    const storage = area === 'session' ? sessionStorage : localStorage;
    const entries = __ENTRIES__;
    const keys = Object.keys(entries);
    for (const key of keys) { storage.setItem(key, entries[key]); }
    return { origin: location.origin, ok: true, keys: keys };
  } catch (error) {
    return { origin: location.origin, ok: false, keys: [], error: String(error) };
  }
})()"##;

const STORAGE_CLEAR_JS: &str = r##"(() => {
  try {
    const area = __AREA__;
    const storage = area === 'session' ? sessionStorage : localStorage;
    const keys = __KEYS__;
    let cleared = 0;
    if (keys) {
      for (const key of keys) { if (storage.getItem(key) !== null) { cleared += 1; storage.removeItem(key); } }
    } else {
      cleared = storage.length;
      storage.clear();
    }
    return { origin: location.origin, cleared: cleared };
  } catch (error) {
    return { origin: location.origin, cleared: 0, error: String(error) };
  }
})()"##;

fn bounded_number(
    args: &Value,
    key: &str,
    default: f64,
    minimum: f64,
    maximum: f64,
) -> Result<f64, CdpError> {
    let value = args.get(key).and_then(Value::as_f64).unwrap_or(default);
    if !value.is_finite() || value < minimum || value > maximum {
        return Err(CdpError::InvalidArgument(format!(
            "{key} must be between {minimum} and {maximum}"
        )));
    }
    Ok(value)
}

fn cap_chars(text: &str, maximum: usize) -> String {
    text.chars().take(maximum).collect()
}

fn ax_string(node: &Value, key: &str) -> String {
    node.pointer(&format!("/{key}/value"))
        .and_then(Value::as_str)
        .map(|value| cap_chars(value, 200))
        .unwrap_or_default()
}

fn optional_string_array(args: &Value, key: &str) -> Option<Vec<String>> {
    args.get(key).and_then(Value::as_array).map(|values| {
        values
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect()
    })
}

fn storage_area(args: &Value) -> Result<String, CdpError> {
    let area = args
        .get("area")
        .and_then(Value::as_str)
        .unwrap_or("local")
        .to_string();
    if area != "local" && area != "session" {
        return Err(CdpError::InvalidArgument(
            "area must be local or session".into(),
        ));
    }
    Ok(area)
}

fn web_scheme(url: &str) -> Option<&'static str> {
    if url.starts_with("https://") {
        Some("https")
    } else if url.starts_with("http://") {
        Some("http")
    } else {
        None
    }
}

fn is_web_url(url: &str) -> bool {
    web_scheme(url).is_some()
}

fn parse_hex_color(color: &str) -> Result<(u8, u8, u8), CdpError> {
    if color.len() != 7 || !color.starts_with('#') {
        return Err(CdpError::InvalidArgument(
            "color must be a #rrggbb value".into(),
        ));
    }
    let red = u8::from_str_radix(&color[1..3], 16)
        .map_err(|_| CdpError::InvalidArgument("color must be a #rrggbb value".into()))?;
    let green = u8::from_str_radix(&color[3..5], 16)
        .map_err(|_| CdpError::InvalidArgument("color must be a #rrggbb value".into()))?;
    let blue = u8::from_str_radix(&color[5..7], 16)
        .map_err(|_| CdpError::InvalidArgument("color must be a #rrggbb value".into()))?;
    Ok((red, green, blue))
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn pdf_error(error: CdpError) -> CdpError {
    match error {
        CdpError::Unsupported(_) => {
            CdpError::Unsupported("this page cannot be printed to PDF".into())
        }
        other => other,
    }
}

fn accessibility_error(error: CdpError) -> CdpError {
    match error {
        CdpError::Unsupported(_) => {
            CdpError::Unsupported("the accessibility tree is unavailable".into())
        }
        other => other,
    }
}

fn cookie_error(error: CdpError) -> CdpError {
    match error {
        CdpError::Unsupported(_) => {
            CdpError::Unsupported("cookies are unavailable on this browser".into())
        }
        other => other,
    }
}

/// Parse a CDP element reference into a backend node id.
///
/// The extension uses @eN references, which have no meaning on the CDP
/// channel; callers get a typed argument error instead of a protocol failure.
fn screenshot_backend_id(reference: &str) -> Result<i64, CdpError> {
    if let Some(raw) = reference.strip_prefix("@c") {
        return raw.parse().map_err(|_| {
            CdpError::InvalidArgument(
                "CDP element references are @c<backendNodeId>, for example @c42".into(),
            )
        });
    }
    if reference.starts_with('@') {
        return Err(CdpError::InvalidArgument(
            "extension references such as @e12 are not CDP node ids; use @c<backendNodeId> from page_snapshot, or omit tab_id and use the extension channel".into(),
        ));
    }
    Err(CdpError::InvalidArgument(
        "browser_screenshot element ref must be @c<backendNodeId> from page_snapshot".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_has_the_expected_tools() {
        assert_eq!(CDP_TOOL_NAMES.len(), 46);
        assert!(is_cdp_tool("browser_navigate"));
        assert!(is_cdp_tool("cdp_send"));
        assert!(is_cdp_tool("page_snapshot"));
        assert!(is_cdp_tool("browser_select"));
        assert!(!is_cdp_tool("browser_channel"));
    }

    #[test]
    fn page_forms_returns_strings_for_named_controls() {
        use std::process::{Command, Stdio};

        let expression = PAGE_FORMS_JS
            .replace("__VALUES__", "false")
            .replace("__MAX__", "20")
            .replace("__SELECTOR__", "null");
        let dir = std::env::temp_dir().join(format!("corvus-page-forms-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create page_forms temp dir");
        let harness_path = dir.join("page_forms_named_controls.mjs");
        let expression_path = dir.join("page_forms.js");
        std::fs::write(
            &harness_path,
            include_str!("../tests/page_forms_named_controls.mjs"),
        )
        .expect("write page_forms harness");
        std::fs::write(&expression_path, expression.as_bytes()).expect("write PAGE_FORMS_JS");
        let child = Command::new("node")
            .arg(&harness_path)
            .arg(&expression_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        let child = match child {
            Ok(child) => child,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let _ = std::fs::remove_dir_all(&dir);
                eprintln!("node is unavailable; skipped the PAGE_FORMS_JS harness");
                return;
            }
            Err(error) => {
                let _ = std::fs::remove_dir_all(&dir);
                panic!("failed to start node: {error}");
            }
        };
        let output = child.wait_with_output().expect("node output");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            output.status.success(),
            "page_forms regression failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("page_forms named controls: ok"),
            "the node harness did not report success: stdout={:?} stderr={:?}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn truncation_respects_character_boundaries() {
        let (text, truncated) = truncate("héllo".into(), 2);
        assert_eq!(text, "hé");
        assert!(truncated);
        assert!(!truncate("short".into(), 32).1);
    }

    #[test]
    fn only_context_loss_is_transient() {
        assert!(is_transient_context_error(&CdpError::Remote {
            code: -32000,
            message: "Execution context was destroyed.".into(),
        }));
        assert!(is_transient_context_error(&CdpError::Remote {
            code: -32000,
            message: "Cannot find context with specified id".into(),
        }));
        assert!(!is_transient_context_error(&CdpError::Transport(
            "connection reset".into()
        )));
        assert!(!is_transient_context_error(&CdpError::Timeout(1000)));
        assert!(!is_transient_context_error(&CdpError::NotConnected));
    }

    #[test]
    fn only_absent_dialog_is_typed_not_found() {
        assert!(matches!(
            dialog_error(CdpError::Remote {
                code: -32602,
                message: "No dialog is showing".into(),
            }),
            CdpError::DialogNotFound
        ));
        assert!(matches!(
            dialog_error(CdpError::Remote {
                code: -32602,
                message: "Invalid parameters".into(),
            }),
            CdpError::Remote { .. }
        ));
        assert!(matches!(
            dialog_error(CdpError::Transport("closed".into())),
            CdpError::Transport(_)
        ));
    }

    #[test]
    fn simple_patterns_match_urls() {
        assert!(pattern_matches("/api/", "https://example.test/api/items"));
        assert!(pattern_matches(
            ".*\\.json",
            "https://example.test/data.json"
        ));
        assert!(!pattern_matches(
            "/admin/",
            "https://example.test/api/items"
        ));
    }

    #[test]
    fn frame_paths_follow_document_order() {
        let tree = json!({
            "frameTree": {
                "frame": { "id": "TOP", "url": "https://example.test/" },
                "childFrames": [
                    {
                        "frame": { "id": "F0", "url": "https://example.test/frame" },
                        "childFrames": [
                            { "frame": { "id": "F00", "url": "https://example.test/nested" } }
                        ]
                    },
                    { "frame": { "id": "F1", "url": "https://other.test/" } }
                ]
            }
        });
        let nodes = flatten_frame_tree(&tree);
        let paths: Vec<&str> = nodes.iter().map(|node| node.path.as_str()).collect();
        assert_eq!(paths, vec!["", "f0", "f0.0", "f1"]);
        assert_eq!(nodes[2].frame_id, "F00");
        assert_eq!(nodes[3].url, "https://other.test/");
    }

    #[test]
    fn frame_summary_counts_only_child_documents() {
        let tree = json!({
            "frameTree": {
                "frame": { "id": "TOP", "url": "https://example.test/" },
                "childFrames": [
                    {
                        "frame": { "id": "F0", "url": "https://example.test/frame" },
                        "childFrames": [
                            { "frame": { "id": "F00", "url": "https://example.test/nested" } }
                        ]
                    },
                    { "frame": { "id": "F1", "url": "https://other.test/" } }
                ]
            }
        });
        let nodes = flatten_frame_tree(&tree);
        let collected = vec![frame_data(""), frame_data("f0"), frame_data("f0.0")];
        let summary = frames_summary(
            &nodes,
            &collected,
            vec![json!({
                "frame": "f1",
                "url": "https://other.test/",
                "reason": "cross_origin",
            })],
        )
        .expect("child frames");
        assert_eq!(summary.get("total").and_then(Value::as_u64), Some(3));
        assert_eq!(summary.get("included").and_then(Value::as_u64), Some(2));
        assert_eq!(
            summary
                .get("skipped")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(1)
        );
    }

    fn frame_data(path: &str) -> FrameData {
        FrameData {
            path: path.to_string(),
            ax: json!({ "nodes": [] }),
            document_index: None,
            origin: (0.0, 0.0),
            scroll: (0.0, 0.0),
        }
    }

    #[test]
    fn frame_summary_is_omitted_without_child_documents() {
        let tree = json!({
            "frameTree": { "frame": { "id": "TOP", "url": "https://example.test/" } }
        });
        let nodes = flatten_frame_tree(&tree);
        assert!(frames_summary(&nodes, &[frame_data("")], Vec::new()).is_none());
        assert!(frames_summary(&[], &[], Vec::new()).is_none());
    }

    #[test]
    fn frame_summary_omits_descendants_of_skipped_frames() {
        let tree = json!({
            "frameTree": {
                "frame": { "id": "TOP", "url": "https://example.test/" },
                "childFrames": [
                    {
                        "frame": { "id": "F0", "url": "https://other.test/" },
                        "childFrames": [
                            { "frame": { "id": "F00", "url": "https://other.test/nested" } }
                        ]
                    }
                ]
            }
        });
        let nodes = flatten_frame_tree(&tree);
        let summary = frames_summary(
            &nodes,
            &[frame_data("")],
            vec![json!({
                "frame": "f0",
                "url": "https://other.test/",
                "reason": "cross_origin",
            })],
        )
        .expect("one child");
        assert_eq!(summary.get("total").and_then(Value::as_u64), Some(1));
        assert_eq!(summary.get("included").and_then(Value::as_u64), Some(0));
    }

    #[test]
    fn cross_origin_frames_are_identified_for_skipping() {
        assert!(same_origin(
            "https://example.test/top",
            "https://example.test/frame"
        ));
        assert!(same_origin(
            "https://EXAMPLE.test/",
            "https://example.test/frame"
        ));
        assert!(!same_origin(
            "https://example.test/",
            "https://other.test/frame"
        ));
        assert!(!same_origin(
            "http://example.test/",
            "https://example.test/"
        ));
        assert!(!same_origin(
            "https://example.test:8443/",
            "https://example.test/"
        ));
        assert!(same_origin("about:blank", "https://example.test/"));
    }

    #[test]
    fn screenshot_references_are_cdp_backend_ids_only() {
        assert_eq!(screenshot_backend_id("@c42").expect("cdp ref"), 42);
        let extension = screenshot_backend_id("@e12").expect_err("extension ref");
        assert!(matches!(extension, CdpError::InvalidArgument(_)));
        assert!(extension.to_string().contains("@c<backendNodeId>"));
        assert!(matches!(
            screenshot_backend_id("#card"),
            Err(CdpError::InvalidArgument(_))
        ));
        assert!(matches!(
            screenshot_backend_id("@cnot-a-number"),
            Err(CdpError::InvalidArgument(_))
        ));
    }

    #[test]
    fn snapshot_documents_decode_root_strings_and_scroll() {
        let strings = json!([
            "",
            "https://example.test/",
            "https://example.test/frame",
            "FRAME-ID"
        ]);
        let documents = json!([
            {
                "frameId": 3,
                "scrollOffsetX": 12.5,
                "scrollOffsetY": 34.5,
                "nodes": { "backendNodeId": [], "nodeName": [] }
            }
        ]);
        let documents = documents.as_array().expect("documents");
        let strings = strings.as_array().expect("strings");
        assert_eq!(
            snapshot_document_index(documents, strings, "FRAME-ID"),
            Some(0)
        );
        assert_eq!(snapshot_document_index(documents, strings, "MISSING"), None);
        assert_eq!(document_scroll(&documents[0]), (12.5, 34.5));
    }

    #[test]
    fn element_screenshots_use_the_snapshot_border_box_for_top_shadow_and_iframes() {
        let cases = [
            (
                "top padded button",
                [
                    8.0, 95.875, 71.734375, 95.875, 71.734375, 110.875, 8.0, 110.875,
                ],
                [0.0, 93.0, 80.0, 93.0, 80.0, 114.0, 0.0, 114.0],
                (0.0, 0.0),
                (0.0, 93.0, 80.0, 21.0),
            ),
            (
                "open shadow button",
                [
                    8.0, 120.875, 98.421875, 120.875, 98.421875, 135.875, 8.0, 135.875,
                ],
                [0.0, 118.0, 106.0, 118.0, 106.0, 139.0, 0.0, 139.0],
                (0.0, 0.0),
                (0.0, 118.0, 106.0, 21.0),
            ),
            (
                "same-origin iframe button",
                [
                    9.0, -207.3125, 81.625, -207.3125, 81.625, -192.3125, 9.0, -192.3125,
                ],
                [1.0, -210.0, 90.0, -210.0, 90.0, -189.0, 1.0, -189.0],
                (0.0, 429.0),
                (1.0, 219.0, 89.0, 21.0),
            ),
        ];
        for (name, content, border, scroll, expected) in cases {
            let model = json!({ "border": border, "content": content });
            let clip = CdpClient::element_clip_from_box_model(&model, "@c239", scroll.0, scroll.1)
                .unwrap_or_else(|error| panic!("{name} clip failed: {error}"));
            assert_eq!(clip, expected, "{name} must match the snapshot border bbox");
        }
    }

    #[test]
    fn element_screenshots_reject_a_missing_border_box() {
        let model = json!({
            "content": [8.0, 95.875, 71.734375, 95.875, 71.734375, 110.875, 8.0, 110.875]
        });
        let error = CdpClient::element_clip_from_box_model(&model, "@c239", 0.0, 0.0)
            .expect_err("content box alone must not be accepted");
        assert!(matches!(error, CdpError::TargetNotFound(ref target) if target == "@c239"));
    }
}
