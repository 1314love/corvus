# Corvus M2a acceptance report

Status: template. Copy this file to `tests/report/M2a-report-<yyyy-mm-dd>.md`,
fill every table, and keep the evidence directory next to the report. The M1
playbook and report template remain in `M1.md`, `M1.zh.md` and
`M1-report-template.md`.

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
| Browser id | `<id from /health browsers[]>` |
| Capabilities | `<from /health>` (reference: `["dom","scripting"]`) |
| MCP endpoint | `http://127.0.0.1:7331/mcp` |
| Test page | `http://127.0.0.1:8808/m2` (fixture `tests/acceptance/fixtures/m2-page.html`) |
| Frozen tool count | 18 |

## 2. Build and connection checks

| check | result (PASS/FAIL) | evidence |
|---|---|---|
| `cargo build --offline -p corvusd` succeeds |  | build log |
| `corvusd serve` binds 7330 and 7331 |  | daemon.log |
| Fixture server serves `/m2`, `/m2-b`, `/m2-c` |  | server log, screenshots |
| Extension loads unpacked and connects |  | screenshot-extensions.png |
| `/health` lists the browser with `dom` |  | health-after.json |
| `tools/list` returns exactly 18 frozen names |  | tools-list.json |
| `tests/conformance/run.sh` exits 0 |  | conformance.log |

## 3. Conformance evidence (2026-11-14)

Reference run of `tests/conformance/run.sh` against a real `corvusd` binary
built from this tree, on isolated ports 27330/27331 with the mock extension.
This evidence exercises the daemon and MCP surface, not a real browser.

| item | observed |
|---|---|
| Build | `CARGO_TARGET_DIR=target/qa-m2-gates cargo build --offline -p corvusd --bin corvusd` exit 0 |
| Result | `RESULT: 64 passed, 0 failed, 0 skipped`, exit 0 |
| Health | version `0.1.0`, protocol `2025-06-18`, ws `127.0.0.1:27330`, http `127.0.0.1:27331` |
| `tools/list` | exactly 18 frozen names, four boolean annotations, frozen schemas |
| Round trip | `page_snapshot` `isError=false`, 700-byte structured result |
| Navigate A/B | `loaded=true`, final URL matches the request |
| Immediate snapshot after B | snapshot URL is page B, two nodes |
| Timeout | `browser_wait` selector `#never` with `timeout_ms:100` -> structured `TIMEOUT` |
| Unknown tab | `tab_switch` `tab-999` -> structured `NOT_FOUND` |
| Defaults on the wire | `page_html.max_chars=200000`, `browser_eval.world="isolated"` |
| Navigate without load | `loaded=false` answer is not an error; `url` is the current document |
| Envelopes | 32 command envelopes recorded by the mock |

Fill the real-browser tables below from the current run; do not copy these
conformance values into the browser tables.

## 4. Real-browser happy path

Record each tool once. Result is the `structuredContent` or the parsed text
payload; latency is client-side `curl %{time_total}` in milliseconds.

