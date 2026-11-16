# Corvus project state

[English](10-project-state.md) | [中文](10-project-state.zh.md)

Confidence: every fact below was read from the repository, the 1.7.0, 1.6.0, 1.5.0 and 1.4.0 acceptance reports, and the published release pages. Check `git log -1` and the release page before quoting this page. 1.7.0 is the current published release, published on 2026-11-16; 1.6.0 is the previous release. The 1.7.0 release has 62 tools in ten categories; QA acceptance passed at HEAD `d804c33` before release commit `f12930c`, and local Windows packaging passed separately. The 62-entry schema snapshot and conformance measurements describe the frozen 62-tool tree; tag, CI and published assets are recorded below. An internal v2.0 P2 milestone on branch `v2.0` is unreleased and outside the published 1.7.0 evidence; its QA report is recorded in section 10. Audience: maintainers and reviewers.

## 1. Current release: 1.7.0

1.7.0 was published on 2026-11-16 from annotated tag `v1.7.0`; the tag object `84d7c086147e96ff58531198b6ead55c65ff116f` targets release commit `f12930c145c8cac05a50db9702ab3f1ad021c666`. GitHub Release `446568768` published the assets, main CI run `43795994581` passed eight jobs, release workflow `43796443828` succeeded, and [tests/acceptance/v1.7.0.md](../tests/acceptance/v1.7.0.md) records QA PASS at HEAD `d804c33` before the release commit. 1.6.0 is the previous release.

The 1.6.0 daemon keeps the 1.5.0 ports-and-adapters backend across ten crates: `corvus-domain`, `corvus-application`, `corvus-api`, `corvus-proto`, `corvus-mcp`, `corvus-transport`, `corvus-record`, `corvus-cdp`, `corvusd` and `corvus`; `corvus-runtime` remains removed. It adds opt-in guardrails in `crates/corvusd/src/guardrails.rs`. The 1.6.0 release declared `const SPECS: [Spec; 43]` in `crates/corvus-mcp/src/tools.rs`; the 1.7.0 release declares 62 specs in ten categories and adds 18 tools, with `browser_screenshot` as the 44th. The 1.6.0 QA run measured 43 tools in nine categories, a 20,493-byte brief `tools/list` body and 37,496 bytes for `detail: "full"`. The earlier 44-tool working-tree run measured 21,164 bytes brief/default and 38,964 bytes full, but those values predate the 62-tool rescope; the 62-tool catalogue measured 31,367 bytes raw brief/default (31,368 with the trailing newline) and 52,328 bytes raw full (52,329 with the newline). Release evidence lives in [release notes](release-notes/v1.6.0.md) and [tests/acceptance/v1.6.0.md](../tests/acceptance/v1.6.0.md); [v1.5.0.md](../tests/acceptance/v1.5.0.md) is the previous baseline.

The 1.6.0 release ships:

- `crates/corvus-domain`, `crates/corvus-application` and `crates/corvus-api` keep the ports-and-adapters structure. The application crate defines the `BrowserRegistry`, `BrowserSession`, `StepRecorder` and `ToolCatalog` ports and keeps `pub const TOOLS: [ToolSpec; 32]` in `crates/corvus-application/src/spec.rs`.
- The REST surface serves `/api/v1` for health, browsers, sessions and tools, with OpenAPI at `/api-docs/openapi.json` and Swagger at `/docs`. Auth is `off` by default and `local` enables Argon2id plus HS256 JWT; `corvusd` mounts `corvus_api::router`.
- `corvus-record` uses SeaORM and `sea-orm-migration` with schema version 1, a pre-migration `VACUUM INTO` backup for existing databases, WAL, one writer and a 5-second busy timeout.
- `extension/` is written in TypeScript and Vue 3 with Pinia, Naive UI, Vite, Vitest and Playwright; three Vite passes produce `extension/build/extension`.
- The published 1.6.0 artifacts report 1.6.0. The source metadata in `Cargo.toml`, `extension/package.json` and `extension/manifest.json` now reports 1.7.0 for the 1.7.0 release.

The MCP surface is unchanged: 43 tools, nine categories, the compact single-text envelope and `browser_batch`. [tests/acceptance/v1.6.0.md](../tests/acceptance/v1.6.0.md) records PASS for 237 workspace tests on 25 result lines, 137 conformance checks, 55 Vitest tests in ten files, a 14-file/490,393-byte extension build and Playwright 1/1. Main CI run 43716832915 and the published-asset hashes are recorded in the 1.6.0 release notes.

