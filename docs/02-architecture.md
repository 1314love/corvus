# Corvus Architecture

[English](02-architecture.md) | [中文](02-architecture.zh.md)

Status: 1.3.0 is the released surface: 43 MCP tools over hybrid CDP and extension transport, compact snapshots, batched calls, single-root packaging, Linux packages, Docker and Kubernetes deployment. The uncommitted working tree adds in-progress 1.4.0 work: the domain, application and api crates plus a TypeScript and Vue 3 extension rewrite; the same tree already wires the daemon to the new crates, but nothing is committed or released. Audience: engineers.

## 1. Constraints that shape the design

1. **Must drive the browser the user is already using by default.** The zero-setup path uses the extension in a browser that is already running: no relaunch, no dedicated profile and no remote-debugging flag. An optional, explicitly configured loopback CDP path can serve every browser-facing tool with `@c` references, so the design is still not CDP-only: the extension remains the zero-setup entry point and provides `@eN` references on its channel.
2. **Zero approvals, zero pairing, no required setup page.** The extension ships without a setup step; the optional options page only overrides the daemon URL and the optional CDP endpoint.
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
    |  mcp-server  -> tool registry -> channel router -> runtime    |
    |  transport hub (ws / native-messaging / long-poll)            |
    |  cdp client (optional): browser-level WS, Target sessions     |
    |  recorder (sqlite + jsonl + trace)                            |
    |  recording (sqlite)    metrics (prometheus)   health          |
    +--------------------------------------------------------------+
        |  framed JSON envelope over WS (primary)
        v
    extension (MV3): service worker + content script + offscreen keepalive
        |  chrome.tabs / scripting / debugger / webRequest / downloads
        v
    browser  <--- optional direct CDP from corvusd (loopback only)

Processes: one corvusd per user session (a service on Windows and Linux; foreground on macOS), one extension per browser profile, one CLI client (corvus).

## 3. Crates

| crate | role |
|---|---|
| corvus-proto | wire types: envelope, command, response, error taxonomy, versioning |
| corvus-domain | domain model and ports for the 1.4.0 refactor; no I/O, no transport, no storage |
| corvus-application | use cases plus the 32 browser-side tool specifications shared by the MCP and REST surfaces |
| corvus-api | REST surface on `/api/v1` with an OpenAPI schema, JWT authentication and argon2 password hashing |
| corvus-mcp | MCP server: tool registry, JSON-RPC, stdio + streamable-http transports |
| corvus-transport | WS server, native-messaging host, long-poll fallback, browser registry |
| corvus-runtime | released 1.3.0 command router; the working tree folds it into corvus-application and corvusd |
| corvus-record | session recorder, sqlite schema, step store, replay support |
| corvus-cdp | browser-level CDP client, endpoint validation, event ring, raw and high-level CDP tools |
| corvusd | daemon binary: wires everything, service lifecycle, config |
| corvus | CLI: doctor, serve, attach, replay, bench, config print |

The domain, application and api crates belong to the 1.4.0 refactor. In the working tree `corvusd` and `corvus-mcp` depend on them and `corvusd` mounts `corvus_api::router`; released 1.3.0 binaries still use the old wiring.

Key crates: tokio, axum, serde, serde_json, rusqlite (bundled), tracing, tracing-subscriber, tracing-appender, thiserror, clap, uuid.

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
- Discovery: `tools/list` returns brief descriptions by default (20,452 bytes for 43 tools); `detail: "full"` returns 37,067 bytes of complete descriptions, and `tool_help` describes one tool on demand.
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

M2 wave 2 added eight tools. The successful results below are the JSON object inside an MCP tool result; failures use the taxonomy in section 8 and set `isError`.

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

### 5.4 M3 wave 1 tool surface

M3 wave 1 adds six tools for a total of 32. Recording is always on; the four session tools read the SQLite recording store, while `metrics` and `logs_tail` read process-local observability state. The successful results below are the JSON object inside an MCP tool result; failures use the taxonomy in section 8 and set `isError`.

