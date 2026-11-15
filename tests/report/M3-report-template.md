# Corvus M3 acceptance report

Status: template. Copy this file to `tests/report/M3-report-<yyyy-mm-dd>.md`,
fill every table, and keep the evidence directory next to the report. M2b
evidence uses `M2b-report-template.md`; M2a uses `M2a-report-template.md`.

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
| Capabilities | `<from /health>` (reference: `["dom","scripting","debugger"]`) |
| MCP endpoint | `http://127.0.0.1:7331/mcp` |
| Database path | `<absolute --db path>` (reference: `/tmp/corvus-m3/corvus.db`) |
| MCP session id | `<Mcp-Session-Id returned by initialize>` |
| Test page | `http://127.0.0.1:8808/m2` (fixture `tests/acceptance/fixtures/m2-page.html`) |
| Frozen tool count | 32 |

## 2. Build and connection checks

| check | result (PASS/FAIL) | evidence |
|---|---|---|
| `cargo build --offline -p corvusd` succeeds |  | build.log |
| `corvusd serve --db <temp path>` binds 7330 and 7331 |  | daemon.log |
| The `--db` file and its `-wal`/`-shm` files are created |  | dir listing |
| `corvus doctor` prints db path, schema version and counts |  | doctor.log |
| Fixture serves `/m2` |  | curl status line |
| Extension loads unpacked and connects |  | screenshot-extensions.png |
| `/health` lists the browser with `dom`, `scripting` and `debugger` |  | health-after.json |
| `tools/list` returns exactly 43 names with the six M3 tools |  | tools-list.json |
| `tests/conformance/run.sh` exits 0 |  | conformance.log |

## 3. Conformance evidence

Reference run of `tests/conformance/run.sh` against a real `corvusd` binary
built from this tree, with a temporary SQLite database and a stateful mock
extension. This exercises the daemon, MCP surface, routing, recorder and replay,
not a real browser. Use isolated ports such as 29330/29331.

| item | observed |
|---|---|
| Build | `CARGO_TARGET_DIR=target/qa-m3-final cargo build --offline -p corvusd --bin corvusd` exit 0, `49.77` s |
| Result | `RESULT: 135 passed, 0 failed, 0 skipped`, exit 0 |
| Health | version `0.1.0`, protocol `2025-06-18`, ws `127.0.0.1:29330`, http `127.0.0.1:29331` |
| `tools/list` | exactly 43 frozen names, four boolean annotations, frozen schemas, optional `browser_id` on every tool |
| Recording | after initialize, `session_list` shows only the current session with steps 0; after calls the same session has positive steps; `session_steps` reports ordered steps |
| Replay dry run | `executed=[]`; control tools in `skipped` with reason `control_tool` |
| Replay live | one recorded safe step executes and returns `replay_session_id`; the replay session records its steps |
| Export | first JSONL line `type:"session"` carrying the session `id`, following lines `type:"step"` with ascending `seq` |
| Metrics | counts increase; `protocol` is the wire protocol integer (1); `errors_by_code` contains observed codes; `recording.db_path` is the temporary DB |
| `/metrics` | HTTP 200, `text/plain`, contains `corvus_tool_calls_total` and `corvus_uptime_seconds` |
| Logs | `logs_tail` returns entries; level filter works; invalid level is BAD_ARGS |
| Envelopes | `53` command envelopes recorded by the mock |

Fill the real-browser tables below from the current run; do not copy these
conformance values into the browser tables.

## 4. Live evidence

Record the actual values from the current desktop session. Reference values in
section 3 came from a mock, not from this browser.

| checkpoint | expected | observed | evidence |
|---|---|---|---|
| Extension id | same id as the loaded unpacked directory |  | screenshot-extensions.png |
| Browser id and capabilities | one browser, `["dom","scripting","debugger"]` |  | health-after.json |
| MCP session id | non-empty `Mcp-Session-Id` |  | init.headers |
| `session_list` initial | exactly the current session, `total=1`, steps 0, errors 0 |  | session-list-initial.json |
| `session_list` current session | same session id, transport `http`, status `active`, steps positive |  | session-list.json |
| `session_steps` | ascending `seq`, tool/args/result, `browser_id` and `tab_id` |  | session-steps.json |
| Replay dry run | planned includes browser tools, executed empty, control tools skipped |  | replay-dry.json |
| Replay live | `executed` has the selected step, `replay_session_id` present |  | replay-live.json |
| Replay session | `session_list` shows agent `corvus-replay`, transport `internal` |  | session-list-replay.json |
| Export | first line `type:"session"` with the session `id`, then `type:"step"` lines |  | session-export.json |
| Metrics | positive calls/errors; `protocol` is 1; recording path equals `--db` |  | metrics.json |
| `/metrics` | Prometheus text with the documented counters |  | metrics.txt |
| Logs | default, `info` and `error` filters return documented shapes |  | logs-*.json |