## 2. 1.6.0 release and evidence

The 1.6.0 release adds opt-in daemon guardrails; it adds no MCP tool.

- **Guardrails policy.** `crates/corvusd/src/guardrails.rs` accepts strict version 1 JSON with read-only mode, tool allow and deny lists, host and private-network navigation gates, cookie redaction and untrusted-content boundaries. An invalid policy aborts startup.
- **Enforcement.** Checks run before dispatch in `RouterBackend::call`, every `browser_batch` step and `session_replay`. Denials use `POLICY_DENIED` as the 11th error code and REST returns 403.
- **Redaction and boundaries.** `browser_cookies` redacts cookie values; `page_text`, `page_html` and compact `page_snapshot` text are wrapped in untrusted-content boundaries. Password inputs stay masked.
- **Surface unchanged.** 43 MCP tools in nine categories; 1.6.0 adds no tool.

Measured for the release: 14 extension build files at 490,393 B, 55 Vitest tests in ten files and isolated Playwright 1/1; QA records 237 workspace tests on 25 result lines, 137 conformance checks and one Low `browser_type` channel finding, with no High or Medium findings. Published assets are the CI rebuilds: Windows 18,822,522 B SHA-256 `bdec40f2f26012f2aeb5f977d6eef4fe80c18a662683c4fa9004ea99f59451db`, Linux 20,124,260 B SHA-256 `e7a21d3996e52f8a0c5725d959987aa8c96eacfb044a928341575af19d4ce738`, combined `SHA256SUMS` 194 B SHA-256 `4ee452ce947c4b47cbe9cca4eabc64d8eac1345177f27ea3e2b0acca2e49f5bf`. The distinct local installer ZIP is 18,801,258 B, SHA-256 `3d91001f980c17c8e8eb41fa426730776244dab592e977b73f33fa0a6bf7cdcd`, with 84 checks and observations and 0 failures. NFR-3 is still unpassed and no live 1.6.0 deployment is claimed.

## 3. v1.7.0 release (2026-11-16)

The 1.7.0 release rescopes the MCP surface: it adds 18 tools to reach 62 tools in ten categories, with `browser_screenshot` as the 44th and the page category at 15. It captures `viewport`, `full_page` or `element` as `webp`, `png` or `jpeg`; `quality` is 1..100 and ignored for png, `inline` defaults to false, and `inline: true` adds an MCP image content block. Files are stored under `<home>/screenshots/` by `crates/corvusd/src/screenshot.rs`, capped at 8 MiB of decoded image bytes, and finalized exactly once by `finalize_screenshot_result` in `crates/corvusd/src/lib.rs`. `page_pdf` writes a unique PDF under `<daemon home>/pdf` with a 32 MiB decoded cap and no inline PDF.

CDP serves 46 browser-facing tools, the extension serves 42 and 12 are daemon-local. The four tab and window tools (`tab_group`, `tab_duplicate`, `window_list`, `window_new`) are extension-only and forced CDP returns `CAPABILITY_UNAVAILABLE`; CDP accepts `@cN` references and the extension accepts `@eN` references, with a wrong-channel reference returning `BAD_ARGS`. The extension declares 11 permissions including `tabGroups`, 42 debugger-based handlers and `all_frames: false`. `browser_batch` has 42 batchable tools after ten named exclusions; `storage`, `sessions` and `help` are not batchable. Recording and guardrails redact `cookie_set` and storage values; screenshot and PDF pixels are not redacted or boundary-wrapped.

Tag `v1.7.0` object `84d7c086147e96ff58531198b6ead55c65ff116f` targets release commit `f12930c145c8cac05a50db9702ab3f1ad021c666`; main CI run 43795994581 passed eight jobs, release workflow 43796443828 succeeded, and GitHub Release 446568768 published Windows ZIP 19,159,984 B SHA-256 `3269ea25a916687c0d638b4798965e24d32dc4adbd5cd9490352afc21076e5e1`, Linux tarball 20,443,563 B SHA-256 `f0e90a7a8d338cdbe3bcee251739567c03186395071457d1a08b064c2733b4c1` and combined `SHA256SUMS` 194 B SHA-256 `7d7163ea9ceabbd9f27018e9510ba6c4cdc96e939bea7e664e5cc62e0bfd6fea`. The schema snapshot at [tests/conformance/schema-snapshot.v1.7.0.json](../tests/conformance/schema-snapshot.v1.7.0.json) is the 62-entry file (36,993 B, 1,853 lines, SHA-256 `0763eee9256bae1942c1480317fc6a80375257d44b3674261b689ae1f3844114`); the conformance runner is 52,902 B and 1,046 lines (SHA-256 `43df9ea1e2366eff9db6171b369b9b56af2a75fbc412c04b589b52bb7e3d691f`). Conformance passed 137 checks with 0 failed and 0 skipped on the frozen 62-tool binary. The earlier 44-tool v1.7.0 results are superseded. QA acceptance passed at HEAD `d804c33` before release commit `f12930c`, and local Windows packaging passed separately: 263 workspace tests on 25 result lines, 91 Vitest tests in 13 files, a 14-file/537,844-byte extension build, 3 Playwright E2E tests, and 160/160 local packaging checks on ZIP 19,129,611 B SHA-256 `6b216bc251bfbbcd444823eed9ccebac97711ca5f9c66984a5f986735d86833e`; the ZIP is local only. 1.6.0 is the previous release.

