# CDP and the hybrid transport

[English](07-cdp.md) | [中文](07-cdp.zh.md)

Status: 1.6.0 is the current published release, released on 2026-11-15; 1.5.0 is previous. Page snapshots traverse open shadow DOM and same-origin child frames, and compact diffs are computed by the daemon. The 1.6.0 release adds daemon guardrails that run before CDP dispatch. The 1.7.0 working tree adds `browser_screenshot` as the 44th MCP tool and the 34th CDP browser-facing tool; its QA, CI and release evidence is Pending. QA PASS for 1.6.0 is recorded in [v1.6.0.md](../tests/acceptance/v1.6.0.md). Audience: operators and developers who want browser-level CDP control in addition to the extension transport.

Corvus 1.6.0 can drive a Chromium browser through the Chrome DevTools Protocol (CDP) directly from the Rust daemon, or through the Manifest V3 extension as before. The extension remains the zero-setup path and the default for user browsers. CDP is an opt-in path for a browser that is already listening on a loopback debugging port; in the 1.7.0 tree it covers all 34 browser-facing tools, including compact snapshots, `@c` element references and screenshots.

## 1. Two channels, one tool surface

The MCP surface is 44 tools in the 1.7.0 tree; the published 1.6.0 release had 43. The channel decides how browser-facing calls travel; the original 32 result shapes remain available, alongside the compact snapshot and batch output introduced in 1.3.0.

| Channel | Transport | Use it when |
|---|---|---|
| `extension` | WebSocket to the extension, then `chrome.*` and content scripts | You want the default zero-setup path, `@eN` DOM references, or the extension's 26 browser-facing tools. CDP is ignored. |
| `cdp` | One long-lived browser-level WebSocket from `corvusd` to the browser | The browser was started with `--remote-debugging-port` and you want all 34 browser-facing tools, including `page_snapshot` with `@c` references and screenshots. Strict: no fallback. |
| `auto` | CDP when connected, otherwise the extension | The default. It prefers CDP and falls back to the extension only when the CDP call reports `Unsupported`; transport and protocol errors are not retried on the other channel. |

`browser_screenshot` follows the same split: CDP accepts `@c<backendNodeId>` element references and supports viewport, full_page and element captures; the extension accepts `@e<number>` references and can fall back to `chrome.tabs.captureVisibleTab` for an active-tab PNG viewport. The wrong reference kind returns `BAD_ARGS`. Screenshot pixels are not redacted or boundary-wrapped.

`auto` is deliberately conservative. If a CDP call fails because the browser is gone or the WebSocket is down, the call fails with a typed error instead of silently replaying the action through a different browser. Only a genuine "this operation has no CDP equivalent" result falls back.

## 2. Turn on CDP in a Chromium browser

CDP is not enabled in a normal browser launch. Start a dedicated Chromium instance with a remote debugging port and a separate profile:

```bash
# Chrome
google-chrome --remote-debugging-port=9222 --user-data-dir=/tmp/corvus-cdp-profile

# Edge
microsoft-edge --remote-debugging-port=9222 --user-data-dir=/tmp/corvus-cdp-profile

# Brave
brave-browser --remote-debugging-port=9222 --user-data-dir=/tmp/corvus-cdp-profile
```

On Windows the executable is `chrome.exe`, `msedge.exe` or `brave.exe`; on macOS use the application binary or `open -na "Google Chrome" --args ...`. Recent Chrome builds can refuse remote debugging on the default profile, so a dedicated `--user-data-dir` is the reliable choice.

Now confirm the browser-level WebSocket URL:

```bash
curl -s http://127.0.0.1:9222/json/version
```

The response contains `webSocketDebuggerUrl`, for example `ws://127.0.0.1:9222/devtools/browser/6f...`. That value is the endpoint the daemon accepts. The HTTP discovery URL itself is not a valid daemon endpoint.

If the browser rejects the WebSocket with an origin error, add `--remote-allow-origins` only for the origin that is being rejected, for example:

```bash
google-chrome --remote-debugging-port=9222 --user-data-dir=/tmp/corvus-cdp-profile \
  --remote-allow-origins=http://127.0.0.1:9222
```

Use `--remote-allow-origins=*` only on an isolated test machine and only while the port is loopback-only. Do not widen the port or the origin allow-list on a machine that holds real accounts.

## 3. Let the extension discover the endpoint

The extension options page stores a CDP endpoint separately from the daemon WebSocket URL, under `corvus.cdpEndpoint`.

| Value | Behaviour |
|---|---|
| `auto` (default) | Probe ports 9222, 9223 and 9333 on `127.0.0.1`, read `/json/version`, and choose an available endpoint whose browser family matches this extension's browser. |
| `http://127.0.0.1:9222` | Probe only this loopback endpoint. The scheme must be `http://`; the host must be `127.0.0.1`, `localhost` or `[::1]`; a numeric port is required; paths, queries and credentials are rejected. |

Auto discovery probes all three ports in parallel with an 800 ms timeout. It prefers an exact family match, then Chrome/Chromium cross-match, then port order. A mismatch is not fatal when the endpoint was configured explicitly, but `auto` rejects a descriptor whose browser family does not match. Chrome and Chromium are one family; Edge, Brave, Opera and Vivaldi are distinct families.

The handshake carries a CDP descriptor with the endpoint, WebSocket debugger URL, browser, protocol version, source and family. The extension advertises the `cdp` capability only when its probe actually found an available endpoint. The daemon re-validates every descriptor and accepts only loopback `ws://` or `wss://` debugger URLs.

## 4. Daemon flags and environment

`corvusd serve` and `corvus serve` accept:

| Flag | Environment | Meaning |
|---|---|---|
| `--cdp-endpoint <ws-url>` | `CORVUS_CDP_ENDPOINT` | Connect to this browser-level WebSocket URL at startup. It must be a loopback `ws://` or `wss://` URL. |
| `--no-cdp` | — | Disable CDP entirely. `auto` then uses the extension; forcing `cdp` fails. |
| `--channel auto|cdp|extension` | `CORVUS_CHANNEL` | Set the startup routing mode. The default is `auto`. |

Examples:

```bash
# Let the extension's descriptor select the endpoint.
corvusd serve --home /opt/corvus --channel auto

# Connect directly; no extension needed for CDP-capable tools.
corvusd serve --home /opt/corvus \
  --cdp-endpoint "ws://127.0.0.1:9222/devtools/browser/6f3a..."

# Serve CDP-capable tools but always use the extension.
corvusd serve --home /opt/corvus --channel extension
```

`corvusd doctor` accepts the same three CDP options for command compatibility, but the local inspection it prints does not connect to the browser. Use `browser_channel` and `cdp_status` over MCP to check a running daemon.

## 5. Inspect and switch the channel

`browser_channel` with no arguments returns the channel status document:

```json
{ "action": "status" }
```

The result includes `channel`, `cdp_enabled`, the CDP status, extension connection and browser ids, the CDP tool names, the last extension descriptor, and `descriptor_error` or `browser_mismatch` when applicable.

Switch at runtime:

```json
{ "action": "set", "channel": "cdp" }
```

`browser_channel set` changes the running daemon only; the next daemon start uses `--channel` or `CORVUS_CHANNEL` again. Setting `cdp` while the daemon was started with `--no-cdp` fails with a typed error. Switching modes does not restart the browser and does not clear the CDP event ring.

## 6. Status and targets

`cdp_status` reports the connection and its counters:

```json
{}
```

The result extends the core status with `event_capacity` (1000), `event_count` and the list of CDP tool names. A disconnected client still answers; read `last_error` and `reconnects` before assuming the endpoint is healthy.

`cdp_targets` lists page targets known to the client:

```json
{}
```

Each target has `target_id`, `target_type`, `title`, `url`, `attached`, an optional `session_id`, and an optional `status`. Target and session identifiers belong to the current connection; after a reconnect, list targets again instead of reusing stale ids.

