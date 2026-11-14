# Corvus M2b acceptance report

Status: template. Copy this file to `tests/report/M2b-report-<yyyy-mm-dd>.md`,
fill every table, and keep the evidence directory next to the report. M2a
evidence keeps its own template in `M2a-report-template.md`; M1 evidence is in
`M1-report-template.md`.

## 1. Run metadata

| field | value |
|---|---|
| Run id | `<yyyy-mm-dd>-<operator>-<n>` |
| Date and time | `<yyyy-mm-dd hh:mm timezone>` |
| Operator | `<name>` |
| Commit | `<git rev-parse --short HEAD>` |
| Host OS | `<Windows 10 build>` |
| CPU / RAM | `<model / GB>` |
| Browser | `<Edge / Chrome / Chromium> <version>` (reference: Microsoft Edge 157) |
| Extension id | `<actual id from edge://extensions>` (reference: `pdgemoghkehbjpklbmkmchgfmpfghbl`) |
| Extension version | `<manifest version>` |
| Daemon version | `<from /health>` |
| Protocol version | `2025-06-18` |
| Browser id | `<id from /health browsers[]>` |
| Capabilities | `<from /health>` (reference: `["dom","scripting"]`) |
| MCP endpoint | `http://127.0.0.1:7331/mcp` |
| Test page | `http://127.0.0.1:8808/m2b` (fixture `tests/acceptance/fixtures/m2b-page.html`) |
| Frozen tool count | 26 |

## 2. Build and connection checks

| check | result (PASS/FAIL) | evidence |
|---|---|---|
| `cargo build --offline -p corvusd` succeeds |  | build.log |
| `corvusd serve` binds 7330 and 7331 |  | daemon.log |
| Fixture serves `/m2b`, `/api/json`, `/api/slow`, `/download/report.txt` |  | fixture.log, curl status lines |
| Extension loads unpacked and connects |  | screenshot-extensions.png |
| `/health` lists the browser with `dom` and `scripting` |  | health-after.json |
| `tools/list` returns exactly 26 names, each with optional `browser_id` |  | tools-list.json |
| `tests/conformance/run.sh` exits 0 |  | conformance.log |

## 3. Conformance evidence (2026-11-14)

Reference run of `tests/conformance/run.sh` against a real `corvusd` binary
built from this tree, on isolated ports 29330/29331 with two stateful mock
extensions. This exercises the daemon, MCP surface and routing, not a real
browser.

| item | observed |
|---|---|
| Build | `CARGO_TARGET_DIR=target/qa-m2b-review-build cargo build --offline -p corvusd --bin corvusd` exit 0, 47.27 s |
| Result | `RESULT: 108 passed, 0 failed, 0 skipped`, exit 0 (final rerun after the mock clippy fix) |
| Health | version `0.1.0`, protocol `2025-06-18`, ws `127.0.0.1:29330`, http `127.0.0.1:29331` |
| `tools/list` | exactly 26 frozen names, four boolean annotations, frozen schemas, optional `browser_id` on every tool |
| Browser registry | `browser_list` one browser, then two mocks `mock:conformance` and `mock:b` sorted by `connected_at_ms` |
| Routing | omitted `browser_id` with two browsers -> BAD_ARGS `multiple_browsers`; explicit `mock:b` -> routed `page_text`; unknown id -> TRANSPORT_DOWN |
| Network | `net_capture` start/status/stop, `net_log` filters and header gating, `net_clear`; seeded four entries |
| Interception | `net_intercept` string rule ids, block/mock/clear, invalid regex -> BAD_ARGS with `rule_id` |
| Downloads | `download_list` newest first with numeric ids and limit; `download_wait` accepts a decimal string, unknown decimal id -> TIMEOUT, non-decimal id -> BAD_ARGS |
| Uploads | `upload_file` happy path; non-file target and missing path -> BAD_ARGS |
| Navigate | A/B loaded and final URL; immediate snapshot sees page B; `loaded=false` is a success |
| Envelopes | 53 command envelopes recorded by the mocks |
| Independent rerun | the earlier 106-check revision was independently rerun by backend on 17440/17441 with 106/0/0; this revision adds two download-id checks |

Fill the real-browser tables below from the current run; do not copy these
conformance values into the browser tables.

## 4. Live evidence

Record the actual values from the current desktop session. Reference values in
section 3 came from mocks, not from this browser.