## 4. Verification and packaging evidence

The formal QA acceptance report [tests/acceptance/v1.6.0.md](../tests/acceptance/v1.6.0.md) records PASS on the working tree at HEAD `61ca85f`: 237 workspace tests with 0 failed on 25 result lines, 137 conformance checks with 0 failed and 0 skipped, 55 Vitest tests in ten files, 14 build files at 490,393 B and Playwright 1/1. One Low `browser_type` channel finding was recorded; there were no High or Medium findings. The published Windows, Linux and combined checksum assets are CI rebuilds from tag `v1.6.0`. The distinct local installer ZIP passed local acceptance with 84 checks and observations, 0 failures, and is not a published release artifact.

The 62-tool v1.7.0 QA acceptance and local Windows packaging reports record PASS: 263 workspace tests on 25 result lines, 137 conformance checks, 91 Vitest tests in 13 files, a 14-file/537,844-byte build, 3 Playwright E2E tests, and 160/160 packaging checks. Earlier 44-tool reports are superseded and are not evidence for this release. The reports are included in the 1.7.0 release; the external evidence directory is not committed. The published Windows, Linux and combined checksum assets are CI rebuilds from tag `v1.7.0`.

## 5. Source of truth

| fact | read from |
|---|---|
| MCP surface is 62 tools in ten categories in the 1.7.0 release; 43 in the previous 1.6.0 release | `crates/corvus-mcp/src/tools.rs` |
| browser-side application spec is 49 tools | `crates/corvus-application/src/spec.rs` |
| application ports and use cases | `crates/corvus-application/src/` |
| snapshot baseline and diff semantics | `crates/corvus-application/src/snapshot_diff.rs`, `crates/corvus-application/src/router.rs` |
| daemon wiring | `crates/corvusd/Cargo.toml`, `crates/corvusd/src/lib.rs` |
| guardrails policy and enforcement | `crates/corvusd/src/guardrails.rs`, `crates/corvusd/src/lib.rs` |
| REST routes and auth | `crates/corvus-api/src/` |
| recording schema and migration | `crates/corvus-record/src/` |
| Rust workspace version | `Cargo.toml` |
| extension version and build | `extension/package.json`, `extension/manifest.json`, `extension/vite.*.config.ts` |
| extension i18n catalogs | `extension/src/ui/shared/i18n/`, `extension/_locales/` |
| released history and limits | `CHANGELOG.md`, `docs/release-notes/` |
| 1.4.0 acceptance evidence | `tests/acceptance/v1.4.0.md` |
| 1.5.0 release and QA acceptance evidence | `tests/acceptance/v1.5.0.md`, `docs/release-notes/v1.5.0.md` |
| 1.6.0 release, QA acceptance and packaging evidence | `tests/acceptance/v1.6.0.md`, `docs/release-notes/v1.6.0.md`, `tests/report/v1.6.0-packaging.md` |
| 1.7.0 release code, acceptance and local packaging | `tests/acceptance/v1.7.0.md`, `tests/report/v1.7.0-packaging.md`, `crates/corvus-mcp/src/tools.rs`, `crates/corvus-cdp/src/tools.rs`, `extension/src/background/screenshot.ts`, `crates/corvusd/src/screenshot.rs`, `crates/corvusd/src/lib.rs` |

## 6. Limits that still apply