| # | tool | request arguments | key result fields (expected) | observed result | latency (ms) | evidence | PASS/FAIL |
|---|---|---|---|---|---|---|---|
| 1 | `browser_navigate` | `{"url":"http://127.0.0.1:8808/m2"}` | `navigated=true`, `loaded=true`, final `url` |  |  | navigate.json, png |  |
| 2 | `page_snapshot` | `{"mode":"interactive"}` | `title="Corvus M2 Page"`, `nodes[]` with a ref on `#q` |  |  | snapshot.json |  |
| 3 | `browser_type` | `{"target":"<Q_REF>","text":"hello corvus"}` | `typed=true`, `value="hello corvus"`, `value_length=12` |  |  | type.json |  |
| 4 | `browser_click` | `{"target":"<GO_REF>"}` | `clicked=true`, pointer/mouse events, `#out` becomes `clicked:hello corvus` |  |  | click.json, png |  |
| 5 | `page_text` | `{}` | `text` contains `Corvus M2`, `truncated=false` |  |  | page-text.json |  |
| 6 | `page_html` | `{"max_chars":2000}` | `html` contains `<main id="main">` |  |  | page-html.json |  |
| 7 | `page_find` | `{"query":"Find target"}` | `count=1`, `matches[0].ref` present |  |  | page-find.json |  |
| 8 | `browser_click` | `{"target":"<FIND_REF>"}` | `clicked=true`; `#findout` becomes `find target clicked` |  |  | find-click.json |  |
| 9 | `browser_hover` | `{"target":"<HOVER_REF>"}` | `hovered=true`, `target.tag` present |  |  | hover.json |  |
| 10 | `page_text` | `{}` | text contains `hover event received` |  |  | hover-text.json |  |
| 11 | `browser_press` | `{"target":"<KEY_REF>","keys":["Enter","Tab"]}` | `pressed=true`, `keys=["Enter","Tab"]` |  |  | press.json |  |
| 12 | `browser_scroll` | `{"direction":"down","pages":1}` | `scrolled=true`, `y>0` |  |  | scroll.json |  |
| 13 | `browser_scroll` | `{"direction":"up","pages":1}` | `y` is smaller than the down-scroll value |  |  | scroll-up.json |  |
| 14 | `browser_select` | `{"target":"<MULTI_REF>","values":["alpha","beta"]}` | `selected=["alpha","beta"]`, `value="alpha"` |  |  | select.json |  |
| 15 | `browser_eval` | `{"expression":"1+1"}` | `ok=true`, `value=2` |  |  | eval.json |  |
| 16 | `browser_eval` | `{"expression":"throw new Error('boom')"}` | `ok=false`, `error.name` and `error.message`; `isError=false` |  |  | eval-throw.json |  |
| 17 | `browser_eval` | cyclic object expression | `ok=true`, `value=null`, `value_note="unserializable"` |  |  | eval-cyclic.json |  |
| 18 | `browser_wait` | `{"selector":"#late","timeout_ms":5000}` | `matched=true`, `waited_ms>0` after a reload |  |  | wait.json |  |
| 19 | `browser_history` | `{"action":"back"}` | `action="back"`, url is `/m2`, `loaded=true` |  |  | history-back.json |  |
| 20 | `browser_history` | `{"action":"forward"}` | `action="forward"`, url is `/m2-b` |  |  | history-forward.json |  |
| 21 | `browser_history` | `{"action":"reload"}` | `action="reload"`, `loaded=true` |  |  | history-reload.json |  |
| 22 | `tab_list` | `{}` | `tabs[]` with `tab_id,index,active,url,title,status` |  |  | tab-list.json |  |
| 23 | `tab_new` | `{"url":"http://127.0.0.1:8808/m2-c"}` | `tab_id`, `loaded=true`, `previous_tab_id` |  |  | tab-new.json |  |
| 24 | `tab_switch` | `{"tab_id":"<NEW_TAB>"}` | `active=true`, url `/m2-c` |  |  | tab-switch.json |  |
| 25 | `tab_close` | `{"tab_id":"<NEW_TAB>"}` | `closed=true`, `tab_id` echoes, optional `next_tab_id` |  |  | tab-close.json |  |
| 26 | `page_text` | `{"max_chars":20}` | `truncated=true`, text length <=20 |  |  | page-text-truncated.json |  |

## 5. Negative cases

Every row must return a structured error (or, for evaluate page throws, an
`ok=false` success). Record the HTTP status and the `error.code`.

