//! Tool handlers executed over the browser-level CDP connection.
//!
//! Every handler preserves the result shape of the extension-backed tool it
//! replaces. CDP-only failures use the shared error codes plus a
//! transport:"cdp" detail block.

use std::collections::HashMap;
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
];

/// Whether this tool can be answered over CDP.
pub fn is_cdp_tool(name: &str) -> bool {
    CDP_TOOL_NAMES.contains(&name)
}

impl CdpClient {
    /// Execute one CDP-backed tool.
    pub async fn call_tool(&self, name: &str, args: &Value) -> Result<Value, CdpError> {
        if !is_cdp_tool(name) {
            return Err(CdpError::Unsupported(format!("{name} is not a CDP tool")));
        }
        match name {
            "browser_navigate" => self.navigate_tool(args).await,
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
                json!({ "url": url, "background": !active }),
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
        self.remember_page_target(&target_id, &url);
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
        Ok(json!({
            "tab_id": target_id,
            "url": url,
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
        let (ax, dom, page) = self.snapshot_inputs(&session).await?;
        let snapshot = snapshot::render(&ax, &dom, &page, &options)?;
        let viewport = page.viewport.as_json();
        match format {
            Format::Compact => Ok(json!({
                "format": "compact",
                "tab_id": target_id,
                "url": page.url,
                "title": page.title,
                "viewport": viewport,
                "mode": mode.as_str(),
                "node_count": snapshot.node_count,
                "truncated": snapshot.truncated,
                "text": snapshot.text,
            })),
            Format::Json => Ok(json!({
                "format": "json",
                "tab_id": target_id,
                "url": page.url,
                "title": page.title,
                "viewport": viewport,
                "mode": mode.as_str(),
                "node_count": snapshot.node_count,
                "truncated": snapshot.truncated,
                "nodes": snapshot.nodes,
            })),
        }
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

    async fn snapshot_inputs(&self, session: &str) -> Result<(Value, Value, PageInfo), CdpError> {
        let ax = self
            .send(
                "Accessibility.getFullAXTree",
                json!({}),
                Some(session),
                None,
                Some(15_000),
            )
            .await?;
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
        Ok((ax, dom, page))
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

    fn target_for_session(&self, session: Option<&str>) -> Option<String> {
        let session = session?;
        self.targets()
            .into_iter()
            .find(|target| target.session_id.as_deref() == Some(session))
            .map(|target| target.target_id)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_has_the_expected_tools() {
        assert_eq!(CDP_TOOL_NAMES.len(), 33);
        assert!(is_cdp_tool("browser_navigate"));
        assert!(is_cdp_tool("cdp_send"));
        assert!(is_cdp_tool("page_snapshot"));
        assert!(is_cdp_tool("browser_select"));
        assert!(!is_cdp_tool("browser_channel"));
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
}
