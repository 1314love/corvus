[English](06-firefox-assessment.md) | [中文](06-firefox-assessment.zh.md)

# Firefox feasibility assessment

Status: assessment complete against the v0.1.0 surface and re-checked against the 1.7.0 working tree on 2026-11-16; full parity is NO-GO; degraded DOM/tab mode is a conditional GO. The decision stands, and the tool counts below name their baseline.

Scope: Firefox 121+ desktop. The original assessment measured the browser-facing surface shipped in v0.1.0, which was 26 of the 32 MCP tools. The 1.7.0 working tree has 44 MCP tools, of which 34 are browser-facing and 10 are daemon-local, so the 26 and 18 counts in this document are historical. They are kept because the platform blockers and the shape of the degraded mode did not change, not because they describe the current catalogue. This document records a feasibility decision only; it does not change product code or the current support matrix.

## 1. Decision

| Option | Decision | Reason |
|---|---|---|
| Full browser-side parity (26 tools in v0.1.0; 42 extension tools in the 1.7.0 tree) | NO-GO | Firefox 121+ has no `chrome.debugger`/CDP attach and no `chrome.offscreen`; the blocking platform gap has no ETA. |
| Degraded 18-tool DOM/tab mode (v0.1.0 scope) | Conditional GO | Feasible with content scripts and the tabs API, but it drops eight tools and needs 15-20 engineer-days. A current port needs re-scoping against 42 extension browser-facing tools. |
| Current milestone | Keep Firefox out | The client has not committed to Firefox; record the NFR-6 deviation and revisit on a product decision. |

Recommendation: do not add Firefox to the current milestone. This document records the NFR-6 deviation; the client decision is pending. Revisit if Firefox becomes a committed target or if Bugzilla 1316741 lands.

## 2. Why full parity is NO-GO

The original parity target was 26 browser-facing tools: the 32-tool MCP surface minus the six daemon-level tools (`session_export`, `session_list`, `session_replay`, `session_steps`, `metrics`, `logs_tail`). The current tree has 50 browser-facing tools: the 62-tool surface minus twelve daemon-level tools, with CDP serving 46 and the extension 42. The absolute count changed; the platform blocker did not.

Firefox 121+ does not provide `chrome.debugger` or an equivalent CDP attach API. Bugzilla 1316741 is NEW with no ETA. The extension's CDP bridge is built on that API:

| Site | Use |
|---|---|
| `extension/src/background/runtime.ts:3277` | `chrome.debugger.getTargets` |
| `extension/src/background/runtime.ts:3294` | `chrome.debugger.attach` |
| `extension/src/background/runtime.ts:3343` | `chrome.debugger.detach` |
| `extension/src/background/runtime.ts:3388` | `chrome.debugger.sendCommand` |
| `extension/src/background/runtime.ts:3642` | `chrome.debugger.onDetach` |
| `extension/src/background/runtime.ts:3648` | `chrome.debugger.onEvent` |

Firefox also does not provide `chrome.offscreen`. The current extension is MV3 with a service worker; it creates an offscreen document to own the loopback WebSocket and heartbeat outside the service worker:

| Site | Use |
|---|---|
| `extension/manifest.json:8-10` | MV3 `background.service_worker` |
| `extension/manifest.json:17` | `debugger` permission |
| `extension/manifest.json:21` | `offscreen` permission |
| `extension/src/background/runtime.ts:467-503` | feature check and `chrome.offscreen.createDocument` |
| `extension/src/background/runtime.ts:495` | stated purpose: own the loopback WebSocket and heartbeat |
| `extension/src/offscreen/index.ts:160` | create the WebSocket |
| `extension/src/offscreen/index.ts:274` | keepalive timer |
| `extension/src/offscreen/index.ts:278` | health timer |

Without offscreen, full parity needs a different long-lived connection design. A Firefox event page is not a drop-in replacement, because it can be suspended when idle.

## 3. Degraded 18-tool mode (conditional GO)

