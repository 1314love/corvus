# Corvus Architecture

[English](02-architecture.md) | [中文](02-architecture.zh.md)

Status: design draft, M0. Audience: engineers.

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
- Long outputs are paginated: first response returns a summary plus a cursor; the agent pulls the rest.
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

## 6. Extension design

Three contexts, no pages:

1. **service worker**: command dispatch, tabs API, scripting, debugger attach/detach, downloads.
2. **content script**: DOM snapshot, click/type synthesis, waits, mutation observers.
3. **offscreen document**: WebSocket owner (SW cannot hold a socket reliably), keepalive, reconnect loop.

The popup exists only to toggle the debugger channel. It is a 40-line page with one switch and one status dot.

Reconnect policy: exponential backoff with jitter (250ms -> 8s cap), immediate retry on network-online, alarm every 30s as a floor, and a daemon-push "resume" message when the daemon restarts.

## 7. Reliability model

    extension states: IDLE -> CONNECTING -> READY -> DEGRADED -> IDLE

- DEGRADED means: attached but missing capabilities (e.g. debugger channel off). Tools that need the missing capability return CAPABILITY_UNAVAILABLE instead of hanging.
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
| CAPABILITY_UNAVAILABLE | e.g. debugger off, permission missing | no, until toggled |
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
    default_timeout_ms = 15000

    [memory]
    enabled = true
    export_format = "jsonl"

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