## 7. Raw CDP tools

`cdp_send` sends one protocol command:

```json
{ "method": "Runtime.evaluate", "params": { "expression": "document.title" }, "target_id": "<target-id>" }
```

| Argument | Meaning |
|---|---|
| `method` | Required protocol method, for example `Runtime.evaluate` or `Page.navigate`. |
| `params` | Optional object, default `{}`. |
| `session_id` | Optional flattened session id. It wins over `target_id`. |
| `target_id` | Optional page target. It is attached on demand when no session is given. |
| `timeout_ms` | 1..60000, default 10000. |

The result is `{ "method": "...", "result": ... }`. Without a session or target, Corvus uses the active page target; when no page target exists the call fails with `CAPABILITY_UNAVAILABLE`.

`cdp_events` reads the bounded event ring:

```json
{ "since_seq": 0, "limit": 100, "method": "Runtime.consoleAPICalled", "wait_ms": 1000 }
```

| Argument | Meaning |
|---|---|
| `since_seq` | Return events after this sequence, default 0. |
| `limit` | 1..500, default 100. |
| `method` | Optional exact method filter. |
| `wait_ms` | 0..30000, default 0. When greater than zero, wait for an event before answering. |
| `clear` | Drop retained events after reading, default false. |

The ring holds 1000 events. `next_seq` is the cursor for the next read and `dropped` counts events evicted before a reader saw them. Event sequence numbers are monotonic for the life of the daemon, not for the life of one connection.

### Snapshots and references over CDP

`page_snapshot` works on both channels. Over CDP its compact output uses `@c<backendNodeId>` element references and the same line grammar as the extension channel:

```text
@c42 button "Save" [focused] bbox=40,120,96,32
@c43 textbox "Name" value="Ada"
# "Hello world"
```

`format: "json"` returns node objects instead of text; `viewport_only`, `include_bbox` and `max_nodes` behave as documented in [09-tools.md](09-tools.md). A reference is valid only for the snapshot that produced it; take a new snapshot after navigation or DOM changes.

On the CDP channel the snapshot also traverses open shadow DOM and same-origin child frames. The top document has no frame path; child frames use document-order paths such as `f0`, `f0.0` and `f1`. Content inside a child frame appends `frame="<path>"` to its compact line, and a JSON node carries `frame` only when it is non-empty. When the page has child frames, the result adds `frames: {total, included, skipped: [{frame, url, reason}]}`; `reason` is `cross_origin`, `unavailable` or `geometry_unavailable`. Cross-origin frame DOM is never read, `max_nodes` is one budget across every included document, and bounding boxes are projected into the top viewport. Compact diffs are computed by the daemon application layer, not by CDP; see [09-tools.md](09-tools.md) for the diff contract.

`@c` references are CDP-only and `@eN` references are extension-only. Passing one to the other channel returns typed `BAD_ARGS`; omit `tab_id` to act on the active target. The eight former extension-only tools now accept `@c` references under forced `cdp`; extension-only availability is zero in 1.6.0.

### Tab identifiers across channels

Tab identifiers are not interchangeable in 1.6.0. The extension uses numeric tab ids (a number or a numeric string such as `7`); CDP uses the 32-character hexadecimal `target_id` from `cdp_targets`. The daemon does not translate one namespace into the other.

When a numeric extension id is passed to a tool routed to CDP, the call fails with typed `BAD_ARGS`; `details.hint` is `omit tab_id to use the active tab; extension tab ids are not CDP target ids`. A 32-hex CDP target id passed to a tool routed to the extension fails the same way; `details.hint` is `omit tab_id to use the active tab; CDP target ids are not extension tab ids`. `details.tab_id` echoes the rejected value. Use an id from the active channel, or omit `tab_id` to act on the active tab. The namespaces remain separate in 1.6.0; a full bridge is deferred to a later release.

