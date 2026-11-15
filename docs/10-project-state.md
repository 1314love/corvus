# Corvus project state

[English](10-project-state.md) | [中文](10-project-state.zh.md)

Confidence: every fact below was read from the repository and the 1.4.0 and 1.5.0 acceptance reports. Check `git log -1` and the release page before quoting this page. 1.4.0 is the current published release; the tree also contains unreleased v1.5.0 work with a QA acceptance PASS on dirty HEAD 73092ea. Audience: maintainers and reviewers.

## 1. Current release: 1.4.0

The 1.4.0 daemon is wired through ten crates: `corvus-domain`, `corvus-application`, `corvus-api`, `corvus-proto`, `corvus-mcp`, `corvus-transport`, `corvus-record`, `corvus-cdp`, `corvusd` and `corvus`; `corvus-runtime` was removed and its routing responsibilities live in `corvus-application` and `corvusd`. `crates/corvus-mcp/src/tools.rs` declares `const SPECS: [Spec; 43]`; the 1.4.0 acceptance run measured 43 tools in nine categories, a 20,451-byte brief `tools/list` body and 37,066 bytes for `detail: "full"`. Release evidence lives in [v1.4.0.md](../tests/acceptance/v1.4.0.md); the superseded 1.3.0 reports remain under `tests/report/` for traceability.

The 1.4.0 release ships:

- `crates/corvus-domain`, `crates/corvus-application` and `crates/corvus-api` provide the ports-and-adapters structure. The application crate defines the `BrowserRegistry`, `BrowserSession`, `StepRecorder` and `ToolCatalog` ports and keeps `pub const TOOLS: [ToolSpec; 32]` in `crates/corvus-application/src/spec.rs`.
- The REST surface serves `/api/v1` for health, browsers, sessions and tools, with OpenAPI at `/api-docs/openapi.json` and Swagger at `/docs`. Auth is `off` by default and `local` enables Argon2id plus HS256 JWT; `corvusd` mounts `corvus_api::router`.
- `corvus-record` uses SeaORM and `sea-orm-migration` with schema version 1, a pre-migration `VACUUM INTO` backup for existing databases, WAL, one writer and a 5-second busy timeout.
- `extension/` is written in TypeScript and Vue 3 with Pinia, Naive UI, Vite, Vitest and Playwright; three Vite passes produce `extension/build/extension`.
- The published 1.4.0 artifacts report 1.4.0; the current tree reports 1.5.0 in `Cargo.toml`, `extension/package.json` and `extension/manifest.json`.

The MCP surface is unchanged: 43 tools, nine categories, the compact single-text envelope and `browser_batch`. [tests/acceptance/v1.4.0.md](../tests/acceptance/v1.4.0.md) records PASS for the required matrix: 199 workspace tests in 25 suites, 135 conformance checks and the extension 21-test plus Playwright gates. Final CI run 43690906863 and the published-asset hashes are recorded in the 1.4.0 release notes.

## 2. Unreleased v1.5.0 work (QA and installer PASS on dirty tree)

The tree also contains work for 1.5.0, which is not released: there is no 1.5.0 tag or CI run and the local installer ZIP is not a published asset, but [v1.5.0.md](../tests/acceptance/v1.5.0.md) records QA PASS on dirty HEAD 73092ea, Windows installer acceptance passed 98 checks with 0 failures on the local ZIP, and the cargo workspace, extension package and manifest report 1.5.0.

- **Snapshot perception v2.** `page_snapshot` traverses open shadow roots and same-origin child frames, adds document-order frame paths, a `frames` summary with skip reasons, one global `max_nodes` budget and top-viewport boxes. Cross-origin frame DOM is never read.
- **Compact diffs.** `crates/corvus-application/src/snapshot_diff.rs` keeps a 64-entry LRU keyed by browser, session and tab. The application layer applies it once to compact output, so the extension, direct CDP and `browser_batch` paths share one grammar; JSON with `diff` is typed `BAD_ARGS`.
- **Extension internationalization.** vue-i18n catalogs for English and zh-CN, locale detection and persistence under `corvus.locale`, reactive switching, and manifest `_locales` for the MV3 name and description.
- **Surface unchanged.** 43 MCP tools in nine categories; 1.5.0 adds no tool.