| checkpoint | expected | observed | evidence |
|---|---|---|---|
| Extension id | same id as the loaded unpacked directory |  | screenshot-extensions.png |
| Browser id and capabilities | one browser, `["dom","scripting"]` |  | health-after.json |
| `browser_list` | one entry with `browser_id`, `profile`, `version`, `capabilities`, `connected_at_ms` |  | browser-list-one.json |
| Capture start | `capturing:true`, `capacity:2000`, active `tab_id` |  | capture-start.json |
| Captured requests | `/api/json` entry with status 200, mime `application/json`, numeric `duration_ms` |  | net-log-api.json, net-log-headers.json |
| Block rule | request `status:null`, `error:"BlockedByClient"`, `intercepted.action:"block"`; page shows `network failed:` |  | intercept-block.json, intercept-block-text.json |
| Mock rule | request 200, `intercepted.action:"mock"`, page text `mock:99` |  | intercept-mock.json, intercept-mock-text.json |
| Cleared rules | real endpoint reached again; page text `json:7` |  | intercept-clear.json |
| Download | `state:"complete"`, filename `corvus-report.txt`, positive byte counts |  | download-wait.json, download-list-after.json |
| Upload | `uploaded:true`, `input.accept` contains `.txt`, `input.files` contains `upload.txt` |  | upload-file.json, upload-files.json |
| Routing | with two browsers, ambiguous BAD_ARGS, explicit route succeeds, unknown TRANSPORT_DOWN |  | route-ambiguous.json, route-unknown.json |

## 5. Real-browser happy path

Record each checkpoint once. Result is the parsed text payload or
`structuredContent`; latency is client-side `curl %{time_total}` in
milliseconds.

| # | tool | request arguments | key result fields (expected) | observed result | latency (ms) | evidence | PASS/FAIL |
|---|---|---|---|---|---|---|---|
| 1 | `browser_list` | `{}` | `browsers[]` with id/profile/version/capabilities/connected_at_ms |  |  | browser-list-one.json |  |
| 2 | `net_capture` | `{"action":"start"}` | `capturing=true`, `tab_id`, `started_at`, `capacity=2000` |  |  | capture-start.json |  |
| 3 | `net_capture` | `{"action":"status"}` | `capturing=true`, `entries>0` |  |  | capture-status.json |  |
| 4 | `net_log` | `{"url_contains":"/api/","limit":20}` | entries sorted by `seq`; `/api/json` status 200, mime, `duration_ms`; `next_seq` |  |  | net-log-api.json |  |
| 5 | `net_log` | `{"url_contains":"/api/json","include_headers":true}` | request and response header maps present |  |  | net-log-headers.json |  |
| 6 | `net_clear` | `{}` | `cleared` equals previous entry count |  |  | net-clear.json |  |
| 7 | `net_log` | `{}` | `entries=[]`, `next_seq=0` |  |  | net-log-empty.json |  |
| 8 | `net_capture` | `{"action":"stop"}` | `capturing=false`, tab id, entries count |  |  | capture-stop.json |  |
| 9 | `net_intercept` | `{"action":"set","rules":[{"id":"block-json","action":"block","url_regex":"/api/json"}]}` | `ok=true`, `scope="tab"`, echoed string rule id |  |  | intercept-block.json |  |
| 10 | `browser_click` | `{"target":"<FETCH_REF>"}` | `clicked=true`; page `#net-out` becomes `network failed: ...`, `data-net="failed"` |  |  | intercept-block-click.json, intercept-block-text.json, png |  |
| 11 | `net_log` | `{"url_contains":"/api/json"}` | blocked entry `status=null`, `error`, `intercepted.action="block"` |  |  | intercept-block-log.json |  |
| 12 | `net_intercept` | mock rule `mock-json`, body base64 `{"ok":true,"source":"mock","value":99}` | `ok=true`, echoed rule |  |  | intercept-mock.json |  |
| 13 | `browser_click` | `{"target":"<FETCH_REF>"}` | page `#net-out` becomes `mock:99`, `data-net="mock"` |  |  | intercept-mock-click.json, intercept-mock-text.json, png |  |
| 14 | `net_intercept` | `{"action":"clear"}` | `ok=true`, `rules=[]`, `applied=true` |  |  | intercept-clear.json |  |
| 15 | `download_list` | `{"limit":20}` | newest first; id/filename/url/state/bytes_received/total_bytes/danger/exists/started_at |  |  | download-list-before.json |  |
| 16 | `browser_click` | `{"target":"<DOWNLOAD_REF>"}` | `clicked=true`; a new download appears |  |  | download-click.json, downloads page png |  |
| 17 | `download_wait` | `{"timeout_ms":20000}` | `state="complete"`, filename `corvus-report.txt`, positive bytes |  |  | download-wait.json |  |
| 18 | `download_wait` | `{"id":"<DOWNLOAD_ID_AS_DECIMAL_STRING>"}` | resolves the same download immediately |  |  | download-wait-by-id.json |  |
| 19 | `upload_file` | `{"target":"#file","files":["<ABSOLUTE_WINDOWS_PATH>"]}` | `uploaded=true`, path echoed, `target.tag="input"`, accept/multiple |  |  | upload-file.json |  |
| 20 | `browser_eval` | `Array.from(document.querySelector("#file").files).map(f=>f.name)` | `ok=true`, `value=["upload.txt"]` |  |  | upload-files.json |  |
| 21 | `page_text` | `{}` | contains `files:upload.txt` |  |  | upload-text.json |  |
| 22 | `browser_list` | `{}` with two browsers loaded | two entries sorted by `connected_at_ms` ascending |  |  | browser-list-two.json |  |
| 23 | `page_text` | `{"browser_id":"<BROWSER_B_ID>"}` | succeeds against the second browser; routed envelope carries that browser |  |  | route-b.json |  |

