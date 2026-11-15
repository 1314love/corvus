# Corvus Requirements Specification v0.1

[English](01-requirements.md) | [中文](01-requirements.zh.md)

Audience: product owner and implementers. Engineering detail lives in [02-architecture.md](02-architecture.md).

## 1. Users and scenarios

**Profile**: one machine, one user, always on. The user hands the browser to an AI and expects it to behave like a
person using the browser, not like a robot asking for permission before every click.

**Typical scenarios**

1. **Long delegated tasks**: the AI works through a dozen pages, crosses login states, downloads files and fills
   forms. No confirmation dialog may interrupt it, and a sleeping extension must not break it.
2. **Several browsers in parallel**: inside one task, Chrome researches, Edge is logged into the enterprise
   system, Firefox verifies front-end compatibility.
3. **After-the-fact review**: when a task fails, replay the whole session frame by frame and see what the AI saw,
   what it clicked, what the network returned.
4. **Reusable capability**: the same tool surface serves Claude Code, Cursor, DSH and Cline, so switching client
   does not mean relearning muscle memory.

## 2. Terms

| Term | Meaning |
|---|---|
| daemon | the corvusd process: MCP server and command routing centre |
| extension | the browser-side MV3 extension: no pages, only a read-only status popup |
| transport | a channel between daemon and extension: WS / Native Messaging / direct CDP |
| session | one recordable sequence of operations: commands, responses, page snapshots, network events |
| site card | verified selectors, flows and pitfalls for one domain, stored in SQLite |
| tool | one function exposed to an MCP client |

## 3. Functional requirements

### 3.1 Connection and lifecycle (FR-C)

| ID | Requirement | Priority | Acceptance |
|---|---|---|---|
| FR-C1 | After installation the extension discovers the local daemon automatically; no pairing code, no clicks | P0 | cold start to first command < 60s, zero manual clicks |
| FR-C2 | Three channels with automatic downgrade: WS -> Native Messaging -> extension-side long poll | P0 | after port 3001 is blocked, switches within 3s and resumes commands |
| FR-C3 | After a daemon restart the extension reconnects and unfinished commands replay by idempotency key | P0 | kill and restart the daemon; in-flight commands eventually succeed and never execute twice |
| FR-C4 | Watchdog: recovers after service-worker sleep, extension reload, browser upgrade | P0 | 72h soak with no human intervention |
| FR-C5 | Several browsers and profiles online at once, routed by browserId | P0 | 3 browsers execute commands concurrently without interference |
| FR-C6 | Each browser can be disabled and enabled individually | P1 | after disabling, tool calls to that browser return a specific error code immediately |

### 3.2 Perception (FR-P)

| ID | Requirement | Priority | Acceptance |
|---|---|---|---|
| FR-P1 | Hybrid snapshot: accessibility tree plus trimmed DOM, with stable reference ids | P0 | complex page (GitHub repository page) snapshot < 300ms, references usable directly in click |
| FR-P2 | Text / HTML / links / form structure extraction | P0 | matches browser DevTools output |
| FR-P3 | Screenshots: viewport, element, full page; DPR and WebP support | P0 | full-page capture succeeds on a 20000px page |
| FR-P4 | Console logs, uncaught exceptions, network waterfall | P0 | timestamps match DevTools within 50ms |
| FR-P5 | Wait primitives: element appears/disappears, text appears, network idle, JS condition true | P0 | every wait accepts a timeout and returns a structured failure reason |
| FR-P6 | Page change subscription: DOM mutations, navigation, download started | P1 | changes pushed to clients as MCP notifications |

### 3.3 Actions (FR-A)