Measured in the unreleased tree: 14 extension build files, 41 Vitest tests in eight files and isolated Playwright 1/1; Windows installer acceptance passed 98 checks and observations with 0 failures on the local ZIP. The 1.5.0 acceptance report records 220 workspace tests on 25 result lines, 136 conformance checks and QA PASS on dirty HEAD 73092ea; CI, published asset hashes and the GitHub release remain Pending. NFR-3 is still unpassed and no live 1.5.0 deployment is claimed.

## 3. Source of truth

| fact | read from |
|---|---|
| MCP surface is 43 tools | `crates/corvus-mcp/src/tools.rs` |
| browser-side application spec is 32 tools | `crates/corvus-application/src/spec.rs` |
| application ports and use cases | `crates/corvus-application/src/` |
| snapshot baseline and diff semantics | `crates/corvus-application/src/snapshot_diff.rs`, `crates/corvus-application/src/router.rs` |
| daemon wiring | `crates/corvusd/Cargo.toml`, `crates/corvusd/src/lib.rs` |
| REST routes and auth | `crates/corvus-api/src/` |
| recording schema and migration | `crates/corvus-record/src/` |
| Rust workspace version (1.5.0) | `Cargo.toml` |
| extension version and build (1.5.0) | `extension/package.json`, `extension/manifest.json`, `extension/vite.*.config.ts` |
| extension i18n catalogs | `extension/src/ui/shared/i18n/`, `extension/_locales/` |
| released history and limits | `CHANGELOG.md`, `docs/release-notes/` |
| 1.4.0 acceptance evidence | `tests/acceptance/v1.4.0.md` |
| 1.5.0 QA acceptance evidence | `tests/acceptance/v1.5.0.md` |

## 4. Limits that still apply

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
- 1.5.0 has QA and Windows installer acceptance PASS on a dirty tree; the local ZIP is not a published asset, and there is no CI or release evidence yet; its version metadata is 1.5.0.
- Snapshot diffs apply to compact output only; `format: "json"` with `diff` is typed `BAD_ARGS`.

## 5. Document confidence

| area | document | confidence |
|---|---|---|
| requirements | [01-requirements.md](01-requirements.md) | high for the 1.4.0 trace; the title still reads v0.1 because the baseline was never renamed |
| architecture | [02-architecture.md](02-architecture.md) | high for the 1.4.0 release and v1.5.0 QA PASS; section 5.9 covers unreleased v1.5.0 work |
| tool surface | [09-tools.md](09-tools.md) | high; 43 tools, with frame and diff additions marked as unreleased |
| roadmap | [03-roadmap.md](03-roadmap.md) | medium; 1.4.0 is delivered, 1.5.0 is unreleased with QA PASS, and NFR-3 stays open |
| release evidence | `tests/acceptance/v1.4.0.md`, `tests/acceptance/v1.5.0.md` | high; 1.4.0 PASS at HEAD 65fbf52; 1.5.0 PASS at dirty HEAD 73092ea |
| archive | `tests/archive/` | low; kept for traceability, never current |

## 6. Superseded material

The M1 to M4 acceptance playbooks and the report templates describe the JavaScript extension and tool surfaces of 18 to 26 tools, so they are not current. They moved to `tests/archive/`. The per-release verification reports for 0.1.0 through 1.4.0 remain in `tests/report/` and `tests/acceptance/`; the 1.4.0 ones are current evidence and earlier ones are history. Current verification uses [v1.4.0.md](../tests/acceptance/v1.4.0.md), [v1.5.0.md](../tests/acceptance/v1.5.0.md) for the 1.5.0 QA PASS, `tests/conformance/run.sh` and `tests/perf/`; 1.5.0 is not released.

## 7. Re-check a claim

- `cargo tree -p corvusd --depth 1` shows the daemon's direct dependencies.
- `pnpm --dir extension build` rebuilds the loadable extension.
- `git log -1 --oneline` shows the commit this page was read from.