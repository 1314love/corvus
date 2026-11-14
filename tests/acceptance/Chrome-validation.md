[English](Chrome-validation.md) | [中文](Chrome-validation.zh.md)

# Chrome validation matrix

## 1. Purpose

This note records the Chrome for Testing validation of the frozen Corvus surface and compares it with the Edge evidence already archived in the repository. It is a compact index, not a replacement for the full suite reports.

## 2. Environment and evidence roots

| Item | Value |
| --- | --- |
| Browser | Chrome for Testing 157.0.8103.78, headless endpoint `127.0.0.1:9333` |
| Browser id | `chromium:7abc8aafb8098216` |
| Corvus service | `corvusd` 0.1.0, MCP 2025-06-18, `http://127.0.0.1:7331/mcp` |
| Extension | unpacked Corvus extension 0.1.0; capabilities `dom`, `scripting`, `debugger` |
| Fixture | `http://127.0.0.1:8811/`, `/m2b`, `/blank`, `/api/json`, `/api/slow`, `/dl.txt` |
| Chrome evidence root | `C:/Users/Administrator/AppData/Local/Temp/corvus-chrome` |
| M2b evidence log | `C:/Users/Administrator/AppData/Local/Temp/corvus-m2b/drive-chrome-20261114T2036.log` |

The live Corvus Windows service was used as-is. It was not restarted or stopped. The dedicated headless Chrome instance was used only through its own tab.

## 3. Check totals

| Suite | Checks | Result |
| --- | ---: | --- |
| Smoke | 7 | 7 / 7 PASS |
| M3 recording, replay and observability | 34 | 34 / 34 PASS |
| M4 cancellation and per-tab gates | 16 | 16 / 16 PASS |
| M2b network, downloads and uploads | 44 | 44 / 44 PASS |
| **Total** | **101** | **101 / 101 PASS** |

## 4. Suite matrix

| Suite | Chrome checks | Chrome evidence | Edge counterpart |
| --- | ---: | --- | --- |
| Smoke: tab_new, page_text, type, click, eval, snapshot, tab_close | 7 | `C:/Users/Administrator/AppData/Local/Temp/corvus-chrome/smoke.json` | `tests/report/M2b-live-report.md` (live Edge smoke and full 105-check run) |
| M3: sessions, steps, export, dry/live replay, metrics, logs, Prometheus | 34 | `C:/Users/Administrator/AppData/Local/Temp/corvus-chrome/m3d/checks.json` | `tests/report/M3-report-template.md` (M3 conformance RESULT 127/0/0) |
| M4: -32800 cancellation, cancel no-op, different-tab overlap, same-tab serialization | 16 | `C:/Users/Administrator/AppData/Local/Temp/corvus-m4/2026-11-14T12-44-30-592Z/cancel.json` and `gates.json`; summary log `C:/Users/Administrator/AppData/Local/Temp/corvus-m4/cancel-chrome-20261114T2044.log`; driver `C:/Users/Administrator/AppData/Local/Temp/corvus-m4/cancel_drive.mjs` | `tests/report/M4-report-template.md` (M4 conformance RESULT 135/0/0, cancellation reference 56 ms) |
| M2b: capture, net_log, intercept, downloads, upload, routing | 44 | `C:/Users/Administrator/AppData/Local/Temp/corvus-m2b/drive-chrome-20261114T2036.log`; driver `C:/Users/Administrator/AppData/Local/Temp/corvus-m2b/drive.mjs` | `tests/report/M2b-live-report.md` (live Edge RESULT passed=105 failed=0) |

The 101 total is the Chrome live matrix. The Edge suites were sized for their own runs (105 live checkpoints for M2b, 127 conformance checks for M3, 135 for M4); they are not the same row counts. The Edge column points to the archived evidence so the shared tool surfaces can be compared without pretending the counts are identical.

## 5. Chrome observations

- `browser_list` returned exactly one browser, `chromium:7abc8aafb8098216`, with `dom`, `scripting` and `debugger`.
- The M2b 44-check driver reports `tool_count: 32` because it accepts the current 32-tool daemon (`>= 26`); this is not an M2b regression.
- The hidden file input works: `upload_file` attached `up.txt` and `browser_eval` saw `input.files[0].name === "up.txt"`.
- Download evidence in the M2b run: id 5, state `complete`, 13 / 13 bytes, filename `D:\ChromeDownloadsTest\dl (3).txt`.
- Cancellation evidence: HTTP cancel returned 202, the cancelled response was JSON-RPC `-32800` in 716 ms, and the different-tab and same-tab gate timings were 418 ms and 843 ms.
- No Chrome-only success shape, error code, or schema difference was observed.

## 6. Reproduce

Start the dedicated headless Chrome and the live service first, then run the drivers from the evidence directories. The exact scripts are next to their JSON evidence:

```bash
node /tmp/corvus-chrome/smoke.mjs
node /tmp/corvus-m2b/drive.mjs
```

The M3 and M4 drivers are in `C:/Users/Administrator/AppData/Local/Temp/corvus-chrome/m3d` and `C:/Users/Administrator/AppData/Local/Temp/corvus-m4`. Capture stdout to a log when a result file is not produced by the driver.

## 7. Pass criteria

- All four suites report zero failed checks; the total is 101 / 101.
- Every check uses the dedicated headless Chrome browser id and leaves the client's own tabs untouched.
- No product defect is recorded; any harness correction is called out in the suite log.
- The Edge evidence links above remain the baseline for the shared surfaces.

## 8. Limitations and follow-up

- Chrome is a single headless browser. Multi-browser ambiguity and two-mock routing remain covered by the conformance suites, not by this live matrix.
- The Chrome M2b run writes a real download into the dedicated test download folder; clean it only if disk use matters.
- This file is evidence indexing only. It does not change the frozen M2/M3/M4 contracts.
