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
| Release | 1.4.0 binaries, changelog and bilingual release notes. QA acceptance is recorded in `tests/acceptance/v1.4.0.md`; final CI run 43690906863 and the published-asset hashes are recorded in the release notes; NFR-3 and the Firefox deviation remain open. |

State: 1.4.0 was delivered, 1.5.0 was delivered on 2026-11-15, and 1.6.0 was released on 2026-11-15. The 1.4.0 release shipped with the workspace and extension versioned 1.4.0, and [v1.4.0.md](../tests/acceptance/v1.4.0.md) records PASS for 199 workspace tests in 25 suites, 135 conformance checks and the extension gate. NFR-3 remains unpassed, no live 1.4.0 deployment is claimed, and final CI run 43690906863 and the published-asset hashes are recorded in the release notes. 1.7.0 is the current release; 1.6.0 is the previous release.

## v1.5.0 delivered (2026-11-15)

1.5.0 was released on 2026-11-15 from annotated tag `v1.5.0`; the tag object `a6ddc0f3e7b37f9cc29b0ff8c92eff839e05415f` targets `7d0e7c7126ea927a0d5f90df572c5aee31a9a4df` (product commit `d4d01eab7ef87cd9c73ec2f7aef8d633eda3a49a`, metadata fix `7d0e7c7`). It adds snapshot perception v2, compact snapshot diffs and extension i18n, with no new MCP tool; the 43-tool surface and nine categories are unchanged. [v1.5.0.md](../tests/acceptance/v1.5.0.md) records QA PASS on the frozen source tree at HEAD `73092ea`; main CI run `43702442120` passed eight jobs, and GitHub Release `445967405` published the assets.

| Item | Delivered | Evidence |
|---|---|---|
| Perception v2 | `page_snapshot` traverses open shadow roots and same-origin child frames, adds frame paths and `frames` metadata, skips cross-origin frames, budgets `max_nodes` globally and projects boxes to the top viewport. | Code in `corvus-cdp` and the extension runtime; QA PASS. |
| Compact diffs | `diff` is compact-only and computed once by the application layer from a 64-entry cache keyed by browser, session and tab; markers and baseline reset are documented in [09-tools.md](09-tools.md). | Code in `snapshot_diff.rs`; `browser_batch` shares the path. |
| Extension i18n | vue-i18n with English and zh-CN catalogs, detection and persistence, reactive switching, and manifest `_locales`. | 14 build files and 41 Vitest tests in eight files measured. |
| Surface | 43 MCP tools in nine categories; no new tool. | Catalogue unchanged. |
| Version alignment | Cargo workspace, extension package and manifest are 1.5.0. | Aligned through the release; metadata fix `7d0e7c7`. |
| QA acceptance | 220 workspace tests and 136 conformance checks passed; 43-tool snapshot 23,561 B. | PASS on frozen HEAD `73092ea`; installer PASS 98/0. |
| Release | Tag `v1.5.0`, CI `43702442120`, GitHub Release `445967405`; Windows ZIP 18,534,237 B and Linux tarball 19,849,047 B. | Published assets verified; combined `SHA256SUMS` 194 B. |

Measured for the release: 220 workspace tests passed on 25 result lines, 136 conformance checks passed, extension build 14 files, 41 Vitest tests in eight files and isolated Playwright 1/1; Windows installer acceptance passed 98 checks and observations with 0 failures on the distinct local ZIP (18,516,546 B, SHA-256 `2a36e28e42385d473edf058e8ed047a20ebf3cc4638f48fd3b469f80c29e5f5f`). The published assets are the CI rebuilds: Windows 18,534,237 B SHA-256 `e91e2ebce31d99d8753004849777e37277a93acd493cd320edfbda3987baabad`, Linux 19,849,047 B SHA-256 `c1a62be75c2895cd9eef27848b4a304551e2177bd66b9e1730d6150b9657c136`, combined `SHA256SUMS` 194 B SHA-256 `845aae4ee442020a34de4d90661c8cb2e6f83377f226d7a9e6263bdb354f9fbf`. QA and installer were measured on the frozen tree before the release commit. NFR-3 is still unpassed, no live 1.5.0 deployment is claimed, and no Docker registry publication or Kubernetes runtime test is claimed.