| Tool | Arguments (required unless marked ?) | Result |
|---|---|---|
| `session_list` | `limit?` 1..200 default 50; `since_ms?` >= 0 | `sessions[]` with `session_id`, `started_at`, `ended_at` (number or null), `status`, `transport`, `agent` (string or null), `steps`, `errors`; `total`; newest `started_at` first |
| `session_steps` | `session_id`; `from_seq?` >= 0 default 0; `limit?` 1..500 default 100 | `session_id`; `steps[]` with `seq`, `ts`, `tool`, `browser_id` (string or null), `tab_id` (string or null), `ok`, `error_code` (string or null), `duration_ms`, `args`, `args_truncated`, `result`, `result_truncated`; `total_steps`; `next_seq` only when more steps remain |
| `session_replay` | `session_id`; `from_seq?` default first; `to_seq?` default last; `mode?` `dry_run` or `live` default `dry_run`; `browser_id?`; `continue_on_error?` default false | `session_id`, `mode`, `total`, `planned[]` with `seq`, `tool`, `args`, `executed[]` with `seq`, `tool`, `ok`, `error_code`, `duration_ms`, `skipped[]` with `seq`, `tool`, `reason`, `stopped_at?`, `error_code?`, `replay_session_id?` |
| `session_export` | `session_id`; `format?` `jsonl` | `session_id`, `format`, `content`, `bytes`, `truncated` |
| `metrics` | none | `uptime_ms`, `started_at`, `protocol`, `browsers_connected`, `browsers[]`, `sessions_total`, `sessions_active`, `tool_calls_total`, `tool_errors_total`, `by_tool` (`tool` -> `calls`, `errors`, `avg_ms`), `errors_by_code`, `inflight`, `recording` (`enabled`, `db_path`, `sessions`, `steps`), `last_call_at_ms?` |
| `logs_tail` | `limit?` 1..1000 default 200; `level?` `debug`, `info`, `warn` or `error`; `since_ms?` | `entries[]` with `ts`, `level`, `target`, `message`, `fields`; `dropped` |

**Session and recording.** A session is one MCP session: the HTTP `Mcp-Session-Id`, or one generated `s-<uuid v4>` id per stdio process. Every `tools/call` is recorded, including failures, with the resolved `browser_id` and `tab_id`, duration, `ok` and `error_code`; `args` and `result` are stored as JSON text capped at 262144 bytes each and flagged with `args_truncated`/`result_truncated`. Recording is asynchronous, and a database error logs a warning and never fails the call. A session ends as `closed` on close or shutdown; a session left active by a previous process is marked `abandoned` at the next startup.

**Session reads.** `session_steps` returns steps with `seq` ascending from `from_seq`. `args` and `result` are parsed JSON when the stored text is complete and parses, otherwise the stored prefix as a string; a value whose serialized form exceeds 65536 bytes is returned as null with the matching `*_truncated` flag and `*_bytes` size. `next_seq` is present only when more steps remain. Unknown session ids are `NOT_FOUND` with `details` `{"session_id":"..."}` for `session_steps`, `session_replay` and `session_export`.

**Replay.** `session_replay` defaults to `dry_run`, which only plans; `live` reissues steps through the normal router with normal tool timeouts and records them into a new session with `agent` `corvus-replay` and `transport` `internal`, returned as `replay_session_id`. `session_list`, `session_steps`, `session_replay`, `session_export`, `metrics`, `logs_tail` and `browser_list` are skipped with reason `control_tool`; in live mode `tab_close` and `tab_switch` are skipped with reason `tab_tool`, and recorded `tab_id` values are removed so steps run against the active tab unless `browser_id` selects a browser. Replay stops at the first failing step unless `continue_on_error` is true; a replayed tool failure is data, not an error of `session_replay` (`isError` false). An empty replayable range is `BAD_ARGS` with `details.reason` `no_replayable_steps`; `live` without a connected browser is `TRANSPORT_DOWN`.

**Export and observability.** `session_export` writes JSONL: one `{"type":"session",...}` line with every session column, then one `{"type":"step",...}` line per step with `args_json` and `result_json` as strings. Content is capped at 2097152 bytes and `truncated` is true when cut; `format` is `jsonl` only. `metrics` counters are process-lifetime atomics with no reset, and `avg_ms` is integer milliseconds truncated. `logs_tail` reads a 5000-entry in-memory ring; `ts` is epoch milliseconds, `fields` holds at most 16 primitive top-level event fields with values stringified, `dropped` counts overwritten events since start, and an invalid `level` is `BAD_ARGS` with `details.level`.

`GET /metrics` on the MCP HTTP port, next to `/health`, serves Prometheus text: `corvus_uptime_seconds`, `corvus_browsers_connected`, `corvus_tool_calls_total{tool="..."}`, `corvus_tool_errors_total{code="..."}`, `corvus_sessions_total` and `corvus_steps_total`. `/health` keeps its current shape.

### 5.5 Concurrency and cancellation

