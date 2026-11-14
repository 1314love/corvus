# Corvus M1 acceptance report

Status: template. Copy this file to `tests/report/M1-report-<yyyy-mm-dd>.md`,
fill every table, and keep the evidence directory next to the report.

M2 wave 1 (18 tools) uses `tests/report/M2a-report-template.md` and the
bilingual M2a playbooks instead.

## 1. Run metadata

| field | value |
|---|---|
| Run id | `<yyyy-mm-dd>-<operator>-<n>` |
| Date and time | `<yyyy-mm-dd hh:mm timezone>` |
| Operator | `<name>` |
| Commit | `<git rev-parse --short HEAD>` |
| Host OS | `<Windows 10 build / macOS version / Linux distro>` |
| CPU / RAM | `<model / GB>` |
| Browser | `<Edge / Chrome / Chromium> <version>` (reference: Microsoft Edge 157) |
| Extension id | `<actual id from edge://extensions>` (reference: `pdgemoghkehbjpklbmkmchgfmpfghbl`) |
| Extension version | `<manifest version>` |
| Daemon version | `<from /health>` |
| Protocol version | `2025-06-18` |
| Browser id | `<id from /health browsers[]>` (reference: `edge:33628f01180d3d48`) |
| Capabilities | `<from /health>` (reference: `["dom","scripting"]`) |
| MCP endpoint | `http://127.0.0.1:7331/mcp` |
| Test page | `http://127.0.0.1:8808/` (fixture `tests/acceptance/fixtures/live-page.html`) |

## 2. Build and connection checks

| check | result (PASS/FAIL) | evidence |
|---|---|---|
| `cargo build --offline -p corvusd` succeeds |  | build log |
| `corvusd serve` binds 7330 and 7331 |  | daemon.log |
| `GET /health` returns version/protocol/addresses |  | health-before.json |
| Fixture server serves `http://127.0.0.1:8808/` |  | fixture server log, page screenshot |
| Extension loads unpacked in Edge without console errors |  | screenshot-extensions.png |
| `/health` lists the browser with `dom` capability |  | health-after.json |
| Popup shows connected |  | popup screenshot |
| Reconnect after disable/enable < 10 s |  | daemon.log excerpt |

## 3. Live evidence (2026-11-14)

Reference observation from the real-browser run on branded Microsoft Edge 157.
Replace the values with the current run and link the matching evidence files.

| item | observed |
|---|---|
| Extension id | `pdgemoghkehbjpklbmkmchgfmpfghbl` |
| Browser id | `edge:33628f01180d3d48` |
| Capabilities | `["dom","scripting"]` |
| MCP endpoint | `http://127.0.0.1:7331/mcp` |
| `tools/list` | exactly four frozen tools |
| Fixture page | `http://127.0.0.1:8808/`, title `Corvus Test Page` |
| `page_snapshot` (interactive) | 7 nodes; `@e1` on `input#q`, `@e2` on `button#go` |
| `browser_type` `{"target":"@e1","text":"hello corvus"}` | `typed=true`, `value="hello corvus"`, events `beforeinput,input,change` |
| `browser_click` `{"target":"@e2"}` | `clicked=true`, full pointer/mouse sequence |
| Snapshot after click | page text `clicked:hello corvus` |
| Navigate timing | `browser_navigate` returned `loaded=true`; an immediate snapshot can still race the new document on a cold start, so wait 500 ms and retry before recording a defect |

## 4. Tool results

Use one row per tool call. Add rows for repeats and negative cases.

