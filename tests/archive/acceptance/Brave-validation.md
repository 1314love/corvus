[English](Brave-validation.md) | [中文](Brave-validation.zh.md)

# Brave validation matrix

## 1. Purpose

This note records the Brave Browser validation of the frozen Corvus surface and complements the Edge and Chrome evidence already archived in the repository. It is a compact index, not a replacement for the full suite reports.

## 2. Environment and evidence roots

| Item | Value |
| --- | --- |
| Browser | Brave Browser 157.1.99.57, official portable win32-x64 build, headless endpoint `127.0.0.1:9335` |
| Browser id | `brave:832eff2fef81596d` |
| Corvus service | `corvusd` 0.1.0, MCP 2025-06-18, `http://127.0.0.1:7331/mcp` |
| Extension | unpacked Corvus extension 0.1.0; capabilities `dom`, `scripting`, `debugger` |
| Fixture | `http://127.0.0.1:8811/`, `/m2b`, `/blank`, `/api/json`, `/api/slow`, `/dl.txt` |
| Brave evidence root | `C:/Users/Administrator/AppData/Local/Temp/corvus-brave` |
| M2b evidence log | `C:/Users/Administrator/AppData/Local/Temp/corvus-brave/m2b_run.txt` |
| M4 evidence | `C:/Users/Administrator/AppData/Local/Temp/corvus-m4/2026-11-14T14-25-24-147Z` |

The live Corvus Windows service was used as-is. It was not restarted or stopped. The portable Brave instance was used only through its own tabs.

## 3. Check totals

| Suite | Checks | Result |
| --- | ---: | --- |
| Smoke | 7 | 7 / 7 PASS |
| M3 recording, replay and observability | 34 | 34 / 34 PASS |
| M4 cancellation and per-tab gates | 16 | 16 / 16 PASS |
| M2b network, downloads and uploads | 44 | 44 / 44 PASS |
| **Total** | **101** | **101 / 101 PASS** |

## 4. Suite matrix

| Suite | Brave checks | Brave evidence | Chrome counterpart |
| --- | ---: | --- | --- |
| Smoke: tab_new, page_text, type, click, eval, snapshot, tab_close | 7 | `C:/Users/Administrator/AppData/Local/Temp/corvus-brave/smoke.log` and `smoke.json` | `Chrome-validation.md` (7 / 7) |
| M3: sessions, steps, export, dry/live replay, metrics, logs, Prometheus | 34 | `C:/Users/Administrator/AppData/Local/Temp/corvus-brave/m3/checks.json` | `Chrome-validation.md` (34 / 34) |
| M4: -32800 cancellation, cancel no-op, different-tab overlap, same-tab serialization | 16 | `C:/Users/Administrator/AppData/Local/Temp/corvus-m4/2026-11-14T14-25-24-147Z/cancel.json` and `gates.json` | `Chrome-validation.md` (16 / 16) |
| M2b: capture, net_log, intercept, downloads, upload, routing | 44 | `C:/Users/Administrator/AppData/Local/Temp/corvus-brave/m2b_run.txt` | `Chrome-validation.md` (44 / 44) |

The 101 total is the Brave live matrix. The Edge, Chrome and Brave suites were sized for their own runs; the counts are not row-for-row interchangeable, and the counterpart links point to the archived evidence rather than claiming identical checkpoints.

## 5. Brave observations

- `browser_list` returned exactly one browser, `brave:832eff2fef81596d`, with `dom`, `scripting` and `debugger`.
- The M2b 44-check driver reports `tool_count: 32` because it accepts the current 32-tool daemon (`>= 26`); this is not an M2b regression.
- The hidden file input works: `upload_file` attached `up.txt` and `browser_eval` saw the attached file name.
- The first M2b run scored 42 / 44 because the new portable profile had no download directory and the download state was `interrupted`. After setting `download.default_directory` to `D:\\BraveDownloadsTest` in the disposable profile and relaunching Brave, the rerun scored 44 / 44 with id 2, state `complete`, 13 / 13 bytes and filename `D:\\BraveDownloadsTest\\dl.txt`. This was profile configuration, not a product defect.
- Cancellation evidence: the cancelled response was JSON-RPC `-32800`; different-tab overlap took 412 ms, same-tab serialization 821 ms, and the post-cancel gate was released in 415 ms.
- No Brave-only success shape, error code, or schema difference was observed.

## 6. Reproduce

Start the portable headless Brave and the live service first, then run the drivers from the evidence directories:

```bash
node /tmp/corvus-chrome/smoke.mjs
node /tmp/corvus-m3/drive.mjs
node /tmp/corvus-m4/cancel_drive.mjs
node /tmp/corvus-m2b/drive.mjs
```

Set the disposable profile's `download.default_directory` before the M2b run. Capture stdout to a log when a result file is not produced by the driver.

## 7. Pass criteria

- All four suites report zero failed checks; the total is 101 / 101.
- Every check uses the dedicated headless Brave browser id and leaves the client's own tabs untouched.
- No product defect is recorded; any profile or harness correction is called out in this note.
- The Edge and Chrome evidence remain the baseline for the shared surfaces.

## 8. Limitations and follow-up

- Brave is a single portable headless browser. Multi-browser ambiguity and two-mock routing remain covered by the conformance suites, not by this live matrix.
- The portable build is not the installer; the Windows service and extension installation path is exercised by the Edge evidence and the live installer smoke test.
- The Brave M2b run writes a real download into `D:\\BraveDownloadsTest`; clean it only if disk use matters.
- This file is evidence indexing only. It does not change the frozen M2/M3/M4 contracts.
