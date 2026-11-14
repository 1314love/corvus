# Corvus Architecture

[English](02-architecture.md) | [中文](02-architecture.zh.md)

Status: M1 implemented; M2 in progress. Audience: engineers.

## 1. Constraints that shape the design

1. **Must drive the browser the user is already using.** No relaunch, no dedicated profile, no remote-debugging flag. This forbids a CDP-only design: the extension is the only way in.
2. **Zero approvals, zero pairing, zero pages.** Any flow that needs a human click is a defect.
3. **Connection is the product.** MV3 service workers are killed after ~30s idle; the design must assume the browser side is hostile to long-lived sockets.
4. **Multi-browser, multi-profile, concurrent.** One daemon, N extensions, independent sessions.
5. **Everything observable.** Every command and its page context must be replayable after the fact.
6. **Single static binary for the daemon.** No runtime dependency, no installer prerequisites.

## 2. Containers

    agent client (Claude Code / Cursor / DSH / Cline / Copilot)
        |  MCP over stdio or streamable-http (JSON-RPC 2.0)
        v
    +--------------------------------------------------------------+
    | corvusd                                                      |
    |  mcp-server  -> tool registry -> router -> session runtime    |
    |  transport hub (ws / native-messaging / long-poll)            |
    |  recorder (sqlite + jsonl + trace)                            |
    |  site memory (sqlite)   metrics (prometheus)   health         |
    +--------------------------------------------------------------+
        |  framed JSON envelope over WS (primary)
        v
    extension (MV3): service worker + content script + offscreen keepalive
        |  chrome.tabs / scripting / debugger / webRequest / downloads
        v
    browser

Processes: one corvusd per user session (service on Win/macOS/Linux), one extension per browser profile, one CLI client (corvus).

## 3. Crates

| crate | role |
|---|---|
| corvus-proto | wire types: envelope, command, response, error taxonomy, versioning |
| corvus-mcp | MCP server: tool registry, JSON-RPC, stdio + streamable-http transports |
| corvus-transport | WS server, native-messaging host, long-poll fallback, browser registry |
| corvus-runtime | command router, per-browser queues, timeouts, idempotency, cancellation |
| corvus-record | session recorder, sqlite schema, trace writer, replay engine |
| corvus-memory | site cards, selector heuristics, export/import |
| corvusd | daemon binary: wires everything, service lifecycle, config |
| corvus | CLI: doctor, serve, attach, replay, bench, config print |

Key crates: tokio, axum, tower, serde, serde_json, rusqlite (bundled), tracing, tracing-subscriber, thiserror, clap, uuid, dashmap, arc-swap.

## 4. Wire protocol

One envelope for all transports. Versioned, forward compatible.

    {
      "v": 1,
      "id": "01J...ULID",
      "kind": "command" | "response" | "event" | "hello" | "ping" | "pong",
      "browser": "edge:default",
      "session": "s-...",
      "tool": "browser_click",
      "args": { "target": "@e12" },
      "meta": { "timeout_ms": 5000, "idempotency_key": "...", "trace_id": "..." }
    }

Rules:

- The daemon assigns ULID ids; the extension echoes them.
- Every command carries an idempotency key; on reconnect the daemon replays only keys with no recorded response.
- Responses carry a typed error object (see section 8) and never a bare string.
- Events (navigation, download started, DOM mutation summaries) flow extension -> daemon unsolicited.
- Handshake: hello(browser, profile, version, capabilities) -> hello_ack(daemon version, limits). Capability negotiation keeps old extensions working.

## 5. MCP surface

- Transports: stdio (default for IDE clients) and streamable-http on 127.0.0.1:7331/mcp (default for DSH and web clients).
- Tool naming: <domain>_<verb>, stable forever; aliases never removed, only deprecated.
- Every tool declares annotations: readOnlyHint, destructiveHint, idempotentHint, openWorldHint.
- Long outputs are capped per call: `max_chars` truncates long strings and sets `truncated: true`; M2 has no cursor pagination.
- Resource: corvus://session/<id>/step/<n> returns the recorded page snapshot of that step, so agents can inspect history without re-running.
- Prompts: none in M1 (agents should use tools), revisited in M3.

### 5.1 browser_navigate payload

Request: `{ "url": "<absolute http, https or file URL>", "tab_id": "<optional tab id>" }`.

Result:

| field | type | meaning |
|---|---|---|
| navigated | boolean | true when the navigation was issued |
| loaded | boolean | true when the new document reached readyState complete (or fired load) within the wait budget; false when the budget expired |
| url | string | final URL after redirects |
| previous | string or null | URL of the document before the navigation |
| same_document | boolean | true when only the URL fragment changed, so no document load was needed |
| started_at | number | Unix time in milliseconds when the navigation started |

The extension waits for the new document before answering, so a `page_snapshot` issued after `browser_navigate` reflects the new page. The wait budget is `max(0, min(meta.timeout_ms - 2000, 15000))` milliseconds. When the budget expires the call is still a success with `loaded: false` and the current `url`; it never becomes `TIMEOUT`. The daemon forwards this result unchanged.

### 5.2 M2 wave 1 tool surface

The frozen M2 wave 1 surface has 18 tools. `browser_navigate`, `page_snapshot`, `browser_click` and `browser_type` keep their M1 behaviour; the other 14 are added in this wave. Each result below is the JSON object inside a successful MCP tool result; failures use the taxonomy in section 8 and set `isError`.

Common conventions:

- `tab_id` is a string in both arguments and results and is optional on page and action tools; when omitted the tool acts on the active tab. A syntactically foreign or unknown id returns `NOT_FOUND` rather than `BAD_ARGS`. `tab_close` and `tab_switch` require the target tab explicitly.
- `target` accepts an `@eN` reference or a CSS selector. References are allocated per document, and `page_snapshot` and `page_find` share one reference space, so either can feed `browser_click`, `browser_type`, `browser_hover`, `browser_press`, `browser_scroll` or `browser_select`.
- `max_chars` caps truncate the returned string and set `truncated: true`; they never turn a successful call into an error.

| Tool | Arguments (required unless marked ?) | Result |
|---|---|---|
| `browser_navigate` | `url`; `tab_id?` | `navigated`, `loaded`, `url`, `previous`, `same_document`, `started_at` |
| `page_snapshot` | `tab_id?`; `mode?` (`interactive` default or `full`); `max_nodes?` 40..3000 | `url`, `title`, `viewport`, `mode`, `node_cap`, `node_count`, `element_count`, `text_count`, `ref_count`, `truncated`, `nodes[]` |
| `browser_click` | `target`; `tab_id?` | `clicked`, `ref`, `selector`, `tag`, `role`, `name`, `bbox`, `point`, `events[]`, `url` |
| `browser_type` | `target`; `text`; `clear?` default true; `blur?` default false; `tab_id?` | `typed`, `ref`, `selector`, `tag`, `role`, `cleared`, `events[]`, `value_length`, `url`, `value` (passwords return `masked` instead) |
| `page_text` | `tab_id?`; `max_chars?` 1..200000 default 100000 | `url`, `title`, `text`, `truncated` |
| `page_html` | `tab_id?`; `target?` default whole document; `max_chars?` 1..500000 default 200000 | `url`, `html`, `truncated` |
| `page_find` | `query`; `tab_id?`; `role?`; `limit?` 1..100 default 20; `regex?` default false | `url`, `count`, `matches[]` with `ref`, `tag`, `role`, `text` |
| `browser_hover` | `target`; `tab_id?` | `hovered: true`, `url`, `target` with `ref?`, `tag`, `text?` |
| `browser_press` | `keys` string or string[]; `target?`; `tab_id?` | `pressed: true`, `keys[]`, `url` |
| `browser_scroll` | `direction` (`up`, `down`, `left`, `right`); `pages?` 0..50 default 1; `target?`; `tab_id?` | `scrolled: true`, `x`, `y`, `url` |
| `browser_select` | `target`; `values[]`; `tab_id?` | `selected[]`, `value`, `url` |
| `browser_eval` | `expression`; `world?` (`main` or `isolated` default; evaluated through CDP `Runtime.evaluate`); `tab_id?` | `ok: true`, `value`; or `ok: false`, `error` with `name`, `message` |
| `browser_wait` | `tab_id?`; at least one of `selector?`, `text?`, `url_contains?`; `timeout_ms?` 100..120000 default 10000 | `matched: true`, `waited_ms`, `url` |
| `browser_history` | `action` (`back`, `forward`, `reload`; back/forward move one entry); `tab_id?` | `action`, final `url`, `loaded` |
| `tab_list` | none | `tabs[]` with `tab_id`, `index`, `active`, `url`, `title`, `status` |
| `tab_new` | `url?`; `active?` default true | `tab_id`, `url`, `loaded`, `previous_tab_id` |
| `tab_close` | `tab_id` | `closed: true`, `tab_id`, `next_tab_id?` |
| `tab_switch` | `tab_id` | `tab_id`, `url`, `title`, `active: true` |