| ID | Requirement | Priority | Acceptance |
|---|---|---|---|
| FR-A1 | Navigate, back, forward, reload, history preserved | P0 | single-page-app route changes are detected |
| FR-A2 | Click with a native event sequence (pointer/mouse/touch) | P0 | works on GitHub buttons, menus and SVG icons |
| FR-A3 | Typing: native setter plus InputEvent for controlled components; clear, append, per-character | P0 | React / Vue / Angular forms do not roll back |
| FR-A4 | Keyboard: chords, shortcuts, per-key sequences | P0 | Ctrl+A / Enter / Escape / Tab take effect in order |
| FR-A5 | Hover, drag, select, scroll (including infinite scroll) | P0 | both HTML5 drag and canvas drag work |
| FR-A6 | Upload (input and drag path) and download (wait for completion, return local path) | P0 | a 100MB upload succeeds; download completion is awaitable |
| FR-A7 | Evaluate JS in page world and isolated world, returning values and exceptions | P0 | can read window variables and call page functions |
| FR-A8 | Semantic location: find elements by role, text, label, placeholder instead of brittle CSS | P1 | location success >= 90% after a site redesign |
| FR-A9 | Highlight and debug helpers: temporary outline for screenshot evidence | P2 | highlighting does not affect layout |

### 3.4 Tabs and windows (FR-T)

| ID | Requirement | Priority | Acceptance |
|---|---|---|---|
| FR-T1 | List, create, close, switch, duplicate tabs | P0 | closing the current tab switches to the neighbour and returns the new id |
| FR-T2 | Tab grouping and batch operations | P1 | five tabs can be grouped and the group closed as a unit |
| FR-T3 | Concurrent command batches across tabs | P0 | 10 tabs navigate in parallel with no cross-talk |
| FR-T4 | Window-level operations: new window, fullscreen, bring to front | P2 | bringing to front does not disturb what the user is typing |

### 3.5 Storage and identity (FR-S)

| ID | Requirement | Priority | Acceptance |
|---|---|---|---|
| FR-S1 | Read, write and clear cookies with domain, path and SameSite filters | P0 | written cookies survive a page reload |
| FR-S2 | Key-level access to localStorage / sessionStorage / IndexedDB | P1 | objects larger than 1MB can be read and written |
| FR-S3 | Inject custom request headers (User-Agent, Referer, authorization) | P1 | injected headers are visible server-side |
| FR-S4 | Login-state snapshots: export and import a site's cookie set | P2 | after importing on another machine, no login is required |

### 3.6 Network (FR-N)

| ID | Requirement | Priority | Acceptance |
|---|---|---|---|
| FR-N1 | Capture requests and responses with body (configurable cap), timing and status | P0 | matches the DevTools Network panel entries |
| FR-N2 | Intercept and rewrite requests (mock responses, modified bodies) | P1 | rules support URL regex and priority |
| FR-N3 | Blocking and throttling (offline, 3G, slow network) | P1 | page load time under throttling falls in the expected range |
| FR-N4 | Wait for a specific request to finish (URL match) | P0 | exposed as one of the FR-P5 wait primitives |

### 3.7 Debugging and emulation (FR-D)

| ID | Requirement | Priority | Acceptance |
|---|---|---|---|
| FR-D1 | Debugger channel: CDP domains (DOM, Network, Runtime, Emulation) | P1 | always advertised, attached on demand; CAPABILITY_UNAVAILABLE only when the browser refuses the attach |
| FR-D2 | Device emulation: viewport, DPR, touch, UA | P1 | matchMedia results are correct after emulation |
| FR-D3 | Geolocation, timezone, language, dark mode emulation | P2 | corresponding navigator fields change together |
| FR-D4 | PDF export separate from full-page screenshots | P2 | the exported file opens in a PDF reader |

### 3.8 Observability and memory (FR-O)

| ID | Requirement | Priority | Acceptance |
|---|---|---|---|
| FR-O1 | Session recording: commands, responses, snapshots, screenshots, network events on one timeline | P0 | recording cost < 5% CPU |
| FR-O2 | Replay: step forward and back, jump to any command | P0 | a 1000-step session replays without UI stalls |
| FR-O3 | Export: JSONL / HTML report / Perfetto trace | P1 | reports open offline |
| FR-O4 | Site memory: successful locators and flows stored in SQLite and reused first | P1 | second task on the same site needs >= 20% fewer steps |
| FR-O5 | Metrics: Prometheus endpoint with command latency, success rate, connection state | P2 | scrapeable by Grafana directly |

### 3.9 System and integration (FR-I)

