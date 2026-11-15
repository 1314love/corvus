# Corvus project state

[English](10-project-state.md) | [中文](10-project-state.zh.md)

Confidence: every fact below was read from the repository and the 1.5.0 and 1.4.0 acceptance reports. Check `git log -1` and the release page before quoting this page. 1.5.0 is the current published release, published on 2026-11-15; 1.4.0 is the previous release. Audience: maintainers and reviewers.

## 1. Current release: 1.5.0

1.5.0 was published on 2026-11-15 from annotated tag `v1.5.0`; the tag object `a6ddc0f3e7b37f9cc29b0ff8c92eff839e05415f` targets `7d0e7c7126ea927a0d5f90df572c5aee31a9a4df`, the product commit is `d4d01eab7ef87cd9c73ec2f7aef8d633eda3a49a`, and metadata fix `7d0e7c7` aligned versions only. GitHub Release `445967405` published the assets, main CI run `43702442120` passed eight jobs, and [tests/acceptance/v1.5.0.md](../tests/acceptance/v1.5.0.md) records QA PASS on the frozen source tree at HEAD `73092ea`. 1.4.0 is the previous release.

The 1.5.0 daemon keeps the 1.4.0 ports-and-adapters backend across ten crates: `corvus-domain`, `corvus-application`, `corvus-api`, `corvus-proto`, `corvus-mcp`, `corvus-transport`, `corvus-record`, `corvus-cdp`, `corvusd` and `corvus`; `corvus-runtime` remains removed. `crates/corvus-mcp/src/tools.rs` declares `const SPECS: [Spec; 43]`; the 1.5.0 QA run measured 43 tools in nine categories, a 20,493-byte brief `tools/list` body and 37,372 bytes for `detail: "full"`. Release evidence lives in [release notes](release-notes/v1.5.0.md) and [tests/acceptance/v1.5.0.md](../tests/acceptance/v1.5.0.md); [v1.4.0.md](../tests/acceptance/v1.4.0.md) is the previous baseline.

The 1.5.0 release ships:

- `crates/corvus-domain`, `crates/corvus-application` and `crates/corvus-api` keep the ports-and-adapters structure. The application crate defines the `BrowserRegistry`, `BrowserSession`, `StepRecorder` and `ToolCatalog` ports and keeps `pub const TOOLS: [ToolSpec; 32]` in `crates/corvus-application/src/spec.rs`.
- The REST surface serves `/api/v1` for health, browsers, sessions and tools, with OpenAPI at `/api-docs/openapi.json` and Swagger at `/docs`. Auth is `off` by default and `local` enables Argon2id plus HS256 JWT; `corvusd` mounts `corvus_api::router`.
- `corvus-record` uses SeaORM and `sea-orm-migration` with schema version 1, a pre-migration `VACUUM INTO` backup for existing databases, WAL, one writer and a 5-second busy timeout.
- `extension/` is written in TypeScript and Vue 3 with Pinia, Naive UI, Vite, Vitest and Playwright; three Vite passes produce `extension/build/extension`.
- The published 1.5.0 artifacts report 1.5.0; `Cargo.toml`, `extension/package.json` and `extension/manifest.json` are aligned at 1.5.0.

The MCP surface is unchanged: 43 tools, nine categories, the compact single-text envelope and `browser_batch`. [tests/acceptance/v1.5.0.md](../tests/acceptance/v1.5.0.md) records PASS for 220 workspace tests on 25 result lines, 136 conformance checks, 41 Vitest tests in eight files and Playwright 1/1. Main CI run 43702442120 and the published-asset hashes are recorded in the 1.5.0 release notes.

## 2. 1.5.0 release and evidence

The 1.5.0 release adds snapshot perception v2, compact snapshot diffs and extension internationalization; it adds no MCP tool.

- **Snapshot perception v2.** `page_snapshot` traverses open shadow roots and same-origin child frames, adds document-order frame paths, a `frames` summary with skip reasons, one global `max_nodes` budget and top-viewport boxes. Cross-origin frame DOM is never read.
- **Compact diffs.** `crates/corvus-application/src/snapshot_diff.rs` keeps a 64-entry LRU keyed by browser, session and tab. The application layer applies it once to compact output, so the extension, direct CDP and `browser_batch` paths share one grammar; JSON with `diff` is typed `BAD_ARGS`.
- **Extension internationalization.** vue-i18n catalogs for English and zh-CN, locale detection and persistence under `corvus.locale`, reactive switching, and manifest `_locales` for the MV3 name and description.
- **Surface unchanged.** 43 MCP tools in nine categories; 1.5.0 adds no tool.