Notes:

- `browser_press` accepts Enter, Tab, Escape, Backspace, Delete, Space, ArrowUp, ArrowDown, ArrowLeft, ArrowRight, PageUp, PageDown, Home, End, single character keys, and the Control+, Shift+, Alt+ and Meta+ modifiers (for example `Control+Shift+KeyK`).
- `browser_select` writes every value in `values`; a `<select multiple>` accepts more than one.
- `browser_eval` is debugger-backed: it evaluates through the automatic per-tab debugger channel with CDP `Runtime.evaluate`. The default `isolated` world runs in a dedicated isolated world created by the channel, and `main` runs in the page world; the page Content-Security-Policy does not restrict the expression. A debugger attach refusal maps to `CAPABILITY_UNAVAILABLE`. A page exception is a successful tool call with `{ "ok": false, "error": { "name", "message" } }`; only tool-level failures set `isError`. Values that cannot be serialised to JSON return `null` with `"value_note": "unserializable"`.
- `browser_history` moves exactly one entry per `back` or `forward` through CDP `Page.getNavigationHistory` and `Page.navigateToHistoryEntry`, waits for the document to load, and returns the final `url`; an out-of-range entry is `NOT_FOUND`.
- `browser_wait` needs at least one condition; a timeout is `TIMEOUT` with `details.waited_ms`. When an error carries `details`, the MCP tool result text appends them as compact JSON (for example `TIMEOUT: wait timed out after 100 ms {"waited_ms":4321}`); error results do not carry `structuredContent`.
- `tab_close` returns `next_tab_id` when closing the active tab activates a neighbour.

### 5.3 M2 wave 2 tool surface

M2 wave 2 adds eight tools for a total of 26. The successful results below are the JSON object inside an MCP tool result; failures use the taxonomy in section 8 and set `isError`.

Common routing and timeouts:

- Every browser-facing tool accepts an optional `browser_id` string, including all 18 tools in section 5.2; `browser_list` is the exception because it operates on the set of browsers. When `browser_id` is omitted and exactly one browser is connected, that browser is selected; when none is connected the call fails with `TRANSPORT_DOWN`; when two or more are connected it fails with `BAD_ARGS` and `details` `{"reason":"multiple_browsers","browser_ids":[...]}`. A present but unknown id fails with `TRANSPORT_DOWN` and `details` `{"browser_id":"..."}`. The existing 18 tools keep their other arguments, behaviour and result shapes unchanged.
- `browser_list` takes no arguments and returns every connected browser sorted by `connected_at_ms` ascending; it is served from the live transport registry without an extension roundtrip.
- All eight new tools use a 60000 ms `meta.timeout_ms` by default; `download_wait` uses `min(args.timeout_ms + 5000, 125000)` instead. The nine error codes in section 8 are unchanged.

| Tool | Arguments (required unless marked ?) | Result |
|---|---|---|
| `browser_list` | none | `browsers[]` with `browser_id`, `profile`, `version`, `capabilities[]`, `connected_at_ms`, sorted by `connected_at_ms` ascending |
| `net_capture` | `action` (`start`, `stop` or `status`); `tab_id?`; `browser_id?` | start: `capturing: true`, `tab_id`, `started_at`, `capacity: 2000`; stop: `capturing: false`, `tab_id`, `entries`; status: `capturing`, `tab_id`, `started_at?`, `entries` |
| `net_log` | `tab_id?`; `browser_id?`; `since_seq?` default 0; `limit?` 1..1000 default 100; `url_contains?`; `method?`; `resource_type?`; `status_min?`; `status_max?`; `include_headers?` default false | `entries[]` with `seq`, `ts`, `tab_id`, `request_id`, `method`, `url`, `status` (number or null), `mime` (string or null), `resource_type`, `from_cache`, `duration_ms` (number or null), `error` (string or null), `intercepted` (`{action, rule_id}` or null), optional `request_headers` and `response_headers`; `next_seq`; `capturing` |
| `net_clear` | `tab_id?`; `browser_id?` | `cleared` |
| `net_intercept` | `action` (`set` or `clear`); `browser_id?`; `tab_id?`; `rules?` 1..50 | `ok: true`, `scope` (`tab` or `browser`), `tab_id?`, `rules[]` with `id`, `action`, `url_regex`, optional `method`, `resource_type`; `applied` |
| `download_list` | `browser_id?`; `limit?` 1..200 default 50 | `downloads[]` with `id`, `filename`, `url`, `state`, `bytes_received`, `total_bytes`, `danger`, `exists`, `started_at`, optional `tab_id`; newest first |
| `download_wait` | `browser_id?`; `id?`; `timeout_ms?` 100..120000 default 30000 | `id`, `state`, `filename`, `url`, `bytes_received`, `total_bytes` |
| `upload_file` | `target`; `files[]` 1..10 absolute paths; `browser_id?`; `tab_id?` | `uploaded: true`, `files[]`, `target` with optional `ref`, `tag`, `text`; `input` with optional `accept`, `multiple` |