The degraded mode keeps DOM and tab operations and drops the debugging, network, upload, and download tools. The kept set below is the v0.1.0 scope. A current port would need re-scoping against 42 extension browser-facing tools and would lose more than the eight tools listed here.

Kept tools (18):

| Group | Tools |
|---|---|
| DOM | `browser_click`, `browser_hover`, `browser_press`, `browser_scroll`, `browser_select`, `browser_type`, `browser_wait`, `page_find`, `page_html`, `page_snapshot`, `page_text` |
| Navigation | `browser_history`, `browser_navigate` |
| Browsers and tabs | `browser_list`, `tab_close`, `tab_list`, `tab_new`, `tab_switch` |

Dropped tools (8, v0.1.0 scope): `browser_eval`, `upload_file`, `net_capture`, `net_clear`, `net_intercept`, `net_log`, `download_list`, `download_wait`.

The kept set can run through content scripts, the tabs/scripting APIs, and the page DOM. It does not need `debugger`, `webRequest`, `downloads`, or `offscreen`.

Effort: 5-7 engineer-days for a prototype that proves the 18-tool path; 15-20 engineer-days for production quality, including the Firefox manifest and packaging, tests, documentation, and release hardening. These estimates belong to the v0.1.0 scope and need a current-tree re-estimate before any commitment. The 72-hour soak adds calendar time, not engineering days.

Risks:

- Event-page WebSocket suspension. A Firefox MV3 background event page can be suspended when idle. Without an offscreen document, the loopback socket may be disconnected; alarms and reconnect logic reduce the risk but do not remove it. The 72-hour reliability target is at risk. The current design uses the offscreen document for exactly this reason (`extension/src/background/runtime.ts:495`).
- Smaller tool surface. There is no network inspection, upload, download, or eval; clients that depend on those tools behave differently. The capability list and installation guide must say so.
- Maintenance cost. A second extension code path, a second manifest and packaging flow, and a larger CI matrix are ongoing costs.
- Platform differences. WebExtension behavior, content-script timing, and tab lifecycle differ from Chromium; the test surface is larger.
- Bug 1316741 has no ETA. Even if it lands later, full parity must be reassessed rather than assumed.

## 4. Conditions and next steps

Start work only if the client commits to Firefox as a supported target and accepts a re-scoped degraded mode.

Before production, the prototype must show:

- 72-hour WebSocket stability on Firefox 121+ (or the current ESR).
- Parity for every tool kept in the re-scoped degraded mode.
- A capability list that advertises the reduced surface.

If approved, keep the Firefox port in a separate extension directory and do not change the Chrome/Edge delivery path.

If not approved, keep Firefox unsupported, leave the NFR-6 deviation open, and revisit only when the product decision or Bugzilla 1316741 changes.

## 5. Evidence and confidence

- `extension/manifest.json:8-10` uses MV3 `background.service_worker`; `:17` requests `debugger`; `:21` requests `offscreen`.
- `extension/src/background/runtime.ts:3277,3294,3343,3388,3642,3648` is the CDP bridge.
- `extension/src/background/runtime.ts:467-503` creates and justifies the offscreen document.
- `extension/src/offscreen/index.ts:160,274,278` creates the WebSocket and runs its timers.
- [Bugzilla 1316741](https://bugzilla.mozilla.org/show_bug.cgi?id=1316741) is NEW with no ETA.
- The 26/18 tool counts are derived from the v0.1.0 MCP surface and the eight dropped tools listed above. The current 34/10 split is recorded in [10-project-state.md](10-project-state.md).
- The v0.1.0 file names `extension/background.js` and `extension/offscreen.js` were replaced by the TypeScript sources cited above in 1.4.0. The old line numbers are not valid for the current tree.
- Confidence: high for the platform gap and the NO-GO decision; medium for the degraded-mode design; low for the effort estimate against the current tree, which has not been re-measured on Firefox.

No product code, extension code, or tests were changed for this assessment.