M4 wave 1 adds no tools. It changes how the router schedules calls and how an in-flight `tools/call` can be cancelled; all 32 tools keep their arguments and result shapes.

**Scope gate.** Every browser command is assigned to one lane before dispatch. The lane key is `(browser_id, tab_id)`. Calls in one lane run one at a time and are ordered; calls in different lanes run concurrently and have no ordering relationship. An explicit `tab_id` therefore runs in parallel with other tabs and is ordered with other calls on the same tab. An omitted `tab_id` resolves to the active tab at dispatch time and joins the lane for that tab, so it is ordered with explicit calls for the same tab instead of starting a second lane. `tab_new`, `tab_close`, `tab_switch`, `download_wait`, and `net_intercept` without a `tab_id` use the per-browser lane, which is keyed by `browser_id` alone; they are ordered with each other and do not serialize independent tab work. Calls that do not execute browser commands (`browser_list`, `metrics`, `logs_tail`, and the read-only session tools) do not enter a lane. `page_text`, `page_html`, `page_find`, `net_log` and `download_list` are ungated read-only calls: they read existing state, are never queued behind a write on the same tab, and do not block writers.

**MCP cancellation.** A client cancels an in-flight request with `notifications/cancelled` and `params.requestId` set to the JSON-RPC id of the `tools/call`. The daemon answers that request with JSON-RPC error `-32800` and records the attempt in `session_steps` with `ok: false` and `error_code` `"CANCELLED"`. To stop browser work it sends the extension an event frame with `event: "cancel"`; the extension aborts cooperatively. Cancellation applies only to the named request. The error message is `"request cancelled"`, and retryable is false.

**What is not cancelled.** The daemon stops waiting immediately and returns `-32800` without waiting for the browser. Browser work is aborted cooperatively for `browser_wait`, `download_wait`, `browser_eval` and `browser_navigate`, and may finish late for other tools. Effects already applied to a page, a download or a tab are not rolled back.

### 5.6 Hybrid CDP transport

v1.2.0 added ten tools for a total of 42, and v1.3.0 adds `browser_batch` for 43. The original 32 keep their arguments and result shapes. Browser-facing calls travel over one of two channels.

**Detection.** The extension options page may store a CDP endpoint under `corvus.cdpEndpoint`. `auto` probes loopback ports 9222, 9223 and 9333 with an 800 ms timeout, reads `/json/version`, and chooses an available endpoint whose browser family matches the extension. Chrome and Chromium share a family; Edge, Brave, Opera and Vivaldi are distinct. The handshake carries a descriptor with the endpoint, WebSocket debugger URL, browser, protocol version, source and family. The `cdp` capability is advertised only when detection succeeded.

**Client.** `corvus-cdp` validates that the descriptor endpoint and debugger URL are loopback `ws://` or `wss://` and keeps one long-lived browser-level WebSocket. Page work uses flattened `Target.attachToTarget` sessions; requests are correlated by numeric id. The client retains a 1000-event ring with monotonic sequence numbers and a `dropped` counter. Reconnects back off from 250 ms to 10 s, a heartbeat runs every 15 s, and request timeouts default to 10 s with a 60 s maximum. A dropped socket fails pending requests; it never replays them.

**Routing.** `auto` prefers CDP and falls back to the extension only when the CDP call reports `Unsupported`. `cdp` is strict and never falls back. `extension` ignores CDP. `--no-cdp` removes the CDP path, and a `cdp` mode request against it fails. `browser_channel` switches the running daemon between modes; the startup mode comes from `--channel` or `CORVUS_CHANNEL`.

**Availability.** In v1.3.0, 25 tools are shared, eight are CDP-only, none are extension-only and ten are daemon-local. Forced CDP serves all 33 browser-facing tools; forced extension fails the CDP-only tools with `CAPABILITY_UNAVAILABLE` instead of silently switching. Auto mode may only move a call after an `Unsupported` CDP result.

**Categories.** Discovery uses two levels: `tools/list` returns brief descriptions by default (20,452 bytes for 43 tools), while `detail: "full"` returns the complete descriptions (37,067 bytes). `tool_help` describes one tool on demand.

| Category | Tools |
|---|---|
| page | 10 |
| input | 7 |
| tabs | 4 |
| network | 5 |
| files | 3 |
| cdp | 5 |
| sessions | 4 |
| diagnostics | 4 |
| help | 1 |
| **total** | **43** |

**Security.** A browser-level CDP endpoint can read cookies, inject script, intercept traffic and drive every profile in that browser instance. Corvus adds no authentication in front of it. Validation accepts loopback only, but any local process that reaches the debugging port has the same power. Never expose 9222, 9223, 9333, 7330 or 7331 beyond loopback.