## 5. Real-browser happy path

Record each checkpoint once. Result is the parsed text payload or
JSON parsed from `content[0].text`; latency is client-side `curl %{time_total}` in
milliseconds. Use your own tab; never drive client tabs.

| # | tool | request arguments | key result fields (expected) | observed result | latency (ms) | evidence | PASS/FAIL |
|---|---|---|---|---|---|---|---|
| 1 | `session_list` | `{}` at session start | one current session, `total=1`, steps 0, errors 0 |  |  | session-list-initial.json |  |
| 2 | `browser_list` | `{}` | one entry with `browser_id`, `profile`, `version`, `capabilities`, `connected_at_ms` |  |  | browser-list.json |  |
| 3 | `tab_new` | `{"url":"http://127.0.0.1:8808/m2","active":true}` | new string `tab_id`, `loaded=true`, previous tab id |  |  | tab-new.json |  |
| 4 | `page_snapshot` | `{"tab_id":"<QA_TAB>","mode":"interactive"}` | nodes with refs, including input and button |  |  | snapshot.json |  |
| 5 | `page_text` | `{"tab_id":"<QA_TAB>"}` | contains `Corvus M2` |  |  | page-text.json |  |
| 6 | `page_find` | `{"query":"Find target","tab_id":"<QA_TAB>"}` | one or more matches with refs |  |  | page-find.json |  |
| 7 | `browser_click` | `{"target":"<FIND_REF>","tab_id":"<QA_TAB>"}` | `clicked=true`; page shows `find target clicked` |  |  | click.json, click-text.json |  |
| 8 | `browser_navigate` | `{"url":"http://127.0.0.1:8808/m2-b","tab_id":"<QA_TAB>"}` | `navigated=true`, `loaded=true`, final url |  |  | navigate-b.json |  |
| 9 | `page_text` | `{"tab_id":"<QA_TAB>"}` | contains page B title or text |  |  | page-text-b.json |  |
| 10 | `session_list` | `{"limit":20}` | current session, newest first, steps positive |  |  | session-list.json |  |
| 11 | `session_steps` | `{"session_id":"<SESSION>","limit":50}` | ordered steps; page tools carry `browser_id`/`tab_id` |  |  | session-steps.json |  |
| 12 | `session_steps` | `{"session_id":"<SESSION>","from_seq":<SEQ>,"limit":5}` | window starts at `from_seq`; `next_seq` when more remain |  |  | session-steps-page.json |  |
| 13 | `session_replay` | `{"session_id":"<SESSION>","mode":"dry_run"}` | `planned` lists recorded browser steps; `executed=[]`; control tools skipped |  |  | replay-dry.json |  |
| 14 | `session_replay` | `{"session_id":"<SESSION>","from_seq":<SAFE_SEQ>,"to_seq":<SAFE_SEQ>,"mode":"live","browser_id":"<BROWSER_ID>"}` | one executed step with `ok=true`; `replay_session_id` present |  |  | replay-live.json |  |
| 15 | `session_list` | `{"limit":20}` | replay session exists with agent `corvus-replay` |  |  | session-list-replay.json |  |
| 16 | `session_steps` | `{"session_id":"<REPLAY_SESSION>","limit":20}` | replay steps recorded |  |  | session-steps-replay.json |  |
| 17 | `session_export` | `{"session_id":"<SESSION>"}` | first line `type:"session"`; step lines ascending; `bytes`/`truncated` |  |  | session-export.json |  |
| 18 | `metrics` | `{}` | calls/errors positive; `by_tool`, `errors_by_code`, `recording` present |  |  | metrics.json |  |
| 19 | HTTP GET | `http://127.0.0.1:7331/metrics` | HTTP 200, Prometheus text with documented counters |  |  | metrics.txt |  |
| 20 | `logs_tail` | `{"limit":20}` | entries with `ts`/`level`/`target`/`message`/`fields`; `dropped` |  |  | logs-default.json |  |
| 21 | `logs_tail` | `{"limit":20,"level":"info"}` | only `info` entries |  |  | logs-info.json |  |
| 22 | `logs_tail` | `{"limit":20,"level":"error"}` | only `error` entries, possibly empty |  |  | logs-error.json |  |

