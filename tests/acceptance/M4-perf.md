[English](M4-perf.md) | [中文](M4-perf.zh.md)

# M4 performance and memory evidence: NFR-1 to NFR-4

This playbook is the repeatable performance and memory evidence procedure for
the Corvus NFRs. The English text is the source of truth; [M4-perf.zh.md](M4-perf.zh.md)
mirrors its sections and commands.

## 1. Purpose

Produce one measured pass that answers four questions with client-observed
numbers:

- NFR-1: are `browser_click`, `browser_type` and `page_snapshot` fast enough
  on a 10,000-element page?
- NFR-2: do ten tabs and ten MCP clients work concurrently without serializing
  into one queue?
- NFR-3: does the daemon stay within its memory-growth budget under sustained
  mixed load, and what is the call success rate?
- NFR-4: are the release daemon binary and the extension bundle under their size
  limits?

The pack is deterministic by default. A mock extension supplies fixed latency,
so a rerun compares like with like. A separate smoke row uses dedicated headless
Edge only to confirm the same fixture and snapshot path in a real extension.
The smoke never uses a user's browser, never changes the installed Windows
service, and creates and closes its own tab.

Run the whole pack with one command. The final summary table is the acceptance
artifact; this document records the method and the reference result.

## 2. Scope and NFR targets

| NFR | Measurement in this pack | Target |
|-----|--------------------------|--------|
| NFR-1 | `browser_click` p95, `browser_type` p95 | below 50 ms |
| NFR-1 | `page_snapshot` p95 on a 10,000-element page | below 300 ms |
| NFR-2 | 10 tabs issue `browser_wait` and then `browser_navigate` in parallel | all calls succeed, wall time near one command |
| NFR-2 | 10 MCP streamable-HTTP sessions initialize and call `browser_list` | 10/10 connected, 10/10 calls succeed |
| NFR-3 | daemon RSS growth over 900 s mixed load plus 120 s idle | below 50 MB |
| NFR-3 | mixed-load success rate | at least 99.9% |
| NFR-4 | release `corvusd.exe` size | below 25 MB |
| NFR-4 | `extension/` bundle size | below 1 MB |

The requirements source is `docs/01-requirements.md`, lines 195-198. NFR-3 in
the requirements document is a 72-hour soak. This pack measures a 15-minute
mixed-load window plus a 2-minute idle tail as a repeatable proxy. It does not
claim a 72-hour soak result; the report must say so explicitly.

The 10,000-element fixture is `tests/fixtures/perf-10k.html`. Its DOM count is
deterministic: `document.querySelectorAll('*').length === 10000`.

## 3. Prerequisites

- Windows 10/11 with MSYS bash, `curl`, and `netstat.exe`.
- Node.js 20 or newer.
- Rust and Cargo; this repository's MSYS path is
  `/c/Users/Administrator/.cargo/bin/cargo.exe`.
- The repository is the current working directory:
  `D:/DSH/browser_bridege/corvus`.
- For the live row, a dedicated headless Edge on `127.0.0.1:9333` with the
  unpacked Corvus extension loaded, and the installed Windows Corvus service
  healthy on `127.0.0.1:7330` (WebSocket) and `http://127.0.0.1:7331/mcp`.
  The expected browser id is `edge:b5e09e58ecdf3caa`.
- Do not use a personal browser for the live row. Do not restart or stop the
  Windows service for any row. Do not reuse its database for the daemon under
  test.

The mock rows use their own daemon, temporary database, fixture server, and
ports, so they cannot interfere with the installed service.

## 4. Build the daemon and mock extension

The full run builds a release daemon into `target/perf-release`, builds the
mock extension, and then starts everything itself:

```bash
cd D:/DSH/browser_bridege/corvus
CARGO=/c/Users/Administrator/.cargo/bin/cargo.exe bash tests/perf/run.sh
```

Defaults used by `run.sh`:

- isolated daemon: WebSocket `31330`, HTTP `31331`;
- temporary database: `$TMP/corvus-perf.db`;
- fixture server: an ephemeral port chosen by `serve-fixture.mjs`;
- mock browser id: `mock:perf`, with a 250 ms delayed wait for `#delay`;
- live row: existing service `http://127.0.0.1:7331/mcp` and browser
  `edge:b5e09e58ecdf3caa`;
- evidence directory: `$TMP/corvus-perf.XXXXXX/evidence`, retained by default.

To use an existing daemon binary, set `CORVUSD_BIN`; to change ports, set
`PERF_WS_PORT` and `PERF_HTTP_PORT`; to change sample counts, set
`PERF_LATENCY_SAMPLES` and `PERF_LIVE_SAMPLES`. The acceptance run always
requires the live row.

A short smoke run is useful while editing the harness:

```bash
PERF_MEMORY_SECONDS=10 PERF_IDLE_SECONDS=5 PERF_LATENCY_SAMPLES=5 \
CARGO=/c/Users/Administrator/.cargo/bin/cargo.exe bash tests/perf/run.sh
```

## 5. Fixture page and servers

`tests/fixtures/perf-10k.html` is 70,226 bytes and 11 lines. It contains seven
structural elements (html, head, title, body, main, button, input) and 9,993
empty `<i>` leaves, for exactly 10,000 elements. There are no scripts, images,
styles, or external requests, so load and node traversal are deterministic.

Start the static server directly when diagnosing a row:

```bash
PORT=0 HOST=127.0.0.1 node tests/perf/serve-fixture.mjs
```

It prints `READY http://HOST:PORT` and serves:

| Path | Content |
|------|---------|
| `/`, `/perf-10k`, `/perf-10k.html` | the 10,000-element fixture |
| `/load` | a small page with `#perf-click` and `#perf-type` for the memory mix |

The mock extension returns a synthetic 3,000-node snapshot payload when the
current tab URL contains `/perf-10k`; that is the NFR-1 payload row. It returns
its normal small snapshot for other URLs. The live extension walks the same
10,000-element DOM, but its full snapshot mode reports only nodes that pass its
`isInteresting` predicate (actions, landmarks, headings, images, explicit roles,
and similar). Empty `<i>` leaves are traversed and filtered out, so a live
snapshot legitimately reports a small `node_count` even on this page. The
10,000-element DOM count is the live stress; the mock row covers a 3,000-node
serialized response.

## 6. Benchmark modes and evidence files

`tests/perf/bench.mjs` has six modes. `run.sh` runs all five measurement modes
and then `summary`.

| Mode | Output file | What it records |
|------|-------------|-----------------|
| `latency` | `latency.json` | 60 samples per tool, 8 warmups, p50/p95/max/mean |
| `concurrency` | `concurrency.json` | 10-tab waits, 10-tab navigates, 10 MCP clients |
| `memory` | `memory.json` | RSS samples, call success, growth, peak |
| `live` | `live.json` | dedicated headless Edge smoke on the fixture |
| `size` | `size.json` | release binary bytes, extension tree bytes and files |
| `summary` | `summary.txt` | NFR table and the final `RESULT` line |

Example direct invocation of one mode:

```bash
node tests/perf/bench.mjs latency \
  --url http://127.0.0.1:31331/mcp \
  --browser-id mock:perf \
  --fixture-url http://127.0.0.1:PORT/perf-10k.html \
  --samples 60 --out /tmp/latency.json
```

Every client call is a JSON-RPC `tools/call` over MCP streamable HTTP. The
benchmark records elapsed wall time around the HTTP POST and the response parse.
It uses `structuredContent` when present and falls back to the text JSON part.
A call is successful only when the HTTP status is 200, there is no JSON-RPC
error, and `isError` is false.

## 7. Latency measurement: method

The latency row opens one tab at the fixture URL and navigates it once. It then
runs the following sequence in one MCP session and one tab:

1. eight warmup calls per tool;
2. 60 measured calls per tool;
3. `browser_click` on `#perf-click`;
4. `browser_type` on `#perf-type` with text `perf` and `clear: true`;
5. `page_snapshot` with default arguments;
6. `page_snapshot` with `mode: "full"` and `max_nodes: 3000`.

