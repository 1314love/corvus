# Corvus Charter

[English](00-charter.md) | [中文](00-charter.zh.md)

| Item | Content |
|---|---|
| Codename | Corvus (渡鸦, raven) |
| One line | A browser hub built for AI agents: the browser as an organ of the agent, not a sandbox it visits. |
| Form | Rust local daemon + zero-UI browser extension + native MCP server |
| License | PolyForm Noncommercial 1.0.0 (source available, commercial use requires a separate license) |
| Started | 2026-11-14 |
| Author / maintainer | cosher |
| Status | Requirements and charter complete; entering M1 |

## 1. Why this exists

Browser automation has long been locked in one assumption: the thing driving the browser is a stranger, so
every click needs authorization, every download must be suspended, and every sensitive field must be released
by a human. That assumption is correct for unknown scripts. When the operator is an agent that already works
on your machine with your credentials, the same design leaves nothing but friction - humans get tired,
approvals get clicked through, and real risk signals drown in the noise.

Corvus does not tighten the old model; it replaces the premise: **the browser is an organ of the agent, not a
sandbox it visits.** Under that premise the first-class features are connection and capability - never drop,
zero approvals, complete tool surface, everything observable. When connection, capability and observability
are done properly, safety comes from the architecture itself (loopback only, per-user isolation, full audit and
replay) instead of from interrupting the user.

## 2. Product principles (final, 2026-11-14)

1. The browser is an organ of the agent; organs do not request permission on every use.
2. No pages. Beyond one tiny popup holding a single switch, the extension exposes no interface.
3. Never drop. Connection is the product's first feature, not an error-handling detail.
4. Complete tools. Not "enough", but everything an AI can legally do to a browser.
5. Everything observable. Every command, frame and request is recordable and replayable.
6. Engineering effort is not the constraint, money is not the constraint. Quality and ceiling are.

## 3. Goals

| ID | Goal | Metric |
|---|---|---|
| G1 | Never drop | 72-hour soak, command success >= 99.9%, median self-heal < 1s |
| G2 | Zero friction | install to first command < 60s, zero human approval prompts |
| G3 | Complete capability | >= 60 MCP tools across perception / action / network / storage / debugging |
| G4 | Fully observable | 100% of commands replayable, recording cost < 5% CPU |
| G5 | Multi-client | works out of the box in Claude Code, Cursor, DSH, Cline, Copilot, LangChain |

## 4. Non-goals (this release cycle)

- No cloud browser fleet.
- No visual workflow builder (recording exists for diagnosis and reproduction, not for orchestration).
- No mobile browser control.
- No enterprise multi-tenancy or compliance suites. Assumed: single machine, single user, user is root.

## 5. Scope and milestones

| Phase | Content | Exit criterion |
|---|---|---|
| M0 Charter | Requirements, architecture, repository and CI skeleton | docs 00/01/02/03 reviewed |
| M1 Skeleton | corvusd + extension + MCP (stdio/http) running navigate / snapshot / click / type | one real web task completed inside DSH |
| M2 Full surface | 60+ tools, multi-browser, tab concurrency, request interception, upload/download | 100% of the tool matrix against the acceptance table |
| M3 Observability | session recording, trace, replay, site memory, Prometheus | any historical session replays step by step |
| M4 Ecosystem | installers, client config generators, docs site, example library | a stranger runs the quick start in 5 minutes |

## 6. Decision log

| Date | Decision | Rationale |
|---|---|---|
| 2026-11-14 | Backend language: Rust | no GC pauses for a latency-critical hub, single-binary distribution, good fit with the CDP/WS ecosystem |
| 2026-11-14 | Shape: local daemon + extension dialing back | the product must control the browser the user is actually using; pure CDP requires a restart or a separate profile and is therefore excluded |
| 2026-11-14 | Remove all approvals and pairing | in the hosted-agent model the security boundary belongs to the OS user, not to modal dialogs |
| 2026-11-14 | Keep one minimal popup with a single debugger switch | the debugger permission shows a persistent browser banner and must be revocable at any moment |
| 2026-11-14 | Source available, commercial use forbidden (PolyForm Noncommercial 1.0.0) | share the code, keep commercial rights with the author |
| 2026-11-14 | Name: Corvus | corvids use tools, remember faces and plan ahead - the metaphor for an agent that scouts, remembers and acts |

## 7. Risks

| Risk | Impact | Mitigation |
|---|---|---|
| MV3 service worker sleeps after 30s and drops the link | High | offscreen keepalive + alarms watchdog + daemon-side reconnect |
| debugger banner is persistent and easy to close by accident | Medium | off by default, one-click toggle in the popup, automatic fallback to the DOM channel |
| Extension id differs across browsers | Medium | pin the id with a manifest key, distribute through the installer |
| Recordings grow without bound | Medium | chunking + compression + retention policy; structured events by default, not full frames |
| MCP client implementations differ | Medium | target the MCP 2025-06-18 spec, provide stdio and HTTP dual stack plus a doctor command |

## 8. Deliverables

1. corvusd (Rust daemon) + corvus (CLI)
2. Corvus extension (Chromium MV3 + Firefox WebExtension)
3. One-command installer (Windows / macOS / Linux)
4. Documentation: README, requirements, architecture, roadmap, client setup guides
5. Acceptance suite: reproducible scripts for 10 real tasks