## v1.6.0 released (2026-11-15)

1.6.0 was released on 2026-11-15 from annotated tag `v1.6.0`; the tag object `c5daf98eb03b0fac310e53c3a59edfa77874f919` targets `b420c3787a8f240c60514ac8c745e67474f1c1fd`. It adds an opt-in daemon guardrails policy in `crates/corvusd/src/guardrails.rs`: read-only mode, tool allow and deny lists, host and private-network navigation gates, cookie redaction and untrusted-content boundaries. Enforcement runs before dispatch in `RouterBackend::call`, in every `browser_batch` step and in `session_replay`. Denials use `POLICY_DENIED` and REST returns 403. The 43-tool surface and nine categories are unchanged. A formal QA acceptance PASS was recorded on the working tree at HEAD `61ca85f`: 237 workspace tests on 25 result lines, 137 conformance checks, 55 Vitest tests in ten files and Playwright 1/1, with one Low `browser_type` channel finding and no High or Medium findings. Main CI run 43716832915 passed eight jobs, release workflow 43717236667 succeeded, and GitHub Release 446071476 published the assets. The distinct local installer ZIP passed local acceptance with 84 checks and observations and 0 failures (18,801,258 B, SHA-256 `3d91001f980c17c8e8eb41fa426730776244dab592e977b73f33fa0a6bf7cdcd`); it is not the published release artifact.

| Item | Delivered | Evidence |
|---|---|---|
| Guardrails policy | Strict version 1 JSON, 65,536-byte limit, CLI `--guardrails` and `CORVUS_GUARDRAILS`; an invalid policy aborts startup. | `crates/corvusd/src/guardrails.rs` and its unit tests; QA PASS. |
| Enforcement | Tool and URL checks before dispatch; guarded batch and replay; `POLICY_DENIED` as the 11th error code with REST 403. | `crates/corvusd/src/lib.rs`; 237 workspace tests, 0 failed. |
| Redaction and boundaries | Cookie values redacted; compact snapshot, page text and HTML wrapped in untrusted-content markers; password inputs masked. | Code in `corvusd` and `corvus-cdp`; QA PASS with one Low `browser_type` finding. |
| Extension UX | Localized `POLICY_DENIED` message with a sanitized reason. | Extension code and unit tests; 55 Vitest tests in ten files, 14 build files, Playwright 1/1. |
| Surface and version | 43 MCP tools in nine categories; source metadata and release metadata report 1.6.0. | Catalogue unchanged; released as tag `v1.6.0`. |
| Release | Annotated tag `v1.6.0` object `c5daf98eb03b0fac310e53c3a59edfa77874f919` targets `b420c3787a8f240c60514ac8c745e67474f1c1fd`; main CI `43716832915` passed eight jobs; release workflow `43717236667`; GitHub Release `446071476`. | Published assets verified: Windows ZIP 18,822,522 B SHA-256 `bdec40f2f26012f2aeb5f977d6eef4fe80c18a662683c4fa9004ea99f59451db`; Linux tarball 20,124,260 B SHA-256 `e7a21d3996e52f8a0c5725d959987aa8c96eacfb044a928341575af19d4ce738`; combined `SHA256SUMS` 194 B SHA-256 `4ee452ce947c4b47cbe9cca4eabc64d8eac1345177f27ea3e2b0acca2e49f5bf`. |

QA evidence: [v1.6.0.md](../tests/acceptance/v1.6.0.md) records PASS for 237 workspace tests on 25 result lines, 137 conformance checks, 55 Vitest tests in ten files, a 14-file/490,393-byte extension build and Playwright 1/1; the report records one Low `browser_type` channel finding and no High or Medium findings. Release evidence is published: main CI run 43716832915 passed eight jobs, release workflow 43717236667 succeeded, and GitHub Release 446071476 published Windows ZIP 18,822,522 B SHA-256 `bdec40f2f26012f2aeb5f977d6eef4fe80c18a662683c4fa9004ea99f59451db`, Linux tarball 20,124,260 B SHA-256 `e7a21d3996e52f8a0c5725d959987aa8c96eacfb044a928341575af19d4ce738`, and combined `SHA256SUMS` 194 B SHA-256 `4ee452ce947c4b47cbe9cca4eabc64d8eac1345177f27ea3e2b0acca2e49f5bf`. The CoT and language gate passed its 10-case self-test, scanned 313 tracked files with 0 findings, produced 0 findings in the last five commits, and the CI language gate passed. The distinct local installer ZIP passed local acceptance with 84 checks and observations and 0 failures (18,801,258 B, SHA-256 `3d91001f980c17c8e8eb41fa426730776244dab592e977b73f33fa0a6bf7cdcd`); it is not the published release artifact. NFR-3 remains unpassed and no live v1.6.0 deployment is claimed.

## v1.7.0 released (2026-11-16; 62-tool rescope)

The 1.7.0 release rescopes the MCP surface from 43 tools in nine categories to 62 tools in ten categories. It adds 18 tools: `page_pdf`, `page_links`, `page_forms`, `page_accessibility`, `browser_drag`, `browser_focus`, `browser_highlight`, `tab_group`, `tab_duplicate`, `window_list`, `window_new`, `cookie_set`, `cookie_clear`, `storage_get`, `storage_set`, `storage_clear`, `health` and `version`. `browser_screenshot` remains the 44th tool, and the page category now holds 15. The daemon file-output path covers screenshots and PDFs: `crates/corvusd/src/screenshot.rs` caps decoded images at 8 MiB and writes under `<home>/screenshots/`; `page_pdf` writes a unique `page-<unix_millis>-<uuid8>.pdf` under `<daemon home>/pdf` with a 32 MiB decoded cap, no inline PDF and the `page_pdf_not_replayable` replay reason.

CDP serves 46 browser-facing tools, the extension serves 42 and 12 are daemon-local (`health` and `version` are local only). The four tab and window tools (`tab_group`, `tab_duplicate`, `window_list`, `window_new`) are extension-only and forced CDP returns `CAPABILITY_UNAVAILABLE`. The extension declares 11 permissions including `tabGroups`, keeps 42 debugger-based handlers and `all_frames: false`. `browser_batch` keeps 42 batchable tools after ten named exclusions; `storage`, `sessions` and `help` are not batchable. Recording and guardrails redact `cookie_set` and storage values, while screenshot and PDF pixels are not redacted or boundary-wrapped.

QA acceptance passed at HEAD `d804c33` before release commit `f12930c`, and local Windows packaging passed separately: 263 workspace tests on 25 result lines, 137 conformance checks, 91 Vitest tests in 13 files, a 14-file/537,844-byte extension build, 3 Playwright E2E tests, and 160/160 local packaging checks on ZIP 19,129,611 B SHA-256 `6b216bc251bfbbcd444823eed9ccebac97711ca5f9c66984a5f986735d86833e`; the ZIP is local only and is not a CI rebuild or published asset. The schema snapshot at [tests/conformance/schema-snapshot.v1.7.0.json](../tests/conformance/schema-snapshot.v1.7.0.json) is the 62-entry file (36,993 B, 1,853 lines, SHA-256 `0763eee9256bae1942c1480317fc6a80375257d44b3674261b689ae1f3844114`), and `tests/conformance/run.sh` is 52,902 B, 1,046 lines, SHA-256 `43df9ea1e2366eff9db6171b369b9b56af2a75fbc412c04b589b52bb7e3d691f`; conformance on the frozen 62-tool binary passed 137 checks with 0 failed and 0 skipped. Tag `v1.7.0` object `84d7c086147e96ff58531198b6ead55c65ff116f` targets release commit `f12930c145c8cac05a50db9702ab3f1ad021c666`; main CI run 43795994581 passed eight jobs, release workflow 43796443828 succeeded, and GitHub Release 446568768 published Windows ZIP 19,159,984 B SHA-256 `3269ea25a916687c0d638b4798965e24d32dc4adbd5cd9490352afc21076e5e1`, Linux tarball 20,443,563 B SHA-256 `f0e90a7a8d338cdbe3bcee251739567c03186395071457d1a08b064c2733b4c1` and combined `SHA256SUMS` 194 B SHA-256 `7d7163ea9ceabbd9f27018e9510ba6c4cdc96e939bea7e664e5cc62e0bfd6fea`. The earlier 44-tool v1.7.0 QA PASS and local packaging PASS are superseded by the rescope. 1.6.0 is the previous release and NFR-3 remains unpassed.


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
