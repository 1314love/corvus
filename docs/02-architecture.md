# Corvus Architecture

[English](02-architecture.md) | [中文](02-architecture.zh.md)

Status: 1.5.0 is the current published release, published on 2026-11-15 from tag `v1.5.0`; 1.4.0 is the previous release. The 1.5.0 release keeps the ports-and-adapters backend and the unchanged 43-tool MCP surface, and adds snapshot perception v2, compact snapshot diffs and extension internationalization. The v1.6.0 guardrails implementation is in the tree and passed formal QA on the working tree at HEAD `61ca85f`; its tag, CI, published package and release evidence is Pending. QA PASS for 1.5.0 is recorded in [v1.5.0.md](../tests/acceptance/v1.5.0.md), main CI run 43702442120 passed and GitHub Release 445967405 published the assets. Audience: engineers.

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
    | corvusd (composition root)                                   |
    |  mcp-server  -> application use cases -> channel router       |
    |  rest api    -> /api/v1, /docs, /api-docs/openapi.json        |
    |  ports: BrowserRegistry, BrowserSession, StepRecorder,         |
    |         ToolCatalog; adapters point inward                    |
    |  transport hub (ws / native-messaging / long-poll)            |
    |  cdp client (optional): browser-level WS, Target sessions     |
    |  recording adapter (SeaORM SQLite)   metrics   health         |
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
| corvus-domain | domain model and ports for the 1.4.0 architecture; no I/O, no transport, no storage |
| corvus-application | use cases, the ports `BrowserRegistry`, `BrowserSession`, `StepRecorder` and `ToolCatalog`, the 32 original router specifications shared by MCP and REST, and the `page_snapshot` diff cache |
| corvus-api | REST surface on `/api/v1` with an OpenAPI schema, JWT authentication and argon2 password hashing |
| corvus-mcp | MCP server: tool registry, JSON-RPC, stdio + streamable-http transports |
| corvus-transport | WS server, native-messaging host, long-poll fallback, browser registry |
| corvus-record | session recorder, SeaORM SQLite schema and migration, step store, replay support |
| corvus-cdp | browser-level CDP client, endpoint validation, event ring, raw and high-level CDP tools |
| corvusd | daemon binary: wires everything, service lifecycle, config, optional guardrails policy |
| corvus | CLI: doctor, serve, attach, replay, bench, config print |

The dependency rule points inward: `corvus-domain` has no I/O or adapter dependency, `corvus-application` depends on the domain and defines the ports, adapters implement those ports, and `corvusd` is the only composition root. `corvus-runtime` no longer exists; its routing responsibilities live in `corvus-application` and `corvusd`.

Key crates: tokio, axum, serde, serde_json, sea-orm, sea-orm-migration, sqlx, tracing, tracing-subscriber, tracing-appender, thiserror, clap, uuid.

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
| `browser_type` | `target`; `text`; `clear?` default true; `blur?` default false; `tab_id?` | `typed`, `ref`, `selector`, `tag`, `role`, `cleared`, `events[]`, `value_length`, `url`, `value` (extension only: truncated, or `masked: true` with no value for passwords; CDP omits `value` and `masked`) |
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

### 5.8 Ports-and-adapters backend and REST API (v1.4.0)

v1.4.0 keeps the MCP surface unchanged: 43 tools, nine categories, the compact single-text envelope and `browser_batch` remain exactly as documented in [09-tools.md](09-tools.md). The change is structural plus a second, REST surface over the same application layer.

**Module map.** `corvus-domain` holds the pure domain model, including recording and batch policy types, with no transport, storage or filesystem dependency. `corvus-application` holds use cases and the ports `BrowserRegistry`, `BrowserSession`, `StepRecorder` and `ToolCatalog`; it also keeps the 32 original router specifications. The adapters are `corvus-cdp` (browser-level CDP), `corvus-transport` (extension WebSocket, native messaging and long-poll), `corvus-record` (SeaORM SQLite), `corvus-mcp` (MCP catalogue and JSON-RPC) and `corvus-api` (Axum REST). `corvusd` constructs the adapters and injects them into the application. Dependencies point inward; the domain crate does not depend on any adapter.

**REST routes.** The daemon serves the following routes on the same HTTP port as `/mcp`:

| Method | Path | Result |
|---|---|---|
| GET | `/api/v1/health` | daemon health, version and bound addresses |
| GET | `/api/v1/browsers` | connected browser summaries |
| GET | `/api/v1/browsers/{id}` | one browser summary |
| GET | `/api/v1/sessions` | recorded sessions |
| GET | `/api/v1/sessions/{id}` | one session |
| GET | `/api/v1/sessions/{id}/steps` | recorded steps |
| GET | `/api/v1/tools` | brief tool definitions; `detail=full` is opt-in |
| POST | `/api/v1/tools/call` | `{name, arguments?, browser_id?, session_id?}`; success is `{ok:true,data}` |
| POST | `/api/v1/auth/login` | token pair |
| POST | `/api/v1/auth/refresh` | rotated token pair |
| POST | `/api/v1/auth/logout` | revokes the presented refresh token |
| GET | `/api/v1/auth/me` | current user |

OpenAPI JSON is served at `/api-docs/openapi.json` and Swagger UI at `/docs`. Both are mounted outside the auth middleware. REST uses the same `ToolCatalog` and `ToolBackend` as MCP, so the tool names, schemas, validation and execution path do not fork. `browser_id` is merged into the arguments before dispatch; `session_id` is accepted but is not yet used to annotate the recording. Every response carries `x-request-id`; the router adds tracing, a 120-second timeout that returns 408, gzip compression and CORS restricted to `http://localhost`, `http://127.0.0.1` and `http://[::1]`.

**Authentication.** Auth has two modes. `off` is the default and is allowed only on a loopback bind. `local` enables Argon2id password verification and HS256 JWTs; binding a non-loopback address with auth `off` fails startup unless the operator passes the explicit `--allow-no-auth` / `CORVUS_ALLOW_NO_AUTH` opt-out, which logs an unsafe-configuration warning. A secret shorter than 32 bytes is rejected. There is no default account: when auth is `local`, the supplied username and password bootstrap one admin account at startup. Access tokens last 900 seconds; refresh tokens last 30 days, are stored as Argon2id hashes and rotate on use. Login, refresh and health are exempt from the guard; every other REST route, including `/auth/me` and `/tools/call`, requires a bearer token when auth is enabled. The JWT issuer is `corvus` and the audience is `corvus-api`.

**What authentication does not cover.** The guard is installed on the REST router only. `/mcp`, the extension WebSocket, the legacy `/health`, `/metrics`, `/api-docs/openapi.json` and `/docs` keep their no-authentication behavior. Enabling REST auth therefore does not make a non-loopback MCP deployment safe; put a trusted authenticating proxy in front of any non-loopback listener. Accounts and refresh tokens live in memory in 1.4.0: a restart invalidates every token and recreates the bootstrap account. Persistent account storage is future work.

**Persistence.** The recording adapter uses SeaORM over SQLite and `sea-orm-migration`. `SCHEMA_VERSION` remains 1, and the baseline migration creates the same `sessions` and `steps` tables plus `schema_meta`, so an existing 1.3.0 database opens without a data migration and remains readable by a 1.3.0 binary. For an existing non-empty database that has no migration history, the daemon runs `VACUUM INTO <db>.backup-<millis>.db` before the migration; failure aborts startup. The pool uses one connection, WAL, `synchronous=NORMAL`, foreign keys and a 5-second busy timeout. A single recorder task drains an unbounded write queue; reads and session bookkeeping run on a dedicated record thread. Startup marks previously active sessions `abandoned`, prunes sessions older than 30 days, and truncates stored arguments and results at 262144 bytes on a UTF-8 boundary.

### 5.9 Snapshot perception v2, compact diffs and extension i18n (v1.5.0)

This section describes the released 1.5.0 surface. QA PASS was recorded on the frozen source tree at HEAD 73092ea; the release tag targets `7d0e7c7` on top of product commit `d4d01ea`. Main CI run 43702442120 passed eight jobs and GitHub Release 445967405 published the assets.

**Snapshot perception v2.** `page_snapshot` now traverses open shadow roots and same-origin child frames. The top document has an empty frame path; child frames use document-order paths such as `f0`, `f0.0` and `f1`. Compact lines inside a child frame append `frame="<path>"`, and JSON nodes carry `frame` only when the path is non-empty. When child frames exist the result adds `frames: {total, included, skipped: [{frame, url, reason}]}` with a reason of `cross_origin`, `unavailable` or `geometry_unavailable`; a cross-origin frame is skipped and never read. Bounding boxes are projected into the top viewport, and `max_nodes` is one budget across every included document. On CDP, `snapshot_tool` in `crates/corvus-cdp/src/tools.rs` walks the frame tree and relies on the accessibility tree for open shadow DOM; the extension path in `extension/src/content/runtime.ts` performs the same traversal.