| ID | Requirement | Priority | Acceptance |
|---|---|---|---|
| FR-I1 | MCP server: stdio and streamable-http dual stack following MCP 2025-06-18 | P0 | at least 3 clients connect successfully |
| FR-I2 | Tool descriptions optimised for LLMs: short, with examples and error codes | P0 | first-call success >= 95% |
| FR-I3 | Client config generation: one command prints snippets for Claude/Cursor/DSH/Cline | P1 | copy, paste, run |
| FR-I4 | Health and self-check: doctor reports environment, permissions, ports, extension state | P0 | diagnoses 90% of installation problems on a fresh machine |
| FR-I5 | Single-binary distribution with no runtime dependency | P0 | one executable each for Windows/macOS/Linux |

## 4. MCP tool matrix (target >= 60)

The delivered surface is 43 tools in nine categories as of 1.6.0, with `browser_screenshot` as an unreleased 44th tool in the 1.7.0 tree; the names below are the original target matrix, not the shipped catalogue.

Perception: page_snapshot, page_text, page_html, page_screenshot, page_pdf, page_console, page_network, page_accessibility, page_links, page_forms, page_find, page_wait, page_observe

Action: browser_navigate, browser_back, browser_forward, browser_reload, browser_click, browser_type, browser_press, browser_hover, browser_drag, browser_select, browser_scroll, browser_focus, browser_upload, browser_download, browser_eval, browser_highlight

Tabs and windows: tab_list, tab_new, tab_close, tab_switch, tab_group, tab_duplicate, window_list, window_new

Storage and identity: cookie_get, cookie_set, cookie_clear, storage_get, storage_set, storage_clear, header_set

Network: net_capture_start, net_capture_stop, net_list, net_wait, net_mock, net_block, net_throttle

Emulation and debugging: emulate_device, emulate_geo, emulate_timezone, emulate_media, debug_attach, debug_detach

Sessions and memory: session_start, session_stop, session_replay, session_export, trace_start, trace_stop, site_card_get, site_card_put

System: browser_list, browser_attach, browser_detach, extension_status, health, doctor, version

### 4.1 M2 wave 1 traceability

M2 wave 1 ships all 18 tools. The following requirements have an implemented tool path in that surface; partial coverage is stated explicitly, and everything not listed here remains open.

- **FR-P1** — `page_snapshot` produces the hybrid tree with stable references, and `page_find` returns references in the same space.
- **FR-P2** — `page_text` and `page_html` cover text and HTML extraction. Link and form-structure extraction remain open.
- **FR-P5** — `browser_wait` covers selector, text and URL conditions with a timeout. Disappearance, network-idle and JS-condition waits remain open.
- **FR-A1** — `browser_navigate` and `browser_history` cover navigate, back, forward and reload.
- **FR-A4** — `browser_press` covers modifier chords, named keys and per-key sequences.
- **FR-A5** — `browser_hover`, `browser_select` and `browser_scroll` cover hover, select and scroll. Drag remains open.
- **FR-A7** — `browser_eval` covers page-world and isolated-world evaluation with structured exceptions.
- **FR-A8** — `page_find` covers text and role lookup, and matches accessible labels and placeholders for form controls.
- **FR-T1** — `tab_list`, `tab_new`, `tab_close` and `tab_switch` cover list, create, close and switch. Duplicate remains open.

### 4.2 M2 wave 2 traceability

M2 wave 2 added eight tools. The following requirements now have an implemented path; partial coverage is stated explicitly. Everything not listed remains open.

- **FR-C5** — `browser_list` reports the live browser set, and every browser-facing tool accepts `browser_id` to route one command to one connected browser. The routing failures are `BAD_ARGS` with `details.reason` `multiple_browsers` when several browsers are connected and no id is given, and `TRANSPORT_DOWN` when none is connected or the id is unknown. Per-browser disable and enable (FR-C6) remain open.
- **FR-N1** — `net_capture` and `net_log` cover request method, URL, status, MIME, resource type, cache flag, duration, errors and optional headers from a 2000-entry per-tab ring. Response bodies remain open.
- **FR-N2** — `net_intercept` covers URL-regex rules with block, mock and continue actions; mock sets status, headers and a base64 body, and the first matching rule wins. Request-body rewriting remains open.
- **FR-N3** — the `block` action covers request blocking. Throttling and offline emulation remain open.
- **FR-A6** — `download_list` and `download_wait` cover download tracking and completion; `upload_file` covers file-input upload through CDP, including hidden inputs. Drag-path upload and a returned local download path remain open.

