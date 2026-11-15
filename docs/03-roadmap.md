# Corvus Roadmap

[English](03-roadmap.md) | [中文](03-roadmap.zh.md)

## Milestones

| Phase | Goal | Key deliverables | Exit criterion |
|---|---|---|---|
| M0 Charter | Write down what we build | charter, requirements, architecture, roadmap, empty repo with CI | four documents reviewed |
| M1 Skeleton | Prove one real path end to end | corvusd + extension + MCP (stdio/http) + navigate/snapshot/click/type | one real web task completed in DSH |
| M2 Full surface | Fill the capability matrix | 60+ tools, multi-browser, tab concurrency, request interception, upload/download | the 10 acceptance tasks in the requirements spec pass |
| M3 Observability | Make everything reviewable | recording, trace, replay, site memory, Prometheus | any historical session replays step by step |
| M4 Ecosystem | Make it adoptable | installers, client config generators, docs site, examples, release pipeline | a stranger completes the quick start in 5 minutes |
| M6 Hybrid CDP | Attach directly when a browser exposes a loopback CDP endpoint | CDP client, extension detection, channel routing, ten CDP-backed tools | 42 tools, auto/cdp/extension switchable, no regression to the original 32 |
| M7 Token efficiency | Make the tool surface cheap enough for long agent sessions | compact page snapshots, lean brief schemas, browser_batch, full CDP parity | 43 tools, brief tools/list under 30 KB, compact snapshot measured |

## v1.1.0 deployment milestone

v1.1.0 is a deployment slice of M4. The 32-tool surface is unchanged.

| Item | Exit criterion |
|---|---|
| Single root | `--home <dir>` or `CORVUS_HOME` holds binaries, the database, logs, config and the extension; `doctor` reports the active database path, and `--db` still overrides it. |
| Windows | The installer defaults to `%ProgramData%\Corvus`, migrates a 1.0.0 `%USERPROFILE%\.corvus` tree, and keeps the service on one root. |
| Linux | The x86_64 tarball carries `install-linux.sh`, a systemd unit, upgrade/uninstall scripts and `SHA256SUMS`. |
| Containers | The amd64 Docker image and Kubernetes manifests mount one `/data` root; CI builds and validates them. |
| Release | 1.1.0 binaries, changelog and bilingual release notes. NFR-3 stays pending until the 72h soak report is attached; Firefox remains unsupported. |

## v1.2.0 hybrid transport milestone

v1.2.0 is a tool-surface and transport slice. It adds ten CDP-backed tools, taking the MCP surface from 32 to 42 without changing the original result shapes.

| Item | Exit criterion |
|---|---|
| Detection | The extension probes loopback 9222, 9223 and 9333, rejects family mismatches, and advertises the cdp capability only with a valid descriptor. |
| Direct client | The daemon holds one browser-level WebSocket, uses Target sessions, keeps a 1000-event ring, reconnects with 250 ms to 10 s backoff and heartbeats every 15 s. |
| Channels | auto, cdp and extension are selectable at startup and at runtime; auto falls back only on Unsupported, cdp never falls back, extension ignores CDP. |
| Tool surface | 42 tools in total; brief tools/list is 29,971 bytes and full detail is 35,552 bytes; the original 32 keep their arguments and results. |
| Release | 1.2.0 binaries, changelog and bilingual release notes. CI and QA evidence is attached only when the runs exist; NFR-3 remains pending and Firefox remains unsupported. |

Implementation and CI checks for this milestone run before the release tag. The 72-hour soak is still not passed, and the Firefox NFR-6 deviation is unchanged.

## v1.3.0 token-efficiency milestone

v1.3.0 is a context-efficiency and parity slice. It adds `browser_batch`, taking the MCP surface from 42 to 43, and makes the default discovery and snapshot output smaller.

| Item | Exit criterion |
|---|---|
| Compact snapshots | `page_snapshot` defaults to compact text on both channels; `format: "json"` keeps node objects; `viewport_only` and `include_bbox` filter and annotate. |
| Lean discovery | Brief `tools/list` removes property descriptions and titles; `detail: "full"` restores them; `tool_help` returns one complete entry. |
| Batching | `browser_batch` runs up to 20 allowlisted browser steps with `stop_on_error`; it is daemon-local. |
| CDP parity | Forced CDP serves all 33 browser-facing tools, including the eight former extension-only tools and `@c` references. |
| Tool surface | 43 tools in 9 categories; the original 32 result shapes remain; brief tools/list is 20,452 bytes and full detail is 37,067 bytes. |
| Release | 1.3.0 binaries, changelog and bilingual release notes. QA live acceptance, CI runs and asset hashes are attached only when available; NFR-3 remains pending and Firefox remains unsupported. |