Measured for the release: 14 extension build files, 41 Vitest tests in eight files and isolated Playwright 1/1; Windows installer acceptance passed 98 checks and observations with 0 failures on the distinct local ZIP (18,516,546 B, SHA-256 `2a36e28e42385d473edf058e8ed047a20ebf3cc4638f48fd3b469f80c29e5f5f`). QA records 220 workspace tests on 25 result lines, 136 conformance checks and a 43-tool snapshot at 23,561 B, SHA-256 `d761561f54112bf06305bde716ebb072d88afa0b57ea4a61d72105436471a0c7`. Published assets are the CI rebuilds: Windows 18,534,237 B SHA-256 `e91e2ebce31d99d8753004849777e37277a93acd493cd320edfbda3987baabad`, Linux 19,849,047 B SHA-256 `c1a62be75c2895cd9eef27848b4a304551e2177bd66b9e1730d6150b9657c136`, combined `SHA256SUMS` 194 B SHA-256 `845aae4ee442020a34de4d90661c8cb2e6f83377f226d7a9e6263bdb354f9fbf`. NFR-3 is still unpassed and no live 1.5.0 deployment is claimed.

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
| 1.5.0 release and QA acceptance evidence | `tests/acceptance/v1.5.0.md`, `docs/release-notes/v1.5.0.md` |

## 4. Limits that still apply

- NFR-3 is not passed: the 72 hour soak stopped after about 8 hours at the customer's request. See [NFR-3-soak-audit.md](../tests/report/NFR-3-soak-audit.md).
- Firefox is not supported; see [06-firefox-assessment.md](06-firefox-assessment.md).
- The CDP client accepts loopback endpoints only and has no authentication.
- Docker and Kubernetes images ship no browser.
- A forced CDP channel has no fallback to the extension.
- macOS has no packaged installer.
- Extension tab ids and CDP tab ids remain two namespaces.
- Auth accounts and refresh tokens are in memory in 1.5.0 and do not survive a restart; there is no default account.
- REST authentication does not cover `/mcp`, the extension WebSocket or Swagger, so a non-loopback listener still needs a trusted authenticating proxy.
- The REST `session_id` field is accepted but not yet used to annotate the recording.
- 1.5.0 is published from tag `v1.5.0`; QA and Windows installer acceptance were measured on the frozen source tree before the release commit, and the local installer ZIP is distinct from the published CI rebuilds. No live 1.5.0 deployment is claimed.
- Snapshot diffs apply to compact output only; `format: "json"` with `diff` is typed `BAD_ARGS`.

## 5. Document confidence

| area | document | confidence |
|---|---|---|
| requirements | [01-requirements.md](01-requirements.md) | high for the 1.4.0 trace; the title still reads v0.1 because the baseline was never renamed |
| architecture | [02-architecture.md](02-architecture.md) | high for the 1.5.0 release; section 5.9 covers the released v1.5.0 work |
| tool surface | [09-tools.md](09-tools.md) | high; 43 tools, with frame and diff current in 1.5.0 |
| roadmap | [03-roadmap.md](03-roadmap.md) | medium; 1.5.0 is delivered, v1.6.0 is not scoped, and NFR-3 stays open |
| release evidence | `tests/acceptance/v1.4.0.md`, `tests/acceptance/v1.5.0.md` | high; 1.5.0 PASS at HEAD 73092ea and published release; 1.4.0 PASS at HEAD 65fbf52 |
| archive | `tests/archive/` | low; kept for traceability, never current |

## 6. Superseded material

The M1 to M4 acceptance playbooks and the report templates describe the JavaScript extension and tool surfaces of 18 to 26 tools, so they are not current. They moved to `tests/archive/`. The per-release verification reports for 0.1.0 through 1.5.0 remain in `tests/report/` and `tests/acceptance/`; the 1.5.0 ones are current evidence and [v1.4.0.md](../tests/acceptance/v1.4.0.md) is the previous baseline. Current verification uses [v1.5.0.md](../tests/acceptance/v1.5.0.md), `tests/conformance/run.sh` and `tests/perf/`; 1.5.0 is released.

## 7. Re-check a claim

- `cargo tree -p corvusd --depth 1` shows the daemon's direct dependencies.
- `pnpm --dir extension build` rebuilds the loadable extension.
- `git log -1 --oneline` shows the commit this page was read from.