`cdp_targets` is the authoritative list for CDP target ids; `tab_list` returns the ids of whichever channel serves that call.

## 8. High-level CDP-backed tools

| Tool | Purpose |
|---|---|
| `browser_console` | Enables Runtime, waits briefly, and returns recent console or exception entries, newest last. Arguments: `tab_id?`, `limit?` 1..500 default 100. |
| `browser_cookies` | With no `urls`, reads browser-wide cookies through `Storage.getCookies`; with up to 20 `urls`, reads page cookies through `Network.getCookies`. |
| `browser_dialog` | Answers a JavaScript dialog with `Page.handleJavaScriptDialog`. Arguments: `tab_id?`, `accept?` default true, `prompt_text?`. |
| `browser_emulate` | Applies or resets device metrics, network conditions, user agent and timezone. `reset: true` clears them. |

`browser_emulate` needs `width` and `height` together for device metrics. Network fields are `offline`, `latency_ms`, `download_throughput` and `upload_throughput`; throughput `-1` disables the corresponding limit. The result lists what was applied.

### Events, downloads and dialogs

On every CDP connection, Corvus calls `Browser.setDownloadBehavior` with `behavior: default` and `eventsEnabled: true` before it serves commands, so download events can feed `download_list` and `download_wait`. If the browser rejects that command, Corvus records a warning and download tools cannot observe new events on that connection; every other CDP tool remains usable.

`Page`, `Runtime` and `Log` are enabled per attach, after `Target.attachToTarget` returns a session, with a five-second cap and best-effort failure handling. The event ring therefore contains only events produced after that attach; it does not backfill earlier history. After a reconnect, attach the page again and the daemon enables the three domains for the new session.

`browser_dialog` with no open dialog returns typed `NOT_FOUND` with the message `no dialog is showing`. Treat it as a state check, not as a transport failure.

## 9. Reconnect, heartbeat and request lifetime

The daemon keeps one browser-level WebSocket. Requests are correlated by numeric id; page work uses flattened `Target.attachToTarget` sessions. Reconnects back off from 250 ms to 10 s, a heartbeat runs every 15 s, and the default request timeout is 10 s with a 60 s maximum. Pending requests fail when the socket drops; they are not replayed automatically. `cdp_status.reconnects` increments on each successful reconnection.

Because the connection is stateful, do not cache `session_id` or `target_id` across a reconnect. Re-read `cdp_targets` and attach again. The event ring survives reconnects inside the daemon process, but the old attached sessions do not. Attach the page again; each new attach enables `Page`, `Runtime` and `Log` for that session, so only events after the new attach are visible.

## 10. Tool availability by channel

| Group | Count | Tools |
|---|---|---|
| Both channels | 25 | `browser_navigate`, `page_snapshot`, `page_text`, `page_html`, `page_find`, `browser_wait`, `browser_history`, `browser_click`, `browser_type`, `browser_hover`, `browser_press`, `browser_scroll`, `browser_select`, `browser_eval`, `tab_list`, `tab_new`, `tab_close`, `tab_switch`, `net_capture`, `net_log`, `net_clear`, `net_intercept`, `upload_file`, `download_list`, `download_wait` |
| CDP-only | 8 | `cdp_status`, `cdp_targets`, `cdp_send`, `cdp_events`, `browser_console`, `browser_cookies`, `browser_dialog`, `browser_emulate` |
| Extension-only | 0 | None in 1.6.0. |
| Daemon-local | 10 | `browser_list`, `browser_channel`, `tool_help`, `browser_batch`, `session_list`, `session_steps`, `session_replay`, `session_export`, `metrics`, `logs_tail` |

The counts add up to 43. The daemon-local tools never use either browser channel. CDP covers all 33 browser-facing tools; the eight CDP-only tools remain unavailable to the extension. In forced `cdp` mode every browser-facing tool is accepted; in forced `extension` mode a CDP-only tool fails with `CAPABILITY_UNAVAILABLE`. `auto` is the only mode that may move a call between channels, and it does so only for an `Unsupported` CDP result.