The compact-versus-JSON benchmark records the candidate mean at 364.83 bytes / 91.21 tokens versus 2,077.33 bytes / 519.33 tokens for JSON, 82.44% smaller. The measured asset rows stay marked pending until CI publishes them.

## v1.4.0 structural milestone

v1.4.0 splits the backend into ports-and-adapters crates, adds a REST surface beside MCP and rewrites the extension in TypeScript and Vue 3. It adds no MCP tools: the 43-tool surface and its compact envelope remain as released in 1.3.0.

| Item | Exit criterion |
|---|---|
| Ports and adapters | `corvus-domain` is pure; `corvus-application` carries use cases, the `BrowserRegistry`, `BrowserSession`, `StepRecorder` and `ToolCatalog` ports, and the 32 original router specifications; `corvus-cdp`, `corvus-transport`, `corvus-record`, `corvus-mcp` and `corvus-api` are adapters; `corvusd` is the composition root; `corvus-runtime` is removed. |
| REST and auth | `/api/v1` serves health, browsers, sessions, tools and tool calls, with OpenAPI at `/api-docs/openapi.json` and Swagger at `/docs`. Auth is `off` by default and `local` enables Argon2id and HS256 JWT; a non-loopback bind requires auth `local`. Accounts and refresh tokens are in memory. |
| Persistence | SeaORM and `sea-orm-migration` manage schema version 1; an existing 1.3.0 database opens without a data migration, a pre-migration `VACUUM INTO` backup protects it, and the adapter keeps WAL, one writer, a 5-second busy timeout, 30-day retention and 262144-byte truncation. |
| Extension rewrite | TypeScript and Vue 3 sources under `extension/src` with Pinia and Naive UI; three Vite passes produce `extension/build/extension`; 21 unit, contract and component tests plus an isolated Playwright E2E suite cover the build. |
| Version alignment | One 1.4.0 version across the cargo workspace, the extension package and the manifest before the release tag. |
| Release | 1.4.0 binaries, changelog and bilingual release notes. QA acceptance is recorded in `tests/acceptance/v1.4.0.md`; CI run identifiers and published asset hashes remain pending; NFR-3 and the Firefox deviation remain open. |

State: 1.4.0 is delivered. The workspace and extension are versioned 1.4.0, and [v1.4.0.md](../tests/acceptance/v1.4.0.md) records PASS for 199 workspace tests in 25 suites, 135 conformance checks and the extension gate. NFR-3 remains unpassed, no live 1.4.0 deployment is claimed, and CI identifiers and published-asset hashes remain pending. 1.5.0 is the next milestone.

## v1.5.0 next

1.5.0 is the next milestone and is not yet scoped. Its scope, exit criteria and dates will be recorded before implementation; no deliverable or live-deployment claim is made for it here.

## Suggested schedule (one main developer + AI pairing)

| Week | Content |
|---|---|
| W1 | workspace skeleton, MCP dual stack, minimal extension link, navigate/snapshot |
| W2 | click/type/press/scroll, error codes and timeouts, M1 acceptance |
| W3-W4 | perception suite, action suite, tab concurrency |
| W5-W6 | request interception, cookies and storage, upload/download, multi-browser |
| W7-W8 | recording, replay, site memory, metrics, M2 acceptance |
| W9-W10 | installers, client configuration, docs site, M3/M4 |

## Quality gates

- Every PR: cargo fmt, clippy -D warnings, tests green, three-platform CI.
- Every tool: at least one integration test plus one real-page smoke test.
- Every week: one stability regression (72h soak can run in the background in parallel).
- Every release: version bump, changelog, signed binaries, checksums.

## Risk buffer

- Solve MV3 lifetime problems in M1; do not carry them into M2.
- The debugger channel and the DOM channel must stay two switchable paths, never a single point of failure.
- Firefox compatibility gets its own slot and is not mixed into the Chromium mainline.