### 5.7 Token-efficient snapshots and discovery

v1.3.0 keeps the hybrid transport and reduces default context cost. `page_snapshot` defaults to `format: "compact"` on both channels and returns one text tree; `format: "json"` preserves the object form. Compact lines use `@eN role "name" [states] value="..." bbox=x,y,w,h` on the extension channel and `@c<backendNodeId>` on CDP, with `# "text"` lines for text nodes. `max_nodes` defaults to 300, and optional `viewport_only` and `include_bbox` narrow or annotate the tree. References are valid only for the snapshot that produced them.

`browser_batch` runs up to 20 `{tool,args}` steps in one daemon-local call, stops on the first failed step by default (`stop_on_error: true`), and rejects recursion and daemon-control tools. Discovery is lean by default: brief `tools/list` is 20,452 bytes for 43 tools, `detail: "full"` is 37,067 bytes, and `tool_help` returns one full descriptor. A successful `tools/call` returns exactly one compact JSON text block and no `structuredContent`, so clients parse `content[0].text`. Forced CDP serves all 33 browser-facing tools.

## 6. Extension design

Three runtime contexts and two small UI surfaces, all built from `extension/src`:

1. **service worker**: command dispatch, tabs API, scripting, debugger attach/detach, downloads.
2. **content script**: DOM snapshot, click/type synthesis, waits, mutation observers.
3. **offscreen document**: WebSocket owner (SW cannot hold a socket reliably), keepalive, reconnect loop.

The popup is status-only: one connection dot and one status line, with no controls. The options page is optional and stores the daemon WebSocket URL (`ws://` or `wss://`, default `ws://127.0.0.1:7330`) under `corvus.daemonUrl`, plus the optional CDP endpoint under `corvus.cdpEndpoint`; neither setting is part of the command path. The `cdp` capability is advertised only when endpoint detection succeeded. The debugger capability is advertised as a base capability and attached on demand when a CDP-backed tool needs it; a refused attach is the only reason the debugger channel becomes unavailable.

Reconnect policy: exponential backoff with jitter (250ms -> 8s cap), immediate retry on network-online, alarm every 30s as a floor, and a daemon-push "resume" message when the daemon restarts.

## 7. Reliability model

    extension states: IDLE -> CONNECTING -> READY -> DEGRADED -> IDLE

- DEGRADED means: attached but a capability is unavailable (e.g. the browser refused the debugger attach). Tools that need the missing capability return CAPABILITY_UNAVAILABLE instead of hanging.
- Every command has a deadline; on expiry the daemon cancels at the extension and returns TIMEOUT with a replayable id.
- The router serializes commands per `(browser_id, tab_id)` lane and leaves read-only perception ungated, as described in section 5.5.
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
| CANCELLED | the command was cancelled before the browser answered | no |

## 9. Storage and file layout

The daemon keeps every backend file under one root selected by `--home <dir>` or `CORVUS_HOME`. Platform defaults are `%LOCALAPPDATA%\Corvus` on Windows, `$XDG_DATA_HOME/corvus` (falling back to `~/.local/share/corvus`) on Linux, and `~/Library/Application Support/Corvus` on macOS. The Windows installer uses `%ProgramData%\Corvus` for the service, the Linux package uses `/opt/corvus`, and the Docker image and Kubernetes manifests use `/data`.

    <root>/
      bin/            corvusd, corvus
      data/corvus.db  SQLite recording store
      logs/           corvusd.log.YYYY-MM-DD (UTC) plus optional service files
      config/         reserved for file configuration
      extension/      unpacked extension

Resolution order: `--db <path>` wins; otherwise a configured `--home`/`CORVUS_HOME` selects `<root>/data/corvus.db`; without either, `CORVUS_DB_PATH` is still honoured, then the platform default database. Relative roots become absolute against the process working directory. `corvusd serve` creates `data/`, `logs/` and `config/`, but not `bin/` or `extension/`. File logging is on by default at `<root>/logs/corvusd.log.YYYY-MM-DD` with UTC daily rotation; `--log-file` overrides the path and `--no-log-file` disables file output. See [05-deployment.md](05-deployment.md) for service and container layouts.

The recording store is SQLite with WAL, schema version 1 in `schema_meta`.

