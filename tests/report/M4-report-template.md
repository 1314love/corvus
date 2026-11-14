# M4 acceptance report template

Copy this file to `M4-report-<date>.md` and fill every placeholder. Keep the
table rows; delete rows only when the scenario is genuinely not applicable and
record the reason in section 9. The M4 verdict is driven by the cancelled
response shape and by the two scheduling intervals, not by subjective
responsiveness.

## 1. Run metadata

| Field | Value |
|-------|-------|
| Report date (UTC+8) | `<YYYY-MM-DD HH:MM>` |
| Operator | `<name>` |
| Repository / commit | `D:/DSH/browser_bridege/corvus` / `<git rev-parse HEAD>` |
| Branch / working tree | `<branch>` / `<clean or list of files>` |
| Build target | `target/<target-dir>/debug/corvusd.exe` |
| Build result | `<exit code and elapsed time>` |
| Daemon command | `corvusd serve --db <temp db> --ws-port <p> --http-port <p+1>` |
| MCP endpoint | `http://127.0.0.1:<p+1>/mcp` |
| Fixture | `http://127.0.0.1:8808/m2` and `/m2-b` |
| Evidence directory | `<absolute path>` |
| Browser / extension | `<Edge version>` / `<extension id and version>` |
| Windows service | `<untouched or describe>` |

## 2. Build and connection checks

| Check | Command | Observed | Pass |
|-------|---------|----------|------|
| Build daemon | `cargo build --offline -p corvusd --bin corvusd` | `<exit code, time>` | `<yes/no>` |
| Health endpoint | `curl -fsS http://127.0.0.1:<p+1>/health` | `<version, protocol, ports>` | `<yes/no>` |
| Browser connected | `browser_list` | `<browser_id, capabilities>` | `<yes/no>` |
| MCP initialize | POST `initialize` | `<HTTP status, Mcp-Session-Id present>` | `<yes/no>` |
| Tools surface | `tools/list` | `<32 names>` | `<yes/no>` |

## 3. Conformance evidence

Record the exact automated command and the final line. The reference run on the
M4 tree produced 135 passed, 0 failed, 0 skipped.

```bash
CARGO=<cargo> CORVUSD_BIN=./target/<target-dir>/debug/corvusd.exe \
CORVUSD_ARGS="serve --ws-port 29330 --http-port 29331" \
CORVUS_DB="/tmp/corvus-m4-<epoch>.db" \
CORVUS_MCP_URL=http://127.0.0.1:29331/mcp \
CORVUS_WS_URL=ws://127.0.0.1:29330 \
CORVUS_HEALTH_URL=http://127.0.0.1:29331/health \
bash tests/conformance/run.sh
```

| Item | Observed |
|------|----------|
| Suite result | `RESULT: <N> passed, <N> failed, <N> skipped` |
| Exit code | `<0/1>` |
| Total checks | `<N>` |
| Mock envelopes | `<N>` (reference M4 run: 53) |
| Tools asserted | 32 |
| Schema snapshot | `tests/conformance/schema-snapshot.m3.json` matches |
| Parallel different-tab check | `<PASS/FAIL>`; wall `<ms>` for delay `<ms>` |
| Same-tab serial check | `<PASS/FAIL>`; start gap `<ms>` |
| Cancellation check | `<PASS/FAIL>`; first response `<ms>` |
| Cancel no-op check | `<PASS/FAIL>` |
| M3 regression | `<PASS/FAIL>`; session, replay, metrics, logs rows |

The reference M4 run measured: different tabs wall 481 ms with a 450 ms delay
and starts 1 ms apart; same tab starts 461 ms apart with the second after the
first end; cancellation error arrived 56 ms after the notification, while the
cancel POST itself returned 202 in 49 ms.

## 4. Live evidence

Record the manual evidence directory and the key files.

| Evidence | File | Present | Notes |
|----------|------|---------|-------|
| Health snapshot | `health.json` | `<yes/no>` | version and protocol |
| MCP session | `init.headers`, `init.json` | `<yes/no>` | session id redacted in report |
| Tool list | `tools.json` | `<yes/no>` | 32 names |
| Schema check | `schema-check.txt` | `<yes/no>` | no drift |
| Tabs before | `tabs-before.json` | `<yes/no>` | saved previous active tab |
| QA tabs | `tab-a.json`, `tab-b.json` | `<yes/no>` | own tabs only |
| Parallel waits | `wait-401.json`, `wait-402.json` | `<yes/no>` | both successful |
| Serial waits | `wait-403.json`, `wait-404.json` | `<yes/no>` | both successful |
| Serial steps | `session-steps-serial.json` | `<yes/no>` | gap `<ms>` |
| Cancelled request | `cancel-wait.json` | `<yes/no>` | code `-32800` |
| Cancel HTTP | `cancel.headers` | `<yes/no>` | 202 |
| Follow-up | `cancel-followup.json` | `<yes/no>` | normal result |
| No-op calls | `noop-first.json`, `noop-second.json` | `<yes/no>` | both successful |
| Cancelled step | `session-steps-cancel.json` | `<yes/no>` | `CANCELLED`, `ok=false` |
| Suite log | `conformance.log` | `<yes/no>` | includes RESULT |