### 4.3 M3 wave 1 traceability

M3 wave 1 adds six tools for a total of 32. The following requirements now have an implemented path; partial coverage is stated explicitly. Everything not listed remains open.

- **FR-O1** — Every `tools/call` in an MCP session is recorded with tool, arguments, result, error code, duration and resolved `browser_id`/`tab_id`; `session_list` and `session_steps` read the timeline. Snapshots, screenshots and network events as separate timeline rows remain open.
- **FR-O2** — `session_replay` plans a recorded range in `dry_run` and re-executes it in `live`, skipping control and tab tools, stopping on the first error unless `continue_on_error` is set, and recording the run as a replay session. Interactive step forward/back control remains open.
- **FR-O3 (partial)** — `session_export` writes JSONL only; HTML report and Perfetto trace remain open.
- **FR-O5 (partial)** — `metrics`, `logs_tail` and `GET /metrics` expose counters, a 5000-entry log ring and Prometheus text; a Grafana dashboard remains open.

Still open: site memory (FR-O4), the remaining FR-O1, FR-O2, FR-O3 and FR-O5 scope above, and the storage and identity family (FR-S1–FR-S4). Request and network-idle waits (FR-N4, FR-P5) also remain open.

### 4.4 M4 wave 1 traceability

M4 wave 1 adds no tools. It adds the scope gate and MCP cancellation to the existing 32-tool surface.

- **FR-T3** — Calls with different explicit `tab_id` values run in parallel lanes, while calls on the same browser tab are ordered, so ten tab-scoped actions can run concurrently without cross-talk. Concurrency is expressed as independent calls; no separate batch primitive is added in this wave.
- **FR-A1–FR-A8** — Any in-flight action `tools/call` can be cancelled with `notifications/cancelled`: the request ends with JSON-RPC error `-32800`, and `session_steps` records `ok: false` with `error_code` `"CANCELLED"`. Abort is cooperative for waits, downloads, evaluation and navigation; other actions may finish late, and effects already applied are not rolled back.
- **FR-O1** — The recording timeline now records cancelled attempts with `error_code` `"CANCELLED"`. Snapshot, screenshot and network-event rows remain open.

Still open: no batch scheduling API for FR-T3; cancellation is cooperative and does not roll back applied effects; FR-O1 still lacks snapshot, screenshot and network timeline rows. The remaining M2 and M3 gaps in sections 4.1-4.3 are unchanged.

### 4.5 v1.1.0 deployment traceability

v1.1.0 is a deployment-release slice. It adds no MCP tools and keeps the 32-tool surface unchanged.

- **Single-root layout** — Every backend file now lives under `--home <dir>` or `CORVUS_HOME`: `bin/`, `data/corvus.db`, `logs/`, `config/` and `extension/`. The database resolves as `--db`, then the configured home root, then `CORVUS_DB_PATH`, then the platform default. `doctor` reports the active database path, schema version, session and step counts. This is the operational foundation for FR-S1-FR-S4, not their completion; the storage and identity family remains open.
- **NFR-5 (partial)** — The Windows installer uses one `%ProgramData%\Corvus` root and the Linux package uses `/opt/corvus` with a systemd unit. Linux packaging is x86_64 only. macOS still has no packaged installer in this release; arm64 Linux packages remain open.
- **Deployment packaging** — A Docker amd64 image and Kubernetes manifests put the same single root at `/data`. Packaging and container checks run in CI; they are not a hosted service.
- **NFR-3 (not completed)** — The clean 72-hour soak started 2026-11-14T14:41:37Z and was stopped at the client's request after about eight hours (last checkpoint 8 at 2026-11-14T22:41:37Z). The planned 72-hour run was not completed. Shorter soak runs are evidence only. NFR-3 is not passed and 1.1.0 is not 72-hour soak certified.
- **NFR-6 (recorded deviation)** — Firefox remains unsupported. The supported browsers are Edge 120+, Chrome 120+ and current Brave; Firefox support is not claimed by this release.