Capture lifecycle:

- `net_capture` is opt-in per tab; omitted `tab_id` means the active tab. `start` attaches the debugger when needed and enables the CDP Network domain; the buffer is a per-tab ring of 2000 entries. An attach refusal is `CAPABILITY_UNAVAILABLE` with `details` `{"tab_id":"...","cause":"..."}`. Capture survives navigation in the same tab and ends silently when the tab closes.
- `net_log` reads captured data. Omitted `tab_id` reads every captured tab of the selected browser. Entries are sorted by `seq` ascending; filters run before `limit`, and `limit` runs after filtering. `next_seq` is the last returned `seq`, or `since_seq` when the result is empty. Headers are present only when `include_headers` is true. `net_clear` clears one tab, or every captured tab when `tab_id` is omitted.

Interception:

- `net_intercept` with `set` replaces the rule set for its scope; `clear` removes it. A rule needs a unique `id` (1..64), a required `url_regex` compiled as a JavaScript `RegExp`, and an `action` of `block`, `mock` or `continue`; `method` and `resource_type` narrow the match. An invalid regex is `BAD_ARGS` with `details` `{"rule_id":"..."}`. `mock` accepts `status` 100..599 (default 200), `headers` and `body_base64` (at most 1 MiB decoded); these three fields are invalid on other actions.
- Rules are evaluated in array order and the first match wins. A tab-scoped set also starts capture on that tab; a browser-scoped set (no `tab_id`) applies to any capturing tab. `block` calls CDP `Fetch.failRequest` with `BlockedByClient`; `mock` calls `Fetch.fulfillRequest` with the status, headers and base64 body (when a body is present and no content type is given, `text/plain; charset=utf-8`); `continue` calls `Fetch.continueRequest`. Every paused request is continued even when a CDP call fails, and that failure returns `isError`.

Downloads and uploads:

- `download_list` returns tracked downloads newest first. It exposes metadata and does not return a local filesystem path.
- `download_wait` resolves when the selected download reaches `complete` or `interrupted`. Its `id` is a string: a non-negative decimal string selects that download; any other string is `BAD_ARGS` with `details` `{"id":"..."}`; a JSON number for `id` is rejected at the schema layer as `BAD_ARGS`.
- An unknown decimal `id` never returns `NOT_FOUND`; it ends as `TIMEOUT` with `details` `{"waited_ms":...,"id":"..."}` (without `id`, the details carry only `waited_ms`). Without `id`, `download_wait` selects the newest download that is new or started within the 5000 ms before the call, never an older one; an already-complete candidate returns immediately.
- `upload_file` resolves `target` (an `@eN` reference from that tab's latest snapshot or a CSS selector) through CDP and calls `DOM.setFileInputFiles`, so hidden file inputs work; it then dispatches `input` and `change` events. It pre-checks every local path before setting the files and returns `BAD_ARGS` with `details` `{"path":"..."}` for a missing or unreadable file.
- The browser may refuse file-URL access when the extension has no file access. A target that is not an `input[type=file]` returns `BAD_ARGS` with `details` `{"reason":"not_a_file_input","tag":"...","input_type":"..."}`.

## 6. Extension design

Three contexts, no pages:

1. **service worker**: command dispatch, tabs API, scripting, debugger attach/detach, downloads.
2. **content script**: DOM snapshot, click/type synthesis, waits, mutation observers.
3. **offscreen document**: WebSocket owner (SW cannot hold a socket reliably), keepalive, reconnect loop.

The popup is status-only: one connection dot and one status line, with no controls. The debugger capability is advertised unconditionally and attached on demand when a CDP-backed tool needs it; a refused attach is the only reason the channel becomes unavailable.

Reconnect policy: exponential backoff with jitter (250ms -> 8s cap), immediate retry on network-online, alarm every 30s as a floor, and a daemon-push "resume" message when the daemon restarts.

## 7. Reliability model

    extension states: IDLE -> CONNECTING -> READY -> DEGRADED -> IDLE

- DEGRADED means: attached but a capability is unavailable (e.g. the browser refused the debugger attach). Tools that need the missing capability return CAPABILITY_UNAVAILABLE instead of hanging.
- Every command has a deadline; on expiry the daemon cancels at the extension and returns TIMEOUT with a replayable id.
- The router keeps a per-browser serial queue for actions that must not interleave (click/type) and a parallel pool for read-only perception.
- Daemon restart does not lose sessions: session state is in SQLite, and the extension resumes by replaying unacknowledged commands.

## 8. Error taxonomy

| code | meaning | retryable |
|---|---|---|
| BAD_ARGS | schema violation | no |
| NOT_FOUND | selector/page/tab missing | yes, after page change |
| NOT_INTERACTABLE | element covered/disabled/offscreen | yes, after scroll |
| TIMEOUT | deadline exceeded | yes |
| NAVIGATION_LOST | page navigated mid-command | yes, re-resolve |
| CAPABILITY_UNAVAILABLE | debugger attach refused by the browser, or the page cannot be scripted | no, until the page or permission changes |
| BROWSER_GONE | tab/browser closed | no |
| TRANSPORT_DOWN | extension disconnected | yes, after reconnect |
| INTERNAL | bug; carries trace id | no |

## 9. Storage

SQLite at ~/.corvus/corvus.db (WAL).

| table | columns |
|---|---|
| sessions | id, started_at, ended_at, browser, profile, agent, status |
| steps | id, session_id, seq, ts, tool, args_json, result_json, error_code, duration_ms |
| snapshots | id, step_id, url, title, nodes, blob_path |
| network | id, step_id, url, method, status, ms, req_headers, res_headers, body_path |
| site_cards | domain, selector, role, success_count, fail_count, last_used, payload |

Blobs (screenshots, response bodies) live beside the DB under ~/.corvus/blobs/<yyyy-mm-dd>/, with a retention policy (default 30 days, configurable).

## 10. Configuration

~/.corvus/config.toml:

    [daemon]
    http_port = 7331
    ws_port = 7330
    bind = "127.0.0.1"

    [recording]
    enabled = true
    mode = "structured"      # structured | full
    retention_days = 30

    [tools]
    paginate_over_bytes = 65536
    default_timeout_ms = 60000

    [memory]
    enabled = true
    export_format = "jsonl"

Per-tool timeout overrides: `browser_wait` gets `args.timeout_ms + 5000` ms capped at 125000 ms, `browser_eval` gets 30000 ms, `download_wait` gets `args.timeout_ms + 5000` ms capped at 125000 ms, and every other tool gets the 60000 ms default; the router clamps every deadline to the 125000 ms ceiling.

## 11. Build and CI

- cargo workspace, edition 2021, MSRV pinned in rust-toolchain.toml.
- CI matrix: windows-latest, macos-14, ubuntu-22.04; each runs fmt, clippy -D warnings, nextest.
- Extension: esbuild bundle, no framework; size budget 1MB; manifest key pinned so the extension id is stable across machines.
- Release: cross-built binaries with checksums, GitHub release notes generated from conventional commits.

## 12. Security posture

Deliberate: no permission prompts, no allow-list, no per-action confirmation. The trust boundary is the OS user account; the daemon binds to loopback only and authenticates extensions with a per-machine secret stored in the OS keychain (Windows DPAPI, macOS Keychain, libsecret).

Threats considered:

- Other local users: loopback + per-user secret + file permissions.
- Malicious web page: content script never evaluates page-controlled strings as code; all selectors and scripts come from the daemon.
- Extension supply chain: pinned manifest key, signed releases.

Non-goal: protecting the user from their own agent. That is out of scope by design.

## 13. Repository layout

    corvus/
      crates/corvus-proto, corvus-mcp, corvus-transport, corvus-runtime,
             corvus-record, corvus-memory, corvusd, corvus
      extension/
        manifest.json, background.js, content.js, offscreen.html, popup.html
      docs/
      tests/e2e/
      scripts/
      .github/workflows/