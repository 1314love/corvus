# Corvus project state

[English](10-project-state.md) | [中文](10-project-state.zh.md)

Confidence: every fact below was read from the repository and the 1.4.0 acceptance report. Check `git log -1` and the release page before quoting this page. The workspace and extension are versioned 1.4.0, and the v1.4.0 acceptance matrix passed. Audience: maintainers and reviewers.

## 1. Current release: 1.4.0

The 1.4.0 daemon is wired through ten crates: `corvus-domain`, `corvus-application`, `corvus-api`, `corvus-proto`, `corvus-mcp`, `corvus-transport`, `corvus-record`, `corvus-cdp`, `corvusd` and `corvus`; `corvus-runtime` was removed and its routing responsibilities live in `corvus-application` and `corvusd`. `crates/corvus-mcp/src/tools.rs` declares `const SPECS: [Spec; 43]`; the 1.4.0 acceptance run measured 43 tools in nine categories, a 20,451-byte brief `tools/list` body and 37,066 bytes for `detail: "full"`. Release evidence lives in [v1.4.0.md](../tests/acceptance/v1.4.0.md); the superseded 1.3.0 reports remain under `tests/report/` for traceability.

The 1.4.0 release ships:

- `crates/corvus-domain`, `crates/corvus-application` and `crates/corvus-api` provide the ports-and-adapters structure. The application crate defines the `BrowserRegistry`, `BrowserSession`, `StepRecorder` and `ToolCatalog` ports and keeps `pub const TOOLS: [ToolSpec; 32]` in `crates/corvus-application/src/spec.rs`; `corvus-runtime` was removed.
- The REST surface serves `/api/v1` for health, browsers, sessions and tools, with OpenAPI at `/api-docs/openapi.json` and Swagger at `/docs`. Auth is `off` by default and `local` enables Argon2id plus HS256 JWT; `corvusd` mounts `corvus_api::router`.
- `corvus-record` uses SeaORM and `sea-orm-migration` with schema version 1, a pre-migration `VACUUM INTO` backup for existing databases, WAL, one writer and a 5-second busy timeout.
- `extension/` is rewritten in TypeScript and Vue 3 with Pinia, Naive UI, Vite, Vitest and Playwright; three Vite passes produce `extension/build/extension`.
- `Cargo.toml`, `extension/package.json` and `extension/manifest.json` all report 1.4.0.

The MCP surface is unchanged: 43 tools, nine categories, the compact single-text envelope and `browser_batch`. [tests/acceptance/v1.4.0.md](../tests/acceptance/v1.4.0.md) records PASS for the required matrix: 199 workspace tests in 25 suites, 135 conformance checks and the extension 21-test plus Playwright gates. Final CI run 43690906863 and the published-asset hashes are recorded in the 1.4.0 release notes.

## 2. Source of truth

| fact | read from |
|---|---|
| MCP surface is 43 tools | `crates/corvus-mcp/src/tools.rs` |
| browser-side application spec is 32 tools | `crates/corvus-application/src/spec.rs` |
| application ports and use cases | `crates/corvus-application/src/` |
| daemon wiring | `crates/corvusd/Cargo.toml`, `crates/corvusd/src/lib.rs` |
| REST routes and auth | `crates/corvus-api/src/` |
| recording schema and migration | `crates/corvus-record/src/` |
| Rust workspace version | `Cargo.toml` |
| extension version and build | `extension/package.json`, `extension/manifest.json`, `extension/vite.*.config.ts` |
| released history and limits | `CHANGELOG.md`, `docs/release-notes/` |
| 1.4.0 acceptance evidence | `tests/acceptance/v1.4.0.md` |

## 3. Limits that still apply

- NFR-3 is not passed: the 72 hour soak stopped after about 8 hours at the customer's request. See [NFR-3-soak-audit.md](../tests/report/NFR-3-soak-audit.md).
- Firefox is not supported; see [06-firefox-assessment.md](06-firefox-assessment.md).
- The CDP client accepts loopback endpoints only and has no authentication.
- Docker and Kubernetes images ship no browser.
- A forced CDP channel has no fallback to the extension.
- macOS has no packaged installer.
- Extension tab ids and CDP tab ids remain two namespaces.
- Auth accounts and refresh tokens are in memory in 1.4.0 and do not survive a restart; there is no default account.
- REST authentication does not cover `/mcp`, the extension WebSocket or Swagger, so a non-loopback listener still needs a trusted authenticating proxy.
- The REST `session_id` field is accepted but not yet used to annotate the recording.

## 4. Document confidence

| area | document | confidence |
|---|---|---|
| requirements | [01-requirements.md](01-requirements.md) | high for the 1.4.0 trace; the title still reads v0.1 because the baseline was never renamed |
| architecture | [02-architecture.md](02-architecture.md) | high for the 1.4.0 release; the ports, REST, auth and persistence sections match the code |
| tool surface | [09-tools.md](09-tools.md) | high; unchanged at 43 tools and matching the registry |
| roadmap | [03-roadmap.md](03-roadmap.md) | medium; 1.4.0 is delivered, 1.5.0 is next, and NFR-3 stays open |
| release evidence | `tests/acceptance/v1.4.0.md` | high; 1.4.0 acceptance run at HEAD 65fbf52 |
| archive | `tests/archive/` | low; kept for traceability, never current |

## 5. Superseded material

The M1 to M4 acceptance playbooks, the report templates and the release reports for 0.1.0 through 1.2.0 describe the JavaScript extension and tool surfaces of 18 to 26 tools, so they are not current. They moved to `tests/archive/`. Current verification uses [v1.2.0.md](../tests/acceptance/v1.2.0.md), [v1.3.0.md](../tests/acceptance/v1.3.0.md), [v1.4.0.md](../tests/acceptance/v1.4.0.md), `tests/conformance/run.sh` and `tests/perf/`; 1.4.0 acceptance is complete.

## 6. Re-check a claim

- `cargo tree -p corvusd --depth 1` shows the daemon's direct dependencies.
- `pnpm --dir extension build` rebuilds the loadable extension.
- `git log -1 --oneline` shows the commit this page was read from.