### 4.6 v1.2.0 hybrid transport traceability

v1.2.0 adds ten tools, including eight CDP-backed tools, taking the MCP surface from 32 to 42. The original 32 tools keep their arguments and result shapes.

- **Extension CDP detection** — The extension probes loopback ports 9222, 9223 and 9333, reads `/json/version`, and prefers the same browser family. It advertises the `cdp` capability only when a valid endpoint is found; the handshake carries the endpoint, browser, protocol version and family for the daemon to re-validate.
- **Direct CDP client** — The daemon keeps one long-lived browser-level WebSocket, uses flattened `Target` sessions for page work, retains a 1000-event ring, reconnects with 250 ms to 10 s backoff, heartbeats every 15 s, and uses a 10 s default request timeout with a 60 s maximum.
- **Channel selection** — `auto`, `cdp` and `extension` are selectable at startup (`--channel`, `CORVUS_CHANNEL`) and at runtime (`browser_channel`). `auto` prefers CDP and falls back to the extension only for an `Unsupported` CDP result; `cdp` is strict and never falls back; `extension` ignores CDP. `--no-cdp` removes the CDP path entirely.
- **Fallback and tool availability** — Eight tools are extension-only, eight are CDP-only, seventeen are shared and nine are daemon-local. A forced channel fails the tools it cannot run with `CAPABILITY_UNAVAILABLE` rather than silently switching.
- **Raw and high-level CDP** — `cdp_send` and `cdp_events` expose the protocol and the event ring directly; `browser_console`, `browser_cookies`, `browser_dialog` and `browser_emulate` cover the common cases without raw protocol calls. `tool_help` describes any tool on demand.
- **MCP categories and token budget** — `tools/list` now describes two levels: a brief default of 29,971 bytes for 42 tools, and a full detail mode of 35,552 bytes. `tool_help` carries the long descriptions so routine listings stay small.
- **NFR-3 (not completed)** — The clean 72-hour soak was stopped at the client's request after about eight hours. NFR-3 is not passed and 1.2.0 is not 72-hour soak certified.
- **NFR-6 (recorded deviation, unchanged)** — Firefox remains unsupported. Supported browsers are Edge 120+, Chrome 120+ and current Brave; Firefox is not claimed by this release.

### 4.7 v1.3.0 token-efficiency traceability

v1.3.0 adds `browser_batch` for 43 tools across nine categories and reduces default context cost without changing the original 32 result shapes.

- **Compact snapshots** — `page_snapshot` defaults to compact text on the extension and CDP channels; `format: "json"` keeps the object form. Compact refs are `@eN` on extension and `@c<backendNodeId>` on CDP. `max_nodes`, `viewport_only`, `include_bbox` and the line grammar are documented in [09-tools.md](09-tools.md).
- **Batched calls** — `browser_batch` executes up to 20 `{tool,args}` steps in one daemon-local call, stops on the first failure by default, and excludes recursion and daemon-control tools.
- **Lean discovery** — brief `tools/list` is 20,452 bytes for 43 tools; `detail: "full"` is 37,067 bytes; `tool_help` returns one full entry. A successful `tools/call` returns one compact JSON text block and no `structuredContent`.
- **CDP parity** — forced CDP serves all 33 browser-facing tools, including the eight former extension-only tools; the extension-only tool count is now zero. Cross-channel references fail with typed `BAD_ARGS`.
- **NFR-3 (not completed)** — 1.3.0 is not 72-hour soak certified; the 72-hour run remains incomplete after the earlier stop at about eight hours. **NFR-6 (unchanged)** — Firefox remains unsupported; supported browsers are Edge 120+, Chrome 120+ and current Brave.

### 4.8 v1.4.0 structural and REST traceability