**Compact diffs.** `crates/corvus-application/src/snapshot_diff.rs` keeps a 64-entry LRU keyed by browser, session and tab. `Router::finish_page_snapshot` in `crates/corvus-application/src/router.rs:483` applies it once to compact results, so the extension path, the direct CDP path (`crates/corvusd/src/lib.rs`) and `browser_batch` steps share one grammar. A compact request with `diff: false` refreshes the baseline; diff and JSON requests do not. See [09-tools.md](09-tools.md) for prefixes and reset behavior.

**Extension internationalization.** The Vue UI uses vue-i18n 11.4.20 with 64-key English and Simplified Chinese catalogues under `extension/src/ui/shared/i18n/`. Locale detection uses `chrome.i18n.getUILanguage()` and `navigator.language` (`zh*` maps to `zh-CN`, everything else to English); an explicit choice is persisted under `corvus.locale`, switches reactively without a reload, and sets `document.documentElement.lang`. The manifest declares `default_locale: en` and mirrors the catalogs in `_locales/en` and `_locales/zh_CN`.

The 1.5.0 release is published: annotated tag `v1.5.0` object `a6ddc0f3e7b37f9cc29b0ff8c92eff839e05415f` targets `7d0e7c7`, and GitHub Release 445967405 carries the Windows, Linux and combined-checksum assets. See [v1.5.0.md](../tests/acceptance/v1.5.0.md) and the [release notes](release-notes/v1.5.0.md).

### 5.10 Guardrails policy (v1.6.0)

The v1.6.0 tree adds an opt-in daemon policy in `crates/corvusd/src/guardrails.rs`. A strict version 1 JSON document can force read-only mode, allow or deny tools, restrict `browser_navigate` and `tab_new` to allowed hosts and re-check `browser_navigate`, `tab_new` and `browser_history` result URLs, block literal private-network hosts, redact cookie values, and wrap `page_text`, `page_html` and compact `page_snapshot` text in untrusted-content markers. An invalid policy aborts startup. `RouterBackend::call` checks the policy before dispatch; `browser_batch` checks each step and `session_replay` skips denied steps. Denials use `POLICY_DENIED`, and REST returns 403. This is not a network sandbox: DNS is not resolved and an allowed page can still fetch other origins. Formal QA PASS is recorded at HEAD `61ca85f` in [v1.6.0.md](../tests/acceptance/v1.6.0.md): 237 workspace tests, 137 conformance checks, 55 Vitest tests in ten files, 14 build files at 490,393 B and Playwright 1/1, with one Low `browser_type` channel finding and no High or Medium findings. See [11-guardrails.md](11-guardrails.md).

## 6. Extension design

Three runtime contexts and two small UI surfaces, all built from `extension/src` by three Vite passes into `extension/build/extension`:

1. **service worker**: command dispatch, tabs API, scripting, debugger attach/detach, downloads.
2. **content script**: DOM snapshot, click/type synthesis, waits, mutation observers.
3. **offscreen document**: WebSocket owner (SW cannot hold a socket reliably), keepalive, reconnect loop.

The popup and options page are Vue 3 applications with Pinia and Naive UI; the service worker, content script and offscreen document stay framework-free TypeScript. A Pinia session store is a read-only projection of the daemon-pushed UI state, and the UI talks to the worker through typed RPC. `pnpm build` cleans the output, runs the three passes, copies the manifest last and validates the required files, version, 10 permissions, module service worker and absence of source maps. The UI uses vue-i18n: locale detection reads `chrome.i18n.getUILanguage()` and `navigator.language` (`zh*` -> `zh-CN`, otherwise English), an explicit choice is stored under `corvus.locale`, switching is reactive without a reload and updates `document.documentElement.lang`, and the manifest `_locales` mirror the two catalogs.

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
| POLICY_DENIED | the daemon guardrails policy denied the tool call or navigation | no |
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