Percentiles use nearest rank on the sorted 60 samples: p50 is element 30 and p95
is element 57 for this sample size. Reporting p50 and p95, not only a mean,
makes a single slow call visible. The measurement is end to end from the client
to the daemon, mock extension, and response parse; it is not an extension-only
timer.

The mock's 250 ms wait delay is not used by the latency row. It is used only by
the concurrency row and must be stated next to that result.

## 8. Latency results

The reference full run measured:

| Tool | Samples | p50 | p95 | Max | Target | Verdict |
|------|---------|-----|-----|-----|--------|---------|
| `browser_click` | 60 | 1 ms | 2 ms | 2 ms | below 50 ms | PASS |
| `browser_type` | 60 | 1 ms | 2 ms | 2 ms | below 50 ms | PASS |
| `page_snapshot` | 60 | 1 ms | 2 ms | 12 ms | below 300 ms | PASS |
| `page_snapshot` full, 3,000 nodes | 60 | 1 ms | 2 ms | 2 ms | below 300 ms | PASS |

These are mock numbers: they isolate protocol, daemon, scheduling, and response
serialization from browser rendering. The live smoke in section 11 adds the
real-extension measurement. A mock-only pass is necessary but not sufficient;
the summary table requires both the mock NFR-1 rows and the live snapshot row.

## 9. Concurrency and MCP clients

The concurrency row creates ten tabs at the fixture URL. It sends ten
`browser_wait` calls in parallel, each with its own `tab_id`, selector
`#delay`, and an 8-second timeout. With the mock configured at 250 ms, a
correct per-tab scheduler should finish all ten in a little over 250 ms, not
2.5 seconds. It then sends ten `browser_navigate` calls in parallel. Finally it
opens ten independent MCP sessions, initializes each, and sends one
`browser_list` call per session.

Reference result:

| Row | Observed | Target | Verdict |
|-----|----------|--------|---------|
| 10 `browser_wait` on 10 tabs | 10/10 in 273 ms | all succeed, near 250 ms | PASS |
| 10 `browser_navigate` on 10 tabs | 10/10 in 7 ms | all succeed | PASS |
| 10 concurrent MCP sessions | 10/10 connected | 10/10 | PASS |
| 10 `browser_list` calls | 10/10 in 5 ms | 10/10 | PASS |
| Session open wall time | 11 ms | no reconnect storm | PASS |

The 273 ms wall time is consistent with ten concurrent 250 ms waits. A result
near 2,500 ms would mean the scheduler serialized different tabs and is a
product defect, not a harness variance.

## 10. Memory measurement

The memory row resolves the daemon's Windows process id from the listening HTTP
port with `netstat -ano`. Every 2 seconds it reads
`(Get-Process -Id PID).WorkingSet64` through PowerShell and records the value
in bytes. It starts the first sample before the workers start and always takes a
final sample after the idle tail.

Load profile:

- 900 seconds of mixed load;
- 3 concurrent workers;
- one tool call every 150 ms per worker while a call is not in flight;
- tool mix: `browser_list`, `tab_list`, `page_text`, `page_find`,
  `browser_click`, `browser_type`, `browser_wait`, `net_log`,
  `download_list`, `logs_tail`;
- 120 seconds of idle after the workers stop.

The tool mix includes daemon-local tools, page perception, page actions, network
reads, downloads, and log reads. It is deliberately not a single hot tool. Call
success is counted independently; a tool error is recorded and lowers the
success rate instead of aborting the run.

Reference result:

| Item | Observed | Target | Verdict |
|------|----------|--------|---------|
| Load duration | 900 s | 900 s | PASS |
| Idle tail | 120 s | 120 s | PASS |
| RSS samples | 510 | at least 500 | PASS |
| Calls attempted | 16,143 | representative | PASS |
| Call errors | 0 | 0 | PASS |
| Success rate | 100% | at least 99.9% | PASS |
| RSS start | 9,428,992 | reference only | PASS |
| RSS end | 11,599,872 | reference only | PASS |
| RSS max | 11,796,480 | reference only | PASS |
| Growth (end minus start) | 2.07 MB | below 50 MB | PASS |
| Peak growth (max minus start) | 2.26 MB | below 50 MB | PASS |