## 6. Negative cases

Every row must return a structured tool error: HTTP 200,
`result.isError=true`, text `CODE: message` plus trailing details JSON. A
JSON-RPC `error` for a tool-level failure is a defect.

| # | request | expected | observed | evidence | PASS/FAIL |
|---|---|---|---|---|---|
| 1 | unknown tool `corvus_missing_tool` | BAD_ARGS |  | negative-unknown.json |  |
| 2 | `net_capture` `{"action":"pause"}` | BAD_ARGS |  | negative-capture-action.json |  |
| 3 | `net_log` `{"limit":0}` | BAD_ARGS |  | negative-log-zero.json |  |
| 4 | `net_log` `{"limit":1001}` | BAD_ARGS |  | negative-log-max.json |  |
| 5 | `net_intercept` `{"action":"set"}` | BAD_ARGS |  | negative-intercept-empty.json |  |
| 6 | rule id numeric `1` | BAD_ARGS |  | negative-intercept-id.json |  |
| 7 | duplicate rule ids | BAD_ARGS, `details.rule_id` |  | negative-intercept-duplicate.json |  |
| 8 | rule `url_regex:"["` | BAD_ARGS, `details.rule_id` |  | negative-intercept-regex.json |  |
| 9 | `download_list` `{"limit":201}` | BAD_ARGS |  | negative-download-limit.json |  |
| 10 | `download_wait` `{"id":"no-such-download"}` | BAD_ARGS (id must be a decimal string) |  | negative-download-id-type.json |  |
| 11 | `download_wait` `{"id":"999999999","timeout_ms":100}` | TIMEOUT, `details.waited_ms` and `details.id` |  | negative-download-id-unknown.json |  |
| 12 | `download_wait` `{"timeout_ms":100}` with no new download | TIMEOUT, `details.waited_ms` |  | negative-download-timeout.json |  |
| 13 | `upload_file` empty `files` | BAD_ARGS |  | negative-upload-empty.json |  |
| 14 | `upload_file` 11 paths | BAD_ARGS |  | negative-upload-count.json |  |
| 15 | `upload_file` target `#net-out` | BAD_ARGS, `not_a_file_input` |  | negative-upload-target.json |  |
| 16 | `upload_file` missing path | BAD_ARGS, `details.path` |  | negative-upload-path.json |  |
| 17 | `upload_file` relative path | BAD_ARGS |  | negative-upload-relative.json |  |
| 18 | `page_text` unknown `browser_id` | TRANSPORT_DOWN, `details.browser_id` |  | negative-browser-unknown.json |  |
| 19 | `page_text` omitted `browser_id` with two browsers | BAD_ARGS, `multiple_browsers` |  | negative-browser-ambiguous.json |  |
| 20 | `tab_switch` `{"tab_id":"tab-999"}` | NOT_FOUND, `details.tab_id` |  | negative-tab-string.json |  |
| 21 | `tab_switch` `{"tab_id":999}` | BAD_ARGS |  | negative-tab-numeric.json |  |
| 22 | extension disabled, then `page_text` | TRANSPORT_DOWN |  | negative-no-extension.json |  |

