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

## v1.1.0 deployment milestone

v1.1.0 is a deployment slice of M4. The 32-tool surface is unchanged.

| Item | Exit criterion |
|---|---|
| Single root | `--home <dir>` or `CORVUS_HOME` holds binaries, the database, logs, config and the extension; `doctor` reports the active database path, and `--db` still overrides it. |
| Windows | The installer defaults to `%ProgramData%\Corvus`, migrates a 1.0.0 `%USERPROFILE%\.corvus` tree, and keeps the service on one root. |
| Linux | The x86_64 tarball carries `install-linux.sh`, a systemd unit, upgrade/uninstall scripts and `SHA256SUMS`. |
| Containers | The amd64 Docker image and Kubernetes manifests mount one `/data` root; CI builds and validates them. |
| Release | 1.1.0 binaries, changelog and bilingual release notes. NFR-3 stays pending until the 72h soak report is attached; Firefox remains unsupported. |

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