This is a 15-minute proxy for NFR-3, not the 72-hour soak. The report must keep
that distinction. The success rate is over every attempted call, including
errors that occur during the idle tail. A single error in roughly 18,000 calls
still rounds to 99.99%, but the exact count must be reported, not just the
percentage.

## 11. Live headless Edge smoke

The live row calls the installed service at `http://127.0.0.1:7331/mcp` and
the dedicated headless Edge browser `edge:b5e09e58ecdf3caa`. It creates an
active tab at the fixture URL, verifies the DOM count, and measures 60 samples
of default snapshot, full snapshot, click, and type. The tab is created active
on purpose: the extension's type path waits for a rendered frame, and a
background headless tab can suspend `requestAnimationFrame`. That is a
headless background-tab limitation, not a product NFR failure; a user's active
tab is not affected.

Reference result:

| Item | Observed | Target | Verdict |
|------|----------|--------|---------|
| DOM elements | 10,000 | exactly 10,000 | PASS |
| Default snapshot nodes | 2 | informational | PASS |
| Full snapshot nodes | 3 | informational | PASS |
| Default snapshot p95 | 22 ms | below 300 ms | PASS |
| Full snapshot p95 | 20 ms | below 300 ms | PASS |
| Click p95 | 4 ms | below 50 ms | PASS |
| Type p95 | 19 ms | below 50 ms | PASS |

The live snapshot node counts are lower than 3,000 because the extension's full
mode filters through `isInteresting`. The DOM count proves the page size; the
mock row in section 8 proves the 3,000-node response path. Report both facts and
do not present the live node count as a failure or as the payload size.

## 12. Size measurement

The size row measures the release daemon binary used by the run and the whole
`extension/` directory tree. Sizes use decimal megabytes (1,000,000 bytes), the
same unit as the NFR budget.

Reference result:

| Artifact | Bytes | MB | Target | Verdict |
|----------|-------|----|--------|---------|
| release `corvusd.exe` | 6,334,464 | 6.33 | below 25 MB | PASS |
| `extension/` bundle | 232,885 | 0.23 | below 1 MB | PASS |

The extension count is all files under `extension/`, including README and
popup assets. A debug daemon binary is not an NFR-4 artifact and must not be
reported here.

## 13. Negative and boundary matrix

These rows prove the harness fails closed instead of reporting a false pass.
They are test-harness checks, not product API errors.

| Case | Expected behavior | Evidence |
|------|-------------------|----------|
| daemon build fails | runner prints the build log and exits non-zero | `build-daemon.log` |
| fixture server does not print READY | runner prints the fixture log and exits non-zero | `fixture.log` |
| isolated daemon does not become healthy | runner prints `daemon.log` and exits non-zero | `daemon.log` |
| mock does not register `mock:perf` | runner prints `mock.log` and exits non-zero | `mock.log` |
| daemon PID cannot be resolved | runner exits non-zero before memory sampling | run output |
| fixture page DOM count is not 10,000 | live row fails its explicit assertion | `live.json` absent or fail |
| live service or headless Edge unavailable | live row exits non-zero; no partial summary | run output |
| any NFR row misses | `summary.txt` prints MISS and exits 1 | `summary.txt` |
| a latency, concurrency or live call receives a tool error | that mode exits non-zero and stops later modes; memory mode instead records the error and reports success | run output |
| port `31330`/`31331` already in use | daemon start or health check fails | `daemon.log` |

A run is not accepted if the summary is missing, if any row is MISS, or if the
final line is not `RESULT: PASS 13 checks`. Do not add sleeps or retries to turn
a transient failure into a pass; fix the environment and rerun the full pack.

## 14. Evidence to record

For a reference run, retain the whole temporary run root. Its `evidence/`
directory contains:

- `latency.json`, `concurrency.json`, `memory.json`, `live.json`,
  `size.json`;