| # | request | expected | observed | evidence | PASS/FAIL |
|---|---|---|---|---|---|
| 1 | unknown tool `corvus_missing_tool` | `BAD_ARGS`, `isError=true` |  | negative-unknown.json |  |
| 2 | `browser_navigate` `{}` | `BAD_ARGS` |  | negative-navigate-empty.json |  |
| 3 | `page_snapshot` `{"mode":"everything"}` | `BAD_ARGS` |  | negative-mode.json |  |
| 4 | `page_text` `{"max_chars":0}` | `BAD_ARGS` |  | negative-text-zero.json |  |
| 5 | `page_html` `{"max_chars":500001}` | `BAD_ARGS` |  | negative-html-max.json |  |
| 6 | `page_find` `{}` | `BAD_ARGS` |  | negative-find-empty.json |  |
| 7 | `browser_hover` `{}` | `BAD_ARGS` |  | negative-hover-empty.json |  |
| 8 | `browser_press` `{"keys":[]}` | `BAD_ARGS` |  | negative-press-empty.json |  |
| 9 | `browser_scroll` `{"direction":"diagonal"}` | `BAD_ARGS` |  | negative-scroll.json |  |
| 10 | `browser_select` `{"target":"<MULTI_REF>","values":[]}` | `BAD_ARGS` |  | negative-select-empty.json |  |
| 11 | `browser_eval` `{}` | `BAD_ARGS` |  | negative-eval-empty.json |  |
| 12 | `browser_eval` `{"expression":"1","world":"server"}` | `BAD_ARGS` |  | negative-eval-world.json |  |
| 13 | `browser_wait` `{}` | `BAD_ARGS` |  | negative-wait-empty.json |  |
| 14 | `browser_wait` `{"selector":"#never","timeout_ms":100}` | structured `TIMEOUT`; record elapsed |  | negative-wait-timeout.json |  |
| 15 | `browser_history` `{"action":"refresh"}` | `BAD_ARGS` |  | negative-history.json |  |
| 16 | `tab_close` `{}` | `BAD_ARGS` |  | negative-tab-close.json |  |
| 17 | `tab_switch` `{"tab_id":"tab-999"}` | `NOT_FOUND` |  | negative-tab-missing.json |  |
| 18 | stale ref `browser_click @e999999` | `NOT_FOUND` |  | negative-stale-ref.json |  |
| 19 | extension disabled, then `page_text` | `TRANSPORT_DOWN` |  | negative-no-extension.json |  |

## 6. Error codes observed

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

Any code outside this table is a defect. In MCP a structured tool error is
`isError:true` with `content[0].text` beginning `CODE: message`; a JSON-RPC
`error` for a tool-level failure is a defect.

MCP observability: current `tools/call` error results carry only the text
`CODE: message`; `ErrorBody.details` is not forwarded. Record details such as
`waited_ms` from daemon logs or envelopes when needed; absence from the MCP text
alone is not a failure.

## 7. Performance

| tool family | runs | min (ms) | median (ms) | p95 (ms) | max (ms) | budget (ms) | verdict |
|---|---|---|---|---|---|---|---|
| `browser_navigate` |  |  |  |  |  | 2000 |  |
| `page_snapshot` |  |  |  |  |  | 1000 |  |
| interactive tools (click/type/hover/press/scroll/select) |  |  |  |  |  | 1000 |  |
| page tools (text/html/find) |  |  |  |  |  | 1500 |  |
| `browser_eval` |  |  |  |  |  | 30000 |  |
| `browser_wait` |  |  |  |  |  | condition + 5000 |  |
| tab and history tools |  |  |  |  |  | 2000 |  |

Hard fail: any happy-path call above 3000 ms except `browser_eval` and
`browser_wait`. Record the default timeout in use (60000 ms, ceiling 125000 ms)
and any call that hit it.

## 8. Defects

| id | severity | tool / area | reproduction | expected | actual | evidence |
|---|---|---|---|---|---|---|
| D1 |  |  |  |  |  |  |
| D2 |  |  |  |  |  |  |

Severity guide: **S1** blocks the M2 exit criterion; **S2** wrong result or
unstructured error with a workaround; **S3** cosmetic, latency or documentation.
Report contract mismatches to the Team Lead instead of patching `crates/` or
`extension/`.

Known timing risk: a `browser_navigate` answer with `loaded=false` is a
success, not an error. If an immediate snapshot still shows the previous page,
record both JSON payloads and retry after 500 ms before classifying it.

## 9. Environment notes

- Browser channel and profile:
- Screen scale / window size:
- Extension reloads or daemon restarts:
- Port overrides used (the default 7330/7331 may be busy):
- Known environmental anomalies (for example branded Edge ignoring
  `--load-extension`):
- Free-form observations for the next run:

## 10. Sign-off

| role | name | date | verdict (PASS / FAIL / PASS WITH DEFECTS) |
|---|---|---|---|
| QA operator |  |  |  |
| Team Lead |  |  |  |

Attach or link the evidence directory. A report without evidence files is not
accepted.
