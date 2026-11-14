# Corvus

> **The browser hub built for AI agents.** 让浏览器成为 AI 的器官，而不是它的沙箱。

Corvus is a Rust-powered local daemon plus a browser extension that gives any MCP-capable agent full, persistent, zero-friction control over the browser you are actually using. No approval dialogs. No pairing ritual. No separate browser. No disconnect.

---

## Why

Browser automation usually begins from the assumption that the agent is an intruder: every action needs a permission, every permission needs a human, and every human decision is one more chance to be interrupted. That trade-off makes sense when a stranger drives your machine. It is the wrong trade-off for an agent that already works for you.

Corvus starts from the opposite premise: **the browser is an organ of the agent, not a sandbox it visits.**

| Principle | What it means in the product |
|---|---|
| No friction | zero approval dialogs, zero pairing steps, zero setup wizards |
| No surface | no side panel, no settings page; one popup switch for the debugger channel |
| No dropout | connection is a first-class subsystem: multi-transport, watchdog, resume across restarts |
| No ceiling | every operation the browser can perform becomes a tool, not a curated subset |
| No blind spots | every command, snapshot and request is recorded and replayable |

## What it is

    +-----------------+        MCP (stdio | streamable-http)        +------------------+
    |  agent client   | <-----------------------------------------> |  corvusd (Rust)  |
    +-----------------+                                             +---------+--------+
                                                                             |
                                                     WS  /  Native Messaging / CDP
                                                                             |
                                               +-----------------------------+----+
                                               |  Corvus extension (MV3, no UI)   |
                                               +-----------------------------+----+
                                                                             |
                                                       Edge / Chrome / Firefox / ...

- **corvusd**: the daemon. MCP server, transport hub, command router, session recorder, site memory.
- **corvus**: the CLI. doctor, serve, attach, session replay, bench.
- **extension**: MV3, zero pages. Its popup holds exactly one switch: the debugger-grade control channel (a single on/off switch, nothing else).

## Feature surface (target)

- **Perception**: hybrid DOM + accessibility snapshot, screenshot (viewport / element / full page), text, HTML, console, network waterfall.
- **Action**: navigate, click, type, press, hover, drag, select, scroll, upload, download, multi-tab concurrency.
- **Power**: evaluate JS in page or isolated world, cookie and storage access, request interception, device and network emulation, PDF export.
- **Continuity**: idempotent commands, replayable sessions, resumable connection across daemon restarts, multi-browser fan-out.
- **Memory**: per-domain site cards learned from successful runs, stored in SQLite, exportable and shareable.

## Status

Design phase. v0.1 targets: single-browser full tool surface, MCP over stdio and HTTP, recording and replay. See [docs/03-roadmap.md](docs/03-roadmap.md).

## Docs

- [立项书](docs/00-立项书.md) - why this exists, scope, milestones, decisions
- [需求规格说明书](docs/01-需求规格说明书.md) - functional and non-functional requirements
- [Architecture](docs/02-architecture.md) - components, protocols, data model
- [Roadmap](docs/03-roadmap.md) - milestones and acceptance tasks

## License

**PolyForm Noncommercial License 1.0.0** — source available: read, run, modify and redistribute for any
noncommercial purpose. Commercial use is not allowed without a separate paid license; contact
`cosher@163.com` to arrange one.

This is deliberately not an OSI-approved open source license: the source is public, commercial use is not.
See [LICENSE](LICENSE) for the binding text.