| table | columns |
|---|---|
| sessions | `id`, `started_at`, `ended_at`, `browser`, `profile`, `agent`, `transport`, `status`, `steps`, `errors` |
| steps | `id`, `session_id`, `seq`, `ts`, `tool`, `args_json`, `result_json`, `error_code`, `duration_ms`, `browser_id`, `tab_id`, `ok`, `args_truncated`, `result_truncated`; `UNIQUE(session_id, seq)` with an index on `(session_id, seq)` |

On startup, sessions left active by a previous process get `ended_at` set to now and `status` `abandoned`; sessions with `started_at` older than 30 days are pruned.

`snapshots`, `network` and `site_cards` are not created in schema v1; they remain future work, and this milestone writes no blob directory.

## 10. Configuration

No `config.toml` is loaded in this release. Settings come from flags and environment variables:

| Setting | Control | Default |
|---|---|---|
| Bind address | `corvusd serve --bind`, `corvus serve --bind` | `127.0.0.1` |
| WebSocket port | `--ws-port` | 7330 |
| MCP HTTP port | `--http-port` | 7331 |
| Home root | `--home`, `CORVUS_HOME` | platform default |
| Database | `--db`, then the home root, then `CORVUS_DB_PATH` | `<root>/data/corvus.db` |
| File log | `--log-file`, `--no-log-file` | `<root>/logs/corvusd.log` |
| Log filter | `RUST_LOG` | `info` |
| MCP transport | `--stdio`, or HTTP on `/mcp` | HTTP |
| CDP endpoint | `--cdp-endpoint`, `CORVUS_CDP_ENDPOINT` | none; extension descriptor |
| Channel | `--channel`, `CORVUS_CHANNEL` | `auto` |
| CDP disable | `--no-cdp` | CDP enabled |

Recording is always on and keeps the fixed 30-day retention. `corvus config --client <name> --transport stdio|http [--home <dir>]` prints a ready `mcpServers` JSON object on stdout, with a location hint on stderr; the stdio form pins `--home` when supplied, and the HTTP form points at `http://127.0.0.1:7331/mcp`.

Per-tool timeout overrides: `browser_wait` gets `args.timeout_ms + 5000` ms capped at 125000 ms, `browser_eval` gets 30000 ms, `download_wait` gets `args.timeout_ms + 5000` ms capped at 125000 ms, and every other tool gets the 60000 ms default; the router clamps every deadline to the 125000 ms ceiling.

## 11. Build and CI

- cargo workspace, edition 2021, Rust 1.90 as MSRV in `Cargo.toml` and `rust-toolchain.toml`.
- CI matrix: windows-latest, macos-latest and ubuntu-latest; each runs `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`.
- Packaging jobs: `scripts/package-linux.sh` builds the x86_64 tar.gz and verifies the systemd unit; a Docker job builds the image, checks the reported version and `/health`, and asserts the 43-tool list.
- Extension: TypeScript and Vue 3 sources under `extension/src`, bundled by Vite (`pnpm build`); manifest key pinned so the extension id is stable across machines.
- Release: platform packages with `SHA256SUMS`; macOS has no packaged installer in 1.3.0.

## 12. Security posture

Deliberate: no permission prompts, no allow-list, no per-action confirmation, and no authentication or pairing secret. The trust boundary is the OS user account: browser transports and the MCP HTTP server bind to loopback, and home-root file permissions protect the recording database. Any local process that can reach ports 7330 or 7331 can drive the connected browser, so never expose those ports beyond loopback. The optional CDP client has the same boundary: a browser-level debugging endpoint can read cookies, inject script and control every profile in that browser instance, it has no authentication, and the daemon accepts only loopback `ws://` or `wss://` endpoints. Never expose 9222, 9223 or 9333 either.

Threats considered:

- Other local users: loopback binding plus per-user home permissions.
- Malicious web page: content script never evaluates page-controlled strings as code; all selectors and scripts come from the daemon.
- Extension supply chain: pinned manifest key, signed releases.

Non-goal: protecting the user from their own agent. That is out of scope by design.

## 13. Repository layout

    corvus/
      crates/corvus-domain, corvus-application, corvus-api, corvus-proto,
             corvus-mcp, corvus-transport, corvus-record,
             corvus-cdp, corvusd, corvus
      extension/
        src/background, src/content, src/offscreen, src/shared,
        src/ui/popup, src/ui/options
        manifest.json, vite.background.config.ts, vite.content.config.ts,
        vite.ui.config.ts, package.json
      tests/archive/            superseded playbooks and reports
      deploy/docker/, deploy/k8s/
      packaging/windows/, packaging/systemd/
      docs/
      tests/
      scripts/
      .github/workflows/