## 5. Real-browser happy path

| # | Tool / check | Exact argument (trimmed) | Observed proving field | Pass |
|---|--------------|--------------------------|------------------------|------|
| 1 | `tools/list` | no arguments | 32 names; sorted list matches section 2 | `<yes/no>` |
| 2 | schema check | M3 snapshot | `schema snapshot matches 32 tools` | `<yes/no>` |
| 3 | `tab_list` | `{}` | active tab `<PREV_TAB>`; saved as `tabs-before.json` | `<yes/no>` |
| 4 | `tab_new` | `{"url":"http://127.0.0.1:8808/m2","active":true}` | `tab_id=<QA_TAB_A>`, `loaded=true` | `<yes/no>` |
| 5 | `tab_new` | same URL | `tab_id=<QA_TAB_B>`, `loaded=true` | `<yes/no>` |
| 6 | `browser_list` | `{}` | one browser `<BROWSER_ID>`; no ambiguity | `<yes/no>` |
| 7 | `browser_navigate` | tab A -> `/m2` | `navigated=true`, `loaded=true` | `<yes/no>` |
| 8 | `browser_navigate` | tab B -> `/m2` | `navigated=true`, `loaded=true` | `<yes/no>` |
| 9 | `browser_wait` | id 401, tab A, `#late`, 5000 ms | `matched=true`; no error | `<yes/no>` |
| 10 | `browser_wait` | id 402, tab B, `#late`, 5000 ms | `matched=true`; no error | `<yes/no>` |
| 11 | parallel interval | steps 401 and 402 | intervals overlap; wall `<ms>` | `<yes/no>` |
| 12 | `browser_navigate` | tab A -> `/m2` | timer reset; `loaded=true` | `<yes/no>` |
| 13 | `browser_wait` | id 403, tab A, `#late`, 5000 ms | `matched=true` | `<yes/no>` |
| 14 | `browser_wait` | id 404, tab A, `#late`, 5000 ms | `matched=true` | `<yes/no>` |
| 15 | serial interval | steps 403 and 404 | second start >= first end; gap `<ms>` | `<yes/no>` |
| 16 | `browser_wait` | id 501, tab A, `#never`, 30000 ms | request in flight; cancel sent | `<yes/no>` |
| 17 | cancellation | notification `requestId=501` | HTTP `202`; first response `-32800` | `<yes/no>` |
| 18 | `page_text` | id 502, tab A | normal result; text length `<n>` | `<yes/no>` |
| 19 | `session_steps` | current session, limit 30 | `CANCELLED`, `ok=false` | `<yes/no>` |
| 20 | `page_text` | id 601, tab A | normal result | `<yes/no>` |
| 21 | cancel finished | notification `requestId=601` | HTTP `202`; no error | `<yes/no>` |
| 22 | cancel unknown | notification `requestId=999999` | HTTP `202`; ignored | `<yes/no>` |
| 23 | `page_text` | id 602, tab A | normal result; session usable | `<yes/no>` |
| 24 | `tab_close` | tab B | `closed=true` | `<yes/no>` |
| 25 | `tab_close` | tab A | `closed=true` | `<yes/no>` |
| 26 | `tab_switch` | `<PREV_TAB>` | previous client tab active | `<yes/no>` |
| 27 | health after | `GET /health` | browser still connected | `<yes/no>` |
| 28 | `session_list` | `{}` | current session present; steps recorded | `<yes/no>` |
| 29 | `metrics` | `{}` | counters include CANCELLED | `<yes/no>` |
| 30 | `logs_tail` | `{"level":"info","limit":10}` | entries present | `<yes/no>` |

Rows 28-30 are M3 regression checks. They are not part of the M4 scheduling
contract but catch a regression caused by cancellation bookkeeping.

## 6. Negative cases