- NFR-3 is not passed: the 72 hour soak stopped after about 8 hours at the customer's request. See [NFR-3-soak-audit.md](../tests/report/NFR-3-soak-audit.md).
- Firefox is not supported; see [06-firefox-assessment.md](06-firefox-assessment.md).
- The CDP client accepts loopback endpoints only and has no authentication.
- Docker and Kubernetes images ship no browser.
- A forced CDP channel has no fallback to the extension.
- macOS has no packaged installer.
- Extension tab ids and CDP tab ids remain two namespaces.
- Auth accounts and refresh tokens are in memory and do not survive a restart; there is no default account.
- REST authentication does not cover `/mcp`, the extension WebSocket or Swagger, so a non-loopback listener still needs a trusted authenticating proxy.
- The guardrails policy gates tools and navigation arguments, but it is not a network sandbox: DNS is not resolved and an allowed page can still fetch other origins.
- Content boundaries wrap page text, page HTML and compact snapshot text only; JSON snapshots are not wrapped, and `browser_eval`, `page_html` and `browser_history` can still expose page or secret data.
- A policy denial includes its policy file path in error details, so do not expose raw daemon errors to untrusted clients.
- The REST `session_id` field is accepted but not yet used to annotate the recording.
- 1.6.0 is published from tag `v1.6.0`; QA was measured on the working tree at HEAD `61ca85f` before the release commit, and the local installer ZIP is distinct from the published CI rebuilds. No live 1.6.0 deployment is claimed.
- 1.6.0 has a formal QA PASS on the working tree at HEAD `61ca85f` and was the current published release until 1.7.0. Its distinct local installer ZIP passed local acceptance with 84 checks and observations and 0 failures (18,801,258 B, SHA-256 `3d91001f980c17c8e8eb41fa426730776244dab592e977b73f33fa0a6bf7cdcd`), but it is not a published release artifact.
- Snapshot diffs apply to compact output only; `format: "json"` with `diff` is typed `BAD_ARGS`.
- The 1.7.0 release has 62 tools in ten categories; tag `v1.7.0` object `84d7c086147e96ff58531198b6ead55c65ff116f` targets release commit `f12930c145c8cac05a50db9702ab3f1ad021c666`. The 62-entry schema snapshot and the conformance and test measurements describe the frozen 62-tool tree; QA acceptance passed at HEAD `d804c33` before the release commit, and local Windows packaging passed separately. Screenshot and PDF pixels are neither redacted nor boundary-wrapped, and the four tab and window tools are extension-only. NFR-3 remains unpassed and no live 1.7.0 deployment is claimed.

## 7. Document confidence

Confidence grades: high means the claim was checked in this review against tagged code, the release tree or published artifacts; medium means the claim is checked but depends on external evidence; low means historical material that must not be used as current. Review date: 2026-11-16.

| area | document | confidence |
|---|---|---|
| requirements | [01-requirements.md](01-requirements.md) | high for the 1.4.0 to 1.7.0 trace and the 62-tool 1.7.0 release; the title still reads v0.1 because the baseline was never renamed, and the section 4 names are an original target matrix, not the shipped catalogue |
| architecture | [02-architecture.md](02-architecture.md) | high for 1.6.0 and the 1.7.0 release; the crate list, REST surface and extension build paths were checked in this review |
| roadmap | [03-roadmap.md](03-roadmap.md) | high; 1.6.0 is previous and 1.7.0 is the current 62-tool release with published assets, and NFR-3 stays open |
| install | [04-install.md](04-install.md) | medium; version stamps and Linux asset names are 1.6.0, macOS has no packaged installer, and the database backup behavior is documented since 1.4.0 |
| deployment | [05-deployment.md](05-deployment.md) | medium; migration and guardrail caveats were checked, and Docker and Kubernetes images ship no browser |
| Firefox assessment | [06-firefox-assessment.md](06-firefox-assessment.md) | medium; the NO-GO decision and platform gap are high, the 26/18 counts are the v0.1.0 baseline, the current tree has 46 CDP and 42 extension browser-facing tools, and the effort estimate is low |
| CDP guide | [07-cdp.md](07-cdp.md) | high for 1.6.0 and the released 1.7.0 46-tool surface |
| logging | [08-logging.md](08-logging.md) | high; logging behavior is unchanged since 1.4.0 |
| tool surface | [09-tools.md](09-tools.md) | high for the 1.7.0 62-tool catalogue; measured 31,367/52,328 B raw brief/full discovery bodies |
| guardrails | [11-guardrails.md](11-guardrails.md) | high for 1.6.0 and the 1.7.0 release; screenshot and PDF pixels are not redacted or boundary-wrapped |
| project state | this page | high; every row below has a source; 1.7.0 evidence is in the tagged release and the external evidence directory |
| release history | `docs/release-notes/` 0.1.0 to 1.4.0 | medium; historical records that are correct for their release, not current |
| tests and archive | `tests/acceptance/`, `tests/report/`, `tests/archive/` | medium for the 1.4.0 to 1.6.0 reports; low for pre-1.4 validation, the M1 to M4 archive and the English-only v1.1.0 record |
| entry points | `README.md`, `extension/README.md` | high; 1.7.0 is published and 1.6.0 is previous |