## 6. Negative cases

Every row must return HTTP 200, `result.isError=true`, text
`CODE: message` plus trailing details JSON. A JSON-RPC `error` for a
tool-level failure is a defect.

| # | request | expected | observed | evidence | PASS/FAIL |
|---|---|---|---|---|---|
| 1 | `session_steps` with `session_id:"s-does-not-exist"` | NOT_FOUND, `details.session_id` |  | negative-session-steps-unknown.json |  |
| 2 | `session_export` with unknown session | NOT_FOUND, `details.session_id` |  | negative-session-export-unknown.json |  |
| 3 | `session_replay` with unknown session | NOT_FOUND, `details.session_id` |  | negative-session-replay-unknown.json |  |
| 4 | `session_replay` on a control-only session | BAD_ARGS, `details.reason=no_replayable_steps` |  | negative-replay-control.json |  |
| 5 | `session_replay` with `mode:"play"` | BAD_ARGS |  | negative-replay-mode.json |  |
| 6 | `session_replay` with `from_seq:9`, `to_seq:2` | BAD_ARGS |  | negative-replay-window.json |  |
| 7 | `session_steps` with `limit:0` | BAD_ARGS |  | negative-steps-limit.json |  |
| 8 | `session_list` with `limit:201` | BAD_ARGS |  | negative-list-limit.json |  |
| 9 | `session_export` with `format:"xml"` | BAD_ARGS |  | negative-export-format.json |  |
| 10 | `logs_tail` with `level:"verbose"` | BAD_ARGS, `details.level` |  | negative-logs-level.json |  |
| 11 | `logs_tail` with `limit:1001` | BAD_ARGS |  | negative-logs-limit.json |  |
| 12 | unknown tool `corvus_tool_that_does_not_exist` | BAD_ARGS with known tool list |  | negative-unknown-tool.json |  |
| 13 | `session_replay` live with no connected browser (optional) | TRANSPORT_DOWN |  | negative-replay-no-browser.json |  |

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

M3 does not add error codes. Recording and replay use these nine codes only.
A missing detail object where the contract specifies one is a defect.

## 8. Performance

| family | calls | p50 (ms) | p95 (ms) | max (ms) | budget |
|---|---|---|---|---|---|
| `session_list` / `session_steps` |  |  |  |  | < 1000 |
| `session_replay` dry run |  |  |  |  | < 1000 |
| `session_replay` live (one step) |  |  |  |  | < 3000 |
| `session_export` |  |  |  |  | < 2000 |
| `metrics` |  |  |  |  | < 1000 |
| `logs_tail` |  |  |  |  | < 1000 |
| HTTP `/metrics` |  |  |  |  | < 1000 |
| M2 browser tools on `/m2` |  |  |  |  | interactive < 1000 |

Recording must not add measurable latency to tools/call: compare the same tool
before and after enabling `--db` and report any regression above 20 percent.

## 9. Defects

Severity: S1 blocks the M3 exit, S2 wrong result or workaround required,
S3 cosmetic, documentation or observability gap.

| id | severity | checkpoint | expected | observed | repro |
|---|---|---|---|---|---|
|  |  |  |  |  |  |

M3-specific risks to check:

- A tools/call is missing from `session_steps`, or its `browser_id`/`tab_id`
  does not match the route that was used.
- A failed call is not recorded, or its `error_code` is wrong.
- A dry run executes browser work, or a live replay executes a control or tab
  tool.
- The replay session is missing, or its steps are attributed to the original
  session.
- `session_export` emits a non-JSON line, or step seq is not ascending.
- `metrics` counters do not increase after calls, or the `/metrics` body is not
  Prometheus text.
- `logs_tail` drops a startup event, or an invalid level is not BAD_ARGS.
- The daemon exits when the recorder cannot write; recording must never fail a
  tool call.

## 10. Environment notes

Record MSYS/bash version, Rust and cargo versions, Node.js and curl versions,
whether the daemon ran as a Windows service or a foreground process, and the
browser profile or extension state. Note any use of an existing Windows service
on 7330/7331: the acceptance flow must not restart or stop it and should use
isolated ports for automated runs.

## 11. Sign-off

| item | value |
|---|---|
| Verdict | `PASS / FAIL` |
| Passed checkpoints | `<n>` |
| Failed checkpoints | `<n>` |
| Skipped checkpoints | `<n>` |
| Conformance RESULT | `<copied line>` |
| Residual risks | `<text>` |
| Signed by | `<operator / date>` |