- `summary.txt` with the NFR table and final result.

The build and process logs (`build-daemon.log`, `build-mock.log`,
`daemon.log`, `fixture.log`, `mock.log`, `mock.jsonl`) sit next to
`evidence/` in that run root. The runner's stdout prints the
`EVIDENCE <path>` pointer.

The report must record the exact run command, UTC+8 start/end time, repository
commit, `corvusd` version and protocol, Node and Rust versions, Edge version,
browser id, fixture URL, and the evidence path. The reference run for this
document used:

- start: 2026-11-14 19:20:46 UTC+8; end: 2026-11-14 19:38:54 UTC+8; duration: 18 min 8 s;
- repository: HEAD 195f8fe5a16c1a28acde34534508f2dbca91d5f8, with uncommitted product and QA changes present at run time;
- daemon: corvusd 0.1.0, MCP protocol 2025-06-18, release build;
- toolchain: Node v24.21.0, cargo 1.99.1;
- live browser: Edge 157.0.4341.46, browser id edge:b5e09e58ecdf3caa, headless endpoint 127.0.0.1:9333;
- fixture: http://127.0.0.1:56722/perf-10k.html;
- evidence: `C:/Users/Administrator/AppData/Local/Temp/corvus-perf.57cjxl/evidence`;
- command: `CARGO=/c/Users/Administrator/.cargo/bin/cargo.exe bash tests/perf/run.sh`;
- summary: `RESULT: PASS 13 checks`.

Raw evidence contains absolute paths and local browser identifiers. Treat it as
internal QA evidence; do not commit it to the repository.

## 15. Pass criteria and interpretation

A performance pass requires all of the following:

1. the final summary line is `RESULT: PASS 13 checks`;
2. click and type p95 are below 50 ms in the mock latency row;
3. snapshot and snapshot-full p95 are below 300 ms in the mock latency row;
4. all ten parallel waits and all ten parallel navigates succeed, and the wait
   wall time is near one 250 ms delay rather than ten times it;
5. ten MCP sessions connect and all ten calls succeed;
6. daemon RSS growth is below 50 MB and mixed-load success is at least 99.9%;
7. the release binary is below 25 MB and the extension bundle is below 1 MB;
8. the live row reports exactly 10,000 DOM elements and live full-snapshot p95
   is below 300 ms.

The mock numbers are not a substitute for real-browser interaction quality. The
live row is a smoke, not a full browser benchmark: it measures one page, one
headless Edge build, and one extension build. The 15-minute memory row is not a
72-hour soak. State both limits in any release report.

If a target misses, record the measured value, the exact tool and arguments, the
sample count, and the evidence file. Do not tune the fixture or the sample set
after seeing the result. A missed NFR is a defect report with numbers, not a
reason to weaken the test.

## 16. Cleanup and troubleshooting

The runner's exit trap kills the mock, fixture server, and isolated daemon,
removes the temporary database and its WAL/SHM files, and prints the evidence
path. It never stops or restarts the installed Windows service. The live row
closes its own tab.

Common fixes:

| Symptom | Fix |
|---------|-----|
| `cargo: command not found` | set `CARGO=/c/Users/Administrator/.cargo/bin/cargo.exe` |
| daemon health never becomes ready | free `31330`/`31331` or set `PERF_WS_PORT`/`PERF_HTTP_PORT` |
| fixture server does not start | run `node tests/perf/serve-fixture.mjs` directly and check the `READY` line |
| live row cannot find the browser | verify the service on 7331 shows `edge:b5e09e58ecdf3caa` and headless Edge on 9333 is running |
| live type hangs | keep the live tab active; do not test `browser_type` in a background headless tab |
| memory RSS read fails | confirm the PID resolved from `netstat -ano` is the `corvusd.exe` process |
| summary reports MISS | inspect the matching JSON row, fix the environment or product, and rerun all modes |

To keep a failed run for diagnosis, do not delete the printed evidence
directory. To discard evidence deliberately, set `PERF_KEEP_TMP=0`; acceptance
runs must keep it.