## 8. Superseded material

The M1 to M4 acceptance playbooks and the report templates describe the JavaScript extension and the 18-tool to 26-tool surfaces of 0.1.0, so they are not current. All 0.1.0 to 1.3.0 acceptance and release records, including the Brave and Chrome validation matrices, also moved to `tests/archive/`; they are historical evidence, not current verification. Current verification uses [v1.7.0.md](../tests/acceptance/v1.7.0.md), [v1.6.0.md](../tests/acceptance/v1.6.0.md), `tests/conformance/run.sh` and `tests/perf/`. 1.7.0 is the latest published release. Its QA PASS report at HEAD `d804c33` and local Windows packaging PASS report are included in the tree, with external evidence for the run; the published assets are CI rebuilds from tag `v1.7.0`.

## 9. Re-check a claim

- `cargo tree -p corvusd --depth 1` shows the daemon's direct dependencies.
- `pnpm --dir extension build` rebuilds the loadable extension.
- `git log -1 --oneline` shows the commit this page was read from.
- `cargo test -p corvusd guardrails` exercises the policy parser, precedence and gates.

## 10. v2.0 P2 internal milestone (unreleased)

The `v2.0` branch carries an internal P2 vertical slice that is not a tag, release or deployment; 1.7.0 remains the current published release. P2 exposes exactly ten tools: `browser_status`, `tab_list`, `tab_open`, `tab_close`, `page_navigate`, `page_read`, `page_snapshot`, `element_click`, `element_type` and `page_screenshot`. It uses dynamic `tools/list` with `notifications/tools/list_changed`, the 10/9/8/7/5/2/1 group matrix, opaque `r-<8 lowercase hex>` refs and a viewport-only `page_screenshot` with an 8 MiB decoded cap; `full_page` is P3.

The provider-only extension has exactly five permissions (`tabs`, `activeTab`, `scripting`, `storage`, `offscreen`), no debugger, content scripts keep `all_frames: false`, and the package version remains 1.7.0. It pairs over encrypted `/provider/v2` with P-256 ECDH, HKDF-SHA256 and AES-256-GCM; the offscreen document owns the socket and the service worker owns state. Rust owns the gateway, capability routing and semantics; a supervised `corvus-cdp-worker` child process uses stdio frames, and `CORVUS_V2_SURFACE=1` plus `CORVUS_V2_WORKER=1` activate the surface. The provider registry is in memory.

QA passed 303/303 functional checks across seven matrices, 27/27 recording assertions over 133 steps and three databases, Rust 321 passed/0 failed/1 ignored, and extension 89 tests in 13 files plus 1 E2E; no High or Medium defect is open. The report is [tests/report/v2.0.0-p2-capability-qa.md](../tests/report/v2.0.0-p2-capability-qa.md), 21,978 B / 253 lines / SHA-256 `59cc8603c6e57532ca1f4755d5ba571daa7d13b88efda44fa872ef199a4d6d23`. QA-frozen artifact SHA-256 values are `corvusd.exe` `7c64029846bb3c4fbf1d2a50f4a001a10d074f8e128dcdcd55926bf8a4437e92`, `corvus-cdp-worker.exe` `18807f4cf7f6b406da626a4597126c39086cece8a6f18d5197b698de3eb43542`, extension `manifest.json` `61f13fff4c4fac655334c8a74f36b4cac508c55de116c1639d54c9e3bf40d6c8`, `background.js` `b21b8ef525e4b49d536c3fca9758f0e8b47ea3b9bc6bdb10f1cef8feffcb7d7e` and `content.js` `46265ee19831fb4b253c15d0bcb9228f2d83d2d7673a84616f4c9a706abd81f0`. P2 is partial; `page_pdf` and the 62-tool surface are not in P2, legacy 1.x was not retested, Chrome 157 headless is the tested browser, cross-origin iframe and shadow DOM support is best effort, and no NFR-3 or live claim is made. P3 restores full coverage.