## 11. Troubleshooting

| Symptom | Check | Action |
|---|---|---|
| No endpoint found on 9222, 9223 or 9333 | `curl -s http://127.0.0.1:9222/json/version` | Start the browser with `--remote-debugging-port`; confirm the port is listening on loopback only. |
| Endpoint found but browser family mismatch | `browser_channel` status shows `browser_mismatch` | The selected endpoint belongs to another browser family. Use the matching browser, or set the extension endpoint explicitly only when the mismatch is intended. |
| Remote debugging rejected | Browser stderr contains an origin rejection | Restart the browser with `--remote-allow-origins` for the exact origin. Do not widen the listening address. |
| WebSocket URL returns 404 | The value came from `/json/list` instead of `/json/version` | Use the browser-level `webSocketDebuggerUrl` from `/json/version`. |
| DevTools and Corvus conflict on a tab | Attach errors or `CAPABILITY_UNAVAILABLE` while DevTools is open | Close DevTools for that tab, or test with a dedicated browser profile. Tab-level debugger attach is exclusive. |
| `browser_channel set cdp` fails | `cdp_enabled` is false | Restart without `--no-cdp`, then set the channel again. |
| CDP-only tool fails under `extension` | Forced `extension` mode has no CDP access | Switch to `auto` or `cdp`, or use the extension's 26 browser-facing tools. `page_snapshot` is no longer extension-only since 1.4.0. |
| Everything works, then stops after a browser restart | `cdp_status.connected` is false; `reconnects` stopped increasing | The restart changed the browser's WebSocket URL. When the endpoint came from the extension descriptor, the daemon re-polls every 500 ms, selects the replacement endpoint and reconnects automatically; `cdp_status.reconnects` then increases and no daemon restart is needed. A URL pinned with `--cdp-endpoint` is not rediscovered; update it only if the browser's printed `webSocketDebuggerUrl` changed. |
| Configured CDP endpoint changed at runtime | The extension endpoint was edited while the daemon was already connected | Auto-discovered endpoints are re-polled every 500 ms and reconnect; a configured endpoint change may need a daemon restart to take effect. Update the URL, restart only when required, and confirm the new endpoint in `cdp_status`. |

## 12. Security and limits

CDP is not a sandbox. A browser-level endpoint can read cookies, inject script, navigate, intercept traffic and control every profile loaded in that browser instance. Corvus adds no authentication in front of it. The endpoint validator accepts only loopback hosts and only `ws://` or `wss://`, but any local process that can reach the debugging port has the same power.

- Never expose ports 9222, 9223, 9333, 7330 or 7331 beyond loopback.
- Use a dedicated profile for CDP work; do not attach a test harness to the everyday browser profile with live accounts.
- Treat the CDP event ring, console output and cookie results as sensitive data.
- The daemon does not launch a browser and does not install one. Docker and Kubernetes cannot reach the host's loopback interface by default and the shipped image contains no browser; see [05-deployment.md](05-deployment.md#7-ports-exposure-and-the-authentication-boundary) for that limitation.
- The extension remains the zero-setup path and serves 26 browser-facing tools with `@eN` references. CDP serves all 34 browser-facing tools with `@c` references; neither channel is a fallback for an arbitrary failure.

## 13. Test policy

Do not drive the user's normal Edge or Chrome window with automated tests. Launch a dedicated browser with a separate `--user-data-dir`, throwaway ports and no signed-in accounts. This is a test policy, not a product restriction: the daemon will drive any loopback CDP endpoint it is given, and the operator is responsible for choosing that endpoint.

For install and upgrade steps see [04-install.md](04-install.md). For container and Kubernetes limits see [05-deployment.md](05-deployment.md). For the architecture of the channel router see [02-architecture.md](02-architecture.md#56-hybrid-cdp-transport). For the complete tool catalogue and compact grammar see [09-tools.md](09-tools.md).
