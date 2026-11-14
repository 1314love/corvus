[English](06-firefox-assessment.md) | [中文](06-firefox-assessment.zh.md)

# Firefox feasibility assessment

Status: assessment complete; full parity is NO-GO, degraded DOM/tab mode is a conditional GO.

Scope: Firefox 121+ desktop, measured against the browser-facing tool surface shipped in v0.1.0. This document records a feasibility decision only; it does not change product code or the current support matrix.

## 1. Decision

| Option | Decision | Reason |
|---|---|---|
| Full 26-tool parity | NO-GO | Firefox 121+ has no `chrome.debugger`/CDP attach and no `chrome.offscreen`; the blocking platform gap has no ETA. |
| Degraded 18-tool DOM/tab mode | Conditional GO | Feasible with content scripts and the tabs API, but it drops eight tools and needs 15-20 engineer-days. |
| Current milestone | Keep Firefox out | The client has not committed to Firefox; record the NFR-6 deviation and revisit on a product decision. |

Recommendation: do not add Firefox to the current milestone. This document records the NFR-6 deviation; the client decision is pending. Revisit if Firefox becomes a committed target or if Bugzilla 1316741 lands.

## 2. Why full parity is NO-GO

The parity target is the 26 browser-facing tools: the 32-tool MCP surface minus the six daemon-level tools (`session_export`, `session_list`, `session_replay`, `session_steps`, `metrics`, `logs_tail`). Full parity means all 26 work on Firefox.

Firefox 121+ does not provide `chrome.debugger` or an equivalent CDP attach API. Bugzilla 1316741 is NEW with no ETA. The extension's CDP bridge is built on that API:

| Site | Use |
|---|---|
| `extension/background.js:2935` | `chrome.debugger.getTargets` |
| `extension/background.js:2950` | `chrome.debugger.attach` |
| `extension/background.js:2999` | `chrome.debugger.detach` |
| `extension/background.js:3044` | `chrome.debugger.sendCommand` |
| `extension/background.js:3198` | `chrome.debugger.onDetach` |
| `extension/background.js:3204` | `chrome.debugger.onEvent` |

Firefox also does not provide `chrome.offscreen`. The current extension is MV3 with a service worker; it creates an offscreen document to own the loopback WebSocket and heartbeat outside the service worker:

| Site | Use |
|---|---|
| `extension/manifest.json:8` | `"service_worker": "background.js"` |
| `extension/background.js:333,334` | feature checks for `chrome.offscreen` |
| `extension/background.js:356` | `chrome.offscreen.createDocument` |
| `extension/background.js:359` | stated purpose: own the WebSocket and heartbeat |
| `extension/offscreen.js:148` | create the WebSocket |
| `extension/offscreen.js:260` | keepalive timer |
| `extension/offscreen.js:264` | health timer |
| `extension/offscreen.js:296` | message bridge to the socket |

Without offscreen, full parity needs a different long-lived connection design. A Firefox event page is not a drop-in replacement, because it can be suspended when idle.

## 3. Degraded 18-tool mode (conditional GO)

The degraded mode keeps DOM and tab operations and drops the debugging, network, upload, and download tools.

Kept tools (18):

| Group | Tools |
|---|---|
| DOM | `browser_click`, `browser_hover`, `browser_press`, `browser_scroll`, `browser_select`, `browser_type`, `browser_wait`, `page_find`, `page_html`, `page_snapshot`, `page_text` |
| Navigation | `browser_history`, `browser_navigate` |
| Browsers and tabs | `browser_list`, `tab_close`, `tab_list`, `tab_new`, `tab_switch` |

Dropped tools (8): `browser_eval`, `upload_file`, `net_capture`, `net_clear`, `net_intercept`, `net_log`, `download_list`, `download_wait`.

The kept set can run through content scripts, the tabs/scripting APIs, and the page DOM. It does not need `debugger`, `webRequest`, `downloads`, or `offscreen`.

Effort: 5-7 engineer-days for a prototype that proves the 18-tool path; 15-20 engineer-days for production quality, including the Firefox manifest and packaging, tests, documentation, and release hardening. The 72-hour soak adds calendar time, not engineering days.

Risks:

- Event-page WebSocket suspension. A Firefox MV3 background event page can be suspended when idle. Without an offscreen document, the loopback socket may be disconnected; alarms and reconnect logic reduce the risk but do not remove it. The 72-hour reliability target is at risk. The current design uses the offscreen document for exactly this reason (`extension/background.js:359`).
- Smaller tool surface. There is no network inspection, upload, download, or eval; clients that depend on those tools behave differently. The capability list and installation guide must say so.
- Maintenance cost. A second extension code path, a second manifest and packaging flow, and a larger CI matrix are ongoing costs.
- Platform differences. WebExtension behavior, content-script timing, and tab lifecycle differ from Chromium; the test surface is larger.
- Bug 1316741 has no ETA. Even if it lands later, full parity must be reassessed rather than assumed.

## 4. Conditions and next steps

Start work only if the client commits to Firefox as a supported target and accepts the 18-tool mode.

Before production, the prototype must show:

- 72-hour WebSocket stability on Firefox 121+ (or the current ESR).
- Parity for all 18 kept tools on the acceptance pages.
- A capability list that advertises the reduced surface.

If approved, keep the Firefox port in a separate extension directory and do not change the Chrome/Edge delivery path.

If not approved, keep Firefox unsupported, leave the NFR-6 deviation open, and revisit only when the product decision or Bugzilla 1316741 changes.

## 5. Evidence

- `extension/manifest.json:7-8` uses MV3 `background.service_worker`; `:15` requests `debugger`; `:19` requests `offscreen`.
- `extension/background.js:2935,2950,2999,3044,3198,3204` is the CDP bridge.
- `extension/background.js:333,334,356,359` creates and justifies the offscreen document.
- `extension/offscreen.js:148,260,264,296` creates the WebSocket, runs its timers, and bridges messages.
- [Bugzilla 1316741](https://bugzilla.mozilla.org/show_bug.cgi?id=1316741) is NEW with no ETA.
- The 26/18 tool counts are derived from the v0.1.0 MCP surface and the eight dropped tools listed above.

No product code, extension code, or tests were changed for this assessment.