v1.4.0 adds no MCP tools: the 1.3.0 surface of 43 tools in nine categories, the compact single-text envelope and `browser_batch` are unchanged. The release restructures the backend and adds a separate REST surface.

- **Ports and adapters.** `corvus-domain` holds the pure domain model with no I/O. `corvus-application` holds use cases and the ports `BrowserRegistry`, `BrowserSession`, `StepRecorder` and `ToolCatalog`, plus the 32 original router specifications. `corvus-cdp`, `corvus-transport`, `corvus-record`, `corvus-mcp` and `corvus-api` are adapters; `corvusd` is the composition root. `corvus-runtime` is removed and its routing responsibilities move into the application crate and `corvusd`. Dependencies point inward: domain has no adapter dependency.
- **REST API.** The daemon serves `/api/v1/health`, `/api/v1/browsers`, `/api/v1/browsers/{id}`, `/api/v1/sessions`, `/api/v1/sessions/{id}`, `/api/v1/sessions/{id}/steps`, `/api/v1/tools` and `/api/v1/tools/call`, plus the auth routes. OpenAPI is at `/api-docs/openapi.json` and Swagger UI at `/docs`. REST uses the same 43-tool `ToolCatalog` and `ToolBackend` as MCP. Every request has an `x-request-id`, tracing, a 120-second timeout returning 408, gzip compression and CORS limited to loopback origins; errors are typed.
- **Authentication.** Auth is off by default on loopback. A non-loopback bind with auth off fails startup unless the operator passes the explicit `--allow-no-auth` / `CORVUS_ALLOW_NO_AUTH` opt-out, which logs an unsafe-configuration warning; no default account exists. `--auth local` enables Argon2id password verification and HS256 JWTs: access tokens last 900 seconds, refresh tokens last 30 days and rotate on use. `/api/v1/auth/login`, `/refresh`, `/logout` and `/me` implement the flow; `/api/v1/health`, login and refresh are unauthenticated. The REST auth guard wraps `/api/v1` only and does not cover the existing `/mcp` endpoint, the extension WebSocket, legacy `/health`, `/metrics`, `/api-docs/openapi.json` or `/docs`; all of those keep their 1.3.0 no-authentication behavior. Accounts and refresh tokens are held in memory in 1.4.0 and do not survive a restart; persistent auth storage is future work.
- **Persistence.** `corvus-record` uses SeaORM and `sea-orm-migration` with `schema_version` 1. A 1.3.0 database opens without a data migration; before the baseline migration runs on an existing file the daemon writes a `VACUUM INTO` backup, and a failed backup aborts startup. The store keeps WAL, a single writer connection, a 5-second busy timeout, 30-day session retention and 262144-byte payload truncation.
- **Extension.** The extension is built from TypeScript and Vue 3 sources with three Vite passes, producing the loadable `extension/build/extension` tree. The popup and options page use Vue 3, Pinia and Naive UI; the service worker, content script and offscreen document stay framework-free TypeScript with typed RPC. The MV3 manifest declares ten permissions and an ESM service worker, and the build checks the expected files and manifest shape.
- **NFR-3 (not completed) and NFR-6 (unchanged).** 1.4.0 is not 72-hour soak certified; the planned run remains incomplete after the earlier stop at about eight hours. Firefox remains unsupported; supported browsers are Edge 120+, Chrome 120+ and current Brave.

### 4.9 v1.5.0 perception and i18n traceability

v1.5.0 adds no MCP tools: the 43-tool surface in nine categories is unchanged. It changes snapshot perception and the extension UI.

- **Perception v2.** `page_snapshot` traverses open shadow roots and same-origin child frames, adds document-order frame paths and `frames` metadata, never reads cross-origin frame DOM, applies one `max_nodes` budget across included documents and projects boxes into the top viewport.
- **Compact diffs.** `diff` is compact-only and computed once by the application layer from a 64-entry cache keyed by browser, session and tab; `browser_batch` shares the same path.
- **Extension i18n.** The Vue UI uses vue-i18n with English and zh-CN catalogs, detects and persists the locale and switches reactively; the manifest declares `_locales`.
- **Evidence.** [tests/acceptance/v1.5.0.md](../tests/acceptance/v1.5.0.md) records PASS for 220 workspace tests, 136 conformance checks, 41 Vitest tests in eight files and Playwright 1/1; CI run 43702442120 and GitHub Release 445967405 published the assets. NFR-3 remains unpassed.