The recording store is SQLite accessed through SeaORM, with migrations managed by `sea-orm-migration`. `SCHEMA_VERSION` is 1 in `schema_meta`. The baseline migration creates the tables below and leaves the legacy version marker at 1, so a 1.3.0 database opens without a data migration and a 1.3.0 binary can still read it. Before migrating an existing non-empty database that has no migration history, the daemon writes `VACUUM INTO <db>.backup-<millis>.db`; a failed backup aborts startup. The pool holds one connection with WAL, `synchronous=NORMAL`, foreign keys and a 5-second busy timeout, a single recorder task drains the write queue, and stored arguments and results are truncated at 262144 bytes on a UTF-8 boundary.

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
| Auth mode | `--auth`, `CORVUS_AUTH` | `off`; `local` for JWT auth |
| Auth secret | `--auth-secret`, `CORVUS_AUTH_SECRET` | none; at least 32 bytes when auth is `local` |
| Bootstrap username | `--auth-username`, `CORVUS_AUTH_USERNAME` | none; required when auth is `local` |
| Bootstrap password | `--auth-password`, `CORVUS_AUTH_PASSWORD` | none; required when auth is `local` |
| Non-loopback without auth | `--allow-no-auth`, `CORVUS_ALLOW_NO_AUTH` | `false`; explicit opt-out, logs a warning |

Recording is always on and keeps the fixed 30-day retention. `corvus config --client <name> --transport stdio|http [--home <dir>]` prints a ready `mcpServers` JSON object on stdout, with a location hint on stderr; the stdio form pins `--home` when supplied, and the HTTP form points at `http://127.0.0.1:7331/mcp`.

Per-tool timeout overrides: `browser_wait` gets `args.timeout_ms + 5000` ms capped at 125000 ms, `browser_eval` gets 30000 ms, `download_wait` gets `args.timeout_ms + 5000` ms capped at 125000 ms, and every other tool gets the 60000 ms default; the router clamps every deadline to the 125000 ms ceiling.

## 11. Build and CI

- cargo workspace, edition 2021, Rust 1.90 as MSRV in `Cargo.toml` and `rust-toolchain.toml`.
- CI matrix: windows-latest, macos-latest and ubuntu-latest; each runs `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`.
- Packaging jobs: `scripts/package-linux.sh` builds the x86_64 tar.gz and verifies the systemd unit; a Docker job builds the image, checks the reported version and `/health`, and asserts the 43-tool list.
- Extension: TypeScript and Vue 3 sources under `extension/src`, bundled by three Vite passes into `extension/build/extension` (`pnpm install && pnpm build`); Vitest covers 41 unit, contract and component tests in eight files, and Playwright runs an isolated end-to-end suite; manifest key pinned so the extension id is stable across machines.
- REST, OpenAPI and auth: router and auth tests run in the workspace suite; `/api-docs/openapi.json` and `/docs` are covered by the API tests.
- Release: platform packages with `SHA256SUMS`; macOS has no packaged installer in 1.5.0.

## 12. Security posture

Deliberate: no permission prompts, no allow-list and no per-action confirmation. The MCP server, the extension WebSocket and the CDP client have no authentication or pairing secret. The REST `/api/v1` surface adds an optional `local` auth mode with Argon2id passwords and HS256 JWTs. A non-loopback bind fails startup unless that mode is enabled or the operator explicitly opts out with `--allow-no-auth` / `CORVUS_ALLOW_NO_AUTH`, which logs a warning. The guard covers REST only, so `/mcp`, the extension WebSocket, `/health`, `/metrics`, `/api-docs/openapi.json` and `/docs` remain unauthenticated. REST auth therefore does not make a non-loopback MCP deployment safe; use a trusted authenticating proxy in front of any non-loopback listener. The trust boundary remains the OS user account: browser transports and the MCP HTTP server bind to loopback by default, and home-root file permissions protect the recording database. Any local process that can reach ports 7330 or 7331 can drive the connected browser, so never expose those ports beyond loopback. The optional CDP client has the same boundary: a browser-level debugging endpoint can read cookies, inject script and control every profile in that browser instance, it has no authentication, and the daemon accepts only loopback `ws://` or `wss://` endpoints. Never expose 9222, 9223 or 9333 either. The optional guardrails policy is defense in depth, not a network sandbox: it gates tools and navigation arguments but does not resolve DNS or prevent an allowed page from fetching other origins.

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
        src/ui/popup, src/ui/options, src/ui/shared/i18n,
        _locales/en, _locales/zh_CN
        manifest.json, vite.background.config.ts, vite.content.config.ts,
        vite.ui.config.ts, package.json
      tests/archive/            superseded playbooks and reports
      deploy/docker/, deploy/k8s/
      packaging/windows/, packaging/systemd/
      docs/
      tests/
      scripts/
      .github/workflows/