## 7. Error codes observed

| code | count | tools / scenarios | retryable (per taxonomy) |
|---|---|---|---|
| BAD_ARGS |  |  | no |
| NOT_FOUND |  |  | yes |
| NOT_INTERACTABLE |  |  | yes |
| TIMEOUT |  |  | yes |
| NAVIGATION_LOST |  |  | yes |
| CAPABILITY_UNAVAILABLE |  |  | no |
| BROWSER_GONE |  |  | no |
| TRANSPORT_DOWN |  |  | yes |
| INTERNAL |  |  | no |

Any code outside this table is a defect. MCP tool failures are
`isError:true` results; `ErrorBody.details` is appended to
`content[0].text` as a trailing JSON object after `CODE: message`. Record
`details.rule_id`, `details.browser_id`, `details.browser_ids`,
`details.reason`, `details.path`, `details.waited_ms` and `details.id` from
that text.

## 8. Performance

| tool family | runs | min (ms) | median (ms) | p95 (ms) | max (ms) | budget (ms) | verdict |
|---|---|---|---|---|---|---|---|
| `browser_list` |  |  |  |  |  | 1000 |  |
| `net_capture` start/status/stop |  |  |  |  |  | 1000 |  |
| `net_log` / `net_clear` |  |  |  |  |  | 1000 |  |
| `net_intercept` set/clear |  |  |  |  |  | 1000 |  |
| `download_list` |  |  |  |  |  | 2000 |  |
| `download_wait` |  |  |  |  |  | timeout + 5000 |  |
| `upload_file` |  |  |  |  |  | 2000 |  |
| browser routing |  |  |  |  |  | 1000 |  |

Hard fail: any happy-path call above 3000 ms except `download_wait` and
`net_capture`, which use their own budget. Record the default timeout
(60000 ms, ceiling 125000 ms) and any call that hit it.

## 9. Defects

| id | severity | tool / area | reproduction | expected | actual | evidence |
|---|---|---|---|---|---|---|
| D1 |  |  |  |  |  |  |
| D2 |  |  |  |  |  |  |

Severity guide: **S1** blocks the M2 exit criterion; **S2** wrong result or
unstructured error with a workaround; **S3** cosmetic, latency or documentation.
Report contract mismatches to the Team Lead instead of patching `crates/` or
`extension/`.

Known risks to classify carefully:

- A browser may uniquify the downloaded filename (for example
  `corvus-report (1).txt`). The `download` attribute requests
  `corvus-report.txt`; record the actual filename and treat only a missing or
  failed download as a defect.
- `net_log` is a ring of 2000 entries per captured tab. Entries can be evicted
  before `net_clear`; record `next_seq` and the observed count.
- Blocked requests legitimately have `status:null` and an `error` string.
  Do not treat a missing numeric status as a contract failure.
- `net_intercept` `set` acknowledges that rules were installed. If a Fetch
  action fails later while a request is in flight, the extension logs
  `intercept action failed` and falls back to continuing the request; the
  original `set` response cannot surface that error. Treat it as an S3
  observability limitation, check the logs, and note that `net_log` may show
  no `intercepted` marker.
- A Chrome download may already exist from a previous run. Always compare
  `download_list` before and after the click and wait for the new id, not a
  fixed count.
- Unknown string `browser_id` is TRANSPORT_DOWN, not NOT_FOUND; ambiguous
  omitted id is BAD_ARGS, not TRANSPORT_DOWN.

## 10. Environment notes

- Browser channel and profile:
- Screen scale / window size:
- Extension reloads or daemon restarts:
- Port overrides used (the default 7330/7331 may be busy):
- Download directory and filename uniquification observed:
- Temporary upload path and whether the browser could read it:
- Known environmental anomalies (for example branded Edge ignoring
  `--load-extension`):
- Free-form observations for the next run:

## 11. Sign-off

| role | name | date | verdict (PASS / FAIL / PASS WITH DEFECTS) |
|---|---|---|---|
| QA operator |  |  |  |
| Team Lead |  |  |  |

Attach or link the evidence directory. A report without evidence files is not
accepted.
