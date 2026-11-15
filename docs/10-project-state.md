# Corvus project state

[English](10-project-state.md) | [中文](10-project-state.zh.md)

Confidence: every fact below was read from the working tree at commit `6b7efee` plus its uncommitted changes. When this page disagrees with an older document, this page wins. Audience: maintainers and reviewers.

## 1. Released and in flight

Released 1.3.0 wires the daemon through `corvus-proto`, `corvus-mcp`, `corvus-transport`, `corvus-runtime`, `corvus-record` and `corvus-cdp`. `crates/corvus-mcp/src/tools.rs` declares `const SPECS: [Spec; 43]`; brief `tools/list` is 20,452 bytes and `detail: "full"` is 37,067 bytes across nine categories. Evidence lives in [v1.3.0-release-verification.md](../tests/report/v1.3.0-release-verification.md), [v1.3.0-packaging.md](../tests/report/v1.3.0-packaging.md) and [v1.3.0-snapshot-benchmark.md](../tests/report/v1.3.0-snapshot-benchmark.md).

The uncommitted working tree carries in-progress 1.4.0 work:

- `crates/corvus-domain`, `crates/corvus-application` (`pub const TOOLS: [ToolSpec; 32]` in `crates/corvus-application/src/spec.rs`) and `crates/corvus-api` (`/api/v1`, OpenAPI through utoipa, JWT and argon2 authentication) add the hexagonal split and a REST surface.
- `extension/` is rewritten in TypeScript and Vue 3 with Vite, pnpm, Vitest and Playwright; both `extension/package.json` and `extension/manifest.json` report 1.4.0, and the previous `background.js`, `content.js`, `offscreen.js`, `popup.js`, `options.js` and `daemon_url.js` are gone.

The refactor is wired in the working tree: `crates/corvusd/Cargo.toml` depends on domain, application and api, `corvusd/src/lib.rs` mounts `corvus_api::router`, and `crates/corvus-mcp` depends on application; the `corvus-runtime` crate is gone and its router now lives in `corvus-application`. What is missing is release: the Rust workspace is still 1.3.0, the extension reports 1.4.0, and none of it is committed.

## 2. Source of truth

| fact | read from |
|---|---|
| MCP surface is 43 tools | `crates/corvus-mcp/src/tools.rs` |
| browser-side application spec is 32 tools | `crates/corvus-application/src/spec.rs` |
| daemon wiring | `crates/corvusd/Cargo.toml` |
| Rust workspace version | `Cargo.toml` |
| extension version | `extension/package.json`, `extension/manifest.json` |
| released history and limits | `CHANGELOG.md`, `docs/release-notes/` |
| live 1.3.0 evidence | `tests/report/v1.3.0-*.md` |

## 3. Limits that still apply

- NFR-3 is not passed: the 72 hour soak stopped after about 8 hours at the customer's request. See [NFR-3-soak-audit.md](../tests/report/NFR-3-soak-audit.md).
- Firefox is not supported; see [06-firefox-assessment.md](06-firefox-assessment.md).
- The CDP client accepts loopback endpoints only and has no authentication.
- Docker and Kubernetes images ship no browser.
- A forced CDP channel has no fallback to the extension.
- macOS has no packaged installer.
- Extension tab ids and CDP tab ids remain two namespaces.

## 4. Document confidence

| area | document | confidence |
|---|---|---|
| requirements | [01-requirements.md](01-requirements.md) | high for the 1.3.0 trace; the title still reads v0.1 because the baseline was never renamed |
| architecture | [02-architecture.md](02-architecture.md) | high for the released daemon; the crate table and repository layout now mark the 1.4.0 crates as in progress |
| tool surface | [09-tools.md](09-tools.md) | high; matches the 43-tool registry |
| roadmap | [03-roadmap.md](03-roadmap.md) | medium; states intent, and NFR-3 stays open |
| release evidence | `tests/report/v1.3.0-*.md` | high; produced by the 1.3.0 verification run |
| archive | `tests/archive/` | low; kept for traceability, never current |

## 5. Superseded material

The M1 to M4 acceptance playbooks, the report templates and the release reports for 0.1.0 through 1.2.0 describe the JavaScript extension and tool surfaces of 18 to 26 tools, so they are not current. They moved to `tests/archive/`. Current verification uses [v1.2.0.md](../tests/acceptance/v1.2.0.md), [v1.3.0.md](../tests/acceptance/v1.3.0.md), `tests/conformance/run.sh` and `tests/perf/`.

## 6. Re-check a claim

- `cargo tree -p corvusd --depth 1` shows the daemon's direct dependencies.
- `pnpm --dir extension build` rebuilds the loadable extension.
- `git log -1 --oneline` shows the commit this page was read from.