| # | tool | request arguments | result (summary) | isError / code | latency (ms) | evidence file | PASS/FAIL |
|---|---|---|---|---|---|---|---|
| 1 | `browser_navigate` | `{"url":"http://127.0.0.1:8808/"}` | `{"navigated":true,"loaded":true,"url":"..."}` | false / - |  | navigate.json, screenshot-navigate.png |  |
| 2 | `page_snapshot` | `{"mode":"interactive"}` | `{"title":"Corvus Test Page","node_count":7,"nodes":[...]}` | false / - |  | snapshot.json |  |
| 3 | `browser_type` | `{"target":"@e1","text":"hello corvus"}` | `{"typed":true,"value":"hello corvus","value_length":12}` | false / - |  | type.json, screenshot-type.png |  |
| 4 | `page_snapshot` | `{"mode":"full"}` | input `attributes.value` is `hello corvus` | false / - |  | snapshot-after-type.json |  |
| 5 | `browser_click` | `{"target":"@e2"}` | `{"clicked":true,"events":[...]}` | false / - |  | click.json, screenshot-click.png |  |
| 6 | `page_snapshot` | `{"mode":"full"}` | text `clicked:hello corvus` present | false / - |  | snapshot-after-click.json |  |
| 7 | `tools/call` | unknown tool `corvus_missing_tool` | `BAD_ARGS: ...` | true / BAD_ARGS |  | negative-unknown.json |  |
| 8 | `browser_click` | `{"target":"@e999999"}` | `NOT_FOUND: ...` | true / NOT_FOUND |  | negative-stale-ref.json |  |
| 9 | `browser_click` | `{}` | `BAD_ARGS: ...` | true / BAD_ARGS |  | negative-missing-target.json |  |
| 10 | `browser_type` | `{"target":"@e1"}` | `BAD_ARGS: ...` | true / BAD_ARGS |  | negative-missing-text.json |  |
| 11 | `page_snapshot` | `{"mode":"everything"}` | `BAD_ARGS: ...` | true / BAD_ARGS |  | negative-invalid-mode.json |  |
| 12 | `page_snapshot` | `{"max_nodes":10}` | `BAD_ARGS: ...` | true / BAD_ARGS |  | negative-node-cap.json |  |
| 13 | `page_snapshot` | extension disabled | `TRANSPORT_DOWN: ...` | true / TRANSPORT_DOWN |  | negative-no-extension.json |  |

## 5. Error codes observed

| code | count | tools / scenarios | retryable (per taxonomy) | notes |
|---|---|---|---|---|
| BAD_ARGS |  |  | no |  |
| NOT_FOUND |  |  | yes |  |
| NOT_INTERACTABLE |  |  | yes |  |
| TIMEOUT |  |  | yes |  |
| NAVIGATION_LOST |  |  | yes |  |
| CAPABILITY_UNAVAILABLE |  |  | no |  |
| BROWSER_GONE |  |  | no |  |
| TRANSPORT_DOWN |  |  | yes |  |
| INTERNAL |  |  | no |  |

Rules: every error must be structured (`error.code` + `error.message`, never a bare
string); a code outside this table is a defect.

## 6. Performance

| tool | runs | min (ms) | median (ms) | p95 (ms) | max (ms) | budget (ms) | verdict |
|---|---|---|---|---|---|---|---|
| `browser_navigate` |  |  |  |  |  | 2000 |  |
| `page_snapshot` |  |  |  |  |  | 1000 (<=700 nodes) |  |
| `browser_click` |  |  |  |  |  | 1000 |  |
| `browser_type` |  |  |  |  |  | 1000 |  |

Measurement: client-side `curl %{time_total}` in milliseconds, including MCP HTTP and
daemon routing. Note whether the browser window was foreground, whether the page was
already loaded, and the `loaded` value returned by `browser_navigate`.

## 7. Defects

| id | severity | tool / area | reproduction | expected | actual | evidence |
|---|---|---|---|---|---|---|
| D1 |  |  |  |  |  |  |
| D2 |  |  |  |  |  |  |

Severity guide: **S1** blocks the M1 exit criterion; **S2** wrong result or
unstructured error with a workaround; **S3** cosmetic, latency or documentation.

Known timing risk: if `browser_navigate` resolves before the new document loads and an
immediate `page_snapshot` still shows the previous page, record the navigate JSON
(`loaded`), the snapshot JSON and both timestamps. If a 500 ms retry fixes it, classify
it as S2 with a workaround; if the new document never appears, classify it as S1.

## 8. Environment notes

- Browser channel and profile used:
- Screen scale / window size:
- Network conditions (VPN, proxy, offline):
- Extension reloads or daemon restarts during the run:
- Known environmental anomalies (for example branded Edge ignoring
  `--load-extension`):
- Free-form observations for the next run:

## 9. Sign-off

| role | name | date | verdict (PASS / FAIL / PASS WITH DEFECTS) |
|---|---|---|---|
| QA operator |  |  |  |
| Team Lead |  |  |  |

Attach or link the evidence directory. A report without evidence files is not
accepted.