### 4.10 v1.6.0 guardrails traceability

v1.6.0 adds no MCP tools: the 43-tool surface in nine categories and the compact envelope are unchanged; brief `tools/list` is 20,493 bytes and `detail: "full"` is 37,496 bytes. It adds an optional local policy enforced by the daemon.

- **Policy file.** Strict JSON, at most 65,536 bytes, `version` exactly 1, no unknown fields; optional `read_only`, `allow_tools`, `deny_tools`, `allowed_hosts`, `block_private_network`, `redact_secrets` and `content_boundary`. Load with `--guardrails <PATH>` or `CORVUS_GUARDRAILS`; any invalid policy aborts startup.
- **Enforcement.** Checks run before dispatch in `RouterBackend::call`, in every `browser_batch` step and in `session_replay`; `browser_navigate` and `tab_new` arguments are gated before dispatch and re-checked on navigation results. Denials use `POLICY_DENIED` as the 11th error code with `retryable: false`, and REST returns 403.
- **Content handling.** `browser_cookies` redacts values; `page_text`, `page_html` and compact `page_snapshot` text are wrapped in untrusted-content boundaries; password inputs stay masked.
- **Evidence.** [tests/acceptance/v1.6.0.md](../tests/acceptance/v1.6.0.md) records PASS for 237 workspace tests, 137 conformance checks, 55 Vitest tests in ten files, a 14-file/490,393-byte extension build and Playwright 1/1, with one Low `browser_type` finding and no High or Medium. CI run 43716832915 and GitHub Release 446071476 published the assets. NFR-3 remains unpassed.

## 5. Non-functional requirements

| ID | Category | Requirement |
|---|---|---|
| NFR-1 | Latency | local click / type p95 < 50ms; snapshot p95 < 300ms (10k-node page) |
| NFR-2 | Concurrency | 10 tabs running commands in parallel, 10 MCP clients connected at once |
| NFR-3 | Stability | 72h soak, command success >= 99.9%, memory growth < 50MB |
| NFR-4 | Size | single binary < 25MB; extension bundle < 1MB |
| NFR-5 | Platforms | Windows 10+, macOS 13+, Linux x64/arm64 |
| NFR-6 | Browsers | Edge 120+, Chrome 120+, current Brave, Firefox 121+ |
| NFR-7 | Maintainability | Rust workspace split into crates, zero clippy warnings, three-platform CI |
| NFR-8 | Observability | structured JSON logs, adjustable level, never log page content by default |
| NFR-9 | Security posture | no approvals, no allow-list, no sandbox; runs as the OS user; MCP, the extension WebSocket and CDP bind loopback only; REST may bind a non-loopback address only with local auth or the explicit unsafe opt-out |
| NFR-10 | Compatibility | MCP 2025-06-18, complete tool annotations (readOnly / destructive / idempotent) |

## 6. Acceptance tasks (real scenarios; M2 is done only when all pass)

1. Create a repository on GitHub and add a README.
2. Search Bing for "browser automation MCP" and summarise the first five results.
3. Log into an enterprise system, fill a 20-field form and submit it.
4. Download an attachment from a mail page and report where it landed.
5. Open three browsers at once, navigate and screenshot in each.
6. Extract a table into JSON with browser_eval.
7. Intercept an API call, mock the response, and verify the page re-renders accordingly.
8. Record a 50-step task, export the report, replay it on another machine.
9. Disconnect the network for 10 seconds, restore it, and have the in-flight command succeed.
10. Connect Claude Code and DSH on the same machine and issue commands alternately without interference.

## 7. Open questions

1. Does Firefox enter M1? (WebExtension differences are large; recommendation: M2.)
2. Does site memory need cloud sync? (Recommendation: local first, keep an export format.)
3. Is recording on by default? (Recommendation: on by default, 30-day retention, can be disabled.)