| # | Scenario | Call | Expected | Observed | Pass |
|---|----------|------|----------|----------|------|
| 1 | Cancel in-flight wait | `browser_wait` 501 `#never` 30000 ms; cancel 501 | JSON-RPC `-32800`; no tool result; under 2 s | `<code, ms>` | `<yes/no>` |
| 2 | Cancel finished request | `page_text` 601; cancel 601 | HTTP 202; no error; later calls work | `<202>` | `<yes/no>` |
| 3 | Cancel unknown id | notification `requestId=999999` | HTTP 202; ignored | `<202>` | `<yes/no>` |
| 4 | Follow-up after cancel | `page_text` 502 same session | Normal tool result | `<text length>` | `<yes/no>` |
| 5 | Same-tab overlap attempt | `browser_wait` 403/404 one tab | Second starts at/after first end | `gap <ms>` | `<yes/no>` |
| 6 | Different-tab concurrency | `browser_wait` 401/402 two tabs | Intervals overlap; wall near one wait | `wall <ms>` | `<yes/no>` |
| 7 | Schema drift | `tools/list` vs M3 snapshot | 32 tools; no changed schema | `<drift list>` | `<yes/no>` |
| 8 | Wrong session for cancel | notification with another/missing session | Rejected or not applied; original not cancelled | `<observed>` | `<yes/no/n.a.>` |
| 9 | Unknown tool | `tools/call` name `not_a_tool` | `BAD_ARGS`; 32 tools remain | `<code>` | `<yes/no>` |
| 10 | Cancel after disconnect | close test session; cancel 501 | No effect; reconnect still 32 tools | `<observed>` | `<yes/no/n.a.>` |

For case 8, do not create a second client session against a production daemon.
If the environment has only one session, mark it `n.a.` and reference the
automated two-session coverage.

## 7. Error codes observed

| Code | Where | Meaning in M4 | Count |
|------|-------|---------------|-------|
| `-32800` | JSON-RPC error for cancelled request | Request cancelled; not a tool result | `<n>` |
| `CANCELLED` | `session_steps.error_code` | Recorded cancelled step; `ok=false` | `<n>` |
| `BAD_ARGS` | tool result error | Invalid arguments, including unknown tool | `<n>` |
| `TIMEOUT` | tool result error | Normal wait/download timeout; cancellation is not TIMEOUT | `<n>` |
| `TRANSPORT_DOWN` | tool result error | Browser/extension disconnected | `<n>` |
| `NOT_FOUND` | tool result error | Unknown tab/download/session id | `<n>` |

Do not report `-32800` as an MCP transport failure or as a tool `isError`
result. It is the first response to the original `tools/call`; HTTP status is
200 and the body is a JSON-RPC error.

## 8. Performance

| Measurement | Observed | Budget / reference |
|-------------|----------|--------------------|
| Parallel two-tab wall time | `<ms>` | Near one wait; reference 481 ms at 450 ms delay |
| Parallel interval overlap | `<ms>` | Positive overlap; reference starts 1 ms apart |
| Same-tab start gap | `<ms>` | Non-negative; reference 461 ms |
| Cancel notification -> error | `<ms>` | < 2000 ms; reference 56 ms |
| Cancel HTTP response | `<ms>` | 202; reference 49 ms |
| Follow-up call after cancel | `<ms>` | Normal tool latency; no reconnect |
| Baseline tool latency | `<ms>` | Compare with M3 report |

If a wait is intentionally slow, state the configured delay next to the
measurement. Do not compare wall times from different delays.

## 9. Defects and deviations

| Severity | Area | Description | Reproduction | Status |
|----------|------|-------------|--------------|--------|
| S1 / S2 / S3 | `<scheduling/cancel/schema/docs>` | `<what happened>` | `<exact ids, commands, evidence file>` | `<open/fixed>` |

Severity guide: S1 blocks M4 exit (same-tab overlap, success result for a
cancelled request, cancellation over 2 s); S2 produces a wrong result or needs a
workaround; S3 is cosmetic, latency, or documentation only.

Deviations from the playbook (port, fixture, skipped row) go here with a reason.
The reference run had no product defects. Two harness corrections are documented
in the QA history but are not product defects: the timing file must not be
deleted while the mock holds it open, and schema comparison must read
`result.tools` from the JSON-RPC body.

## 10. Environment notes

- Record the exact Windows build, Edge version, extension id/version, Node
  version, and Rust toolchain.
- State whether the installed Windows service on 7330/7331 was running. It must
  be untouched by this test.
- State the fixture port and whether it was stopped after the run.
- Record any browser popup, extension reload, or reconnect that happened during
  the run. A reconnect during the cancellation row invalidates that row.

## 11. Sign-off

| Item | Value |
|------|-------|
| Automated result | `<N> passed, <N> failed, <N> skipped` |
| Manual result | `<N> passed, <N> failed` |
| M4 verdict | `<PASS / FAIL>` |
| Open defects | `<none or ids>` |
| Evidence retained until | `<date>` |
| QA sign-off | `<name, date>` |
| Lead acceptance | `<name, date>` |

A PASS requires the cancellation row, both scheduling rows, and the 32-tool
schema row to pass. A single S1 defect fails the report regardless of the other
rows.
