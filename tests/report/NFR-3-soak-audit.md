# NFR-3 soak audit: run 1 (superseded) and run 2 clean baseline

- Task: task-52 (run 1 audit) and task-53 (clean-run review), owner qa.
- Audit time: 2026-11-14T14:38Z for run 1; 2026-11-14T14:45Z (22:45 UTC+8) for the run 2 baseline review.
- Mode: mock extension, isolated daemon 29430/29431, temp DB, 72 h duration, 3600 s checkpoints.
- Run 1 evidence root: C:/Users/Administrator/AppData/Local/Temp/corvus-nfr3-soak-20261114T2025
- Run 2 evidence root: C:/Users/Administrator/AppData/Local/Temp/corvus-nfr3-soak-clean-20261114T144131
- Harness: tests/perf/soak.sh and tests/perf/soak.mjs at commit db6106b11716a6441acd86be6dd3243fa2b5b810.
- Local harness hashes: soak.sh c4489fed4dafcb33e1da4f5d19648f8f9d805bbddfaa7d1e7f42e589262c39fc, soak.mjs 005a8e55826e4bfbf1caeada4c3e966633354093905ed3d80250c66c51134a06; both match the committed blobs.
- Repository HEAD at audit: 1f8611811d52470819f9f6f5568348d0fc5c52b6.
- Audit scope: read-only. QA did not restart, stop, or edit either soak, its state.json, hourly.jsonl, or summary. Run 2 was started by the operator after the run 1 audit.

## Run status

- **Run 1 - SUPERSEDED.** Run 1 cannot prove 72 h continuous daemon uptime because the daemon was replaced at 2026-11-14T13:27:44.986Z; daemon uptime and tool_calls_total reset. Its checkpoint table is retained below as continuity evidence only.
- **Run 2 - ACTIVE.** Clean restart at 2026-11-14T14:41:37.572Z; baseline verified in the Run 2 section below. QA has not restarted or edited it.

## Run 1 verdict (SUPERSEDED)

**Run 1: PASS for acceptance criteria 1-5, but SUPERSEDED for NFR-3 72 h evidence.** One documented continuity anomaly: the daemon was replaced at 13:27:44.986Z, between checkpoint 1 and checkpoint 2. The checkpoint sequence is contiguous, the original start is preserved, and all recomputed NFR values pass. The task description refers to a resume at about 13:41Z; the evidence shows the replacement daemon started at 13:27:44.986Z and no process or log evidence of a 13:41 restart. The preservation requirement is still met and the exact-time discrepancy is recorded below.

## Evidence and commands

Read-only checks used:

    date '+%Y-%m-%dT%H:%M:%S%z'
    curl -sS --max-time 5 http://127.0.0.1:29431/health
    read state.json, hourly.jsonl, NFR-3-soak-summary.txt, daemon.log, mock.log, health-start.json
    tasklist.exe /FI "PID eq 28144" /FO CSV /NH
    powershell.exe -NoProfile -NonInteractive -Command 'Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -like "*soak*" } | Select-Object ProcessId,ParentProcessId,Name,CreationDate,CommandLine | Format-List'
    git rev-parse HEAD
    git show db6106b:tests/perf/soak.sh | sha256sum
    git show db6106b:tests/perf/soak.mjs | sha256sum

Process evidence at audit time: bash.exe PID 12048 (soak.sh), node.exe PID 21156 (the soak.mjs driver), corvusd.exe PID 28144 (replacement daemon), mock-extension.exe PID 23740. The node launcher chain is bash 12048 -> niu 12904 -> node 28128 -> cmd 15512 -> node 21156; this is one driver chain, not two independent drivers. The daemon was listening on 29430 and 29431 and /health was healthy.

## Run 1 checkpoint table (continuity evidence)

| index | at (UTC) | elapsed s | RSS bytes | RSS growth bytes | daemon calls | errors | success % | pending | daemon uptime ms | sessions | steps | load attempted/ok/failed |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| 0 | 12:25:05.822 | 0.03 | 7,770,112 | 0 | 1 | 0 | 100 | 1 | 339 | 2 | 1 | 0 / 0 / 0 |
| 1 | 13:25:05.796 | 3600.01 | 10,665,984 | 2,895,872 | 3,582 | 0 | 100 | 1 | 3,600,323 | 2 | 3,582 | 3,580 / 3,580 / 0 |
| 2 | 14:25:05.801 | 7200.01 | 10,989,568 | 3,219,456 | 3,423 | 0 | 100 | 1 | 3,441,012 | 4 | 7,116 | 7,092 / 7,092 / 0 |

Checkpoint indices are exactly 0, 1, 2, with no duplicate and no gap. The start-to-start intervals are 3,599,974 ms (3,599.974 s) and 3,600,005 ms (3,600.005 s), both within 26 ms of 3600 s. elapsed_seconds is strictly increasing: 0.03, 3600.01, 7200.01. baseline is true only at index 0; catchup and final are false at all three checkpoints.

## Run 1 recomputed values

- Success rate from cumulative load counters: 7,092 ok / 7,092 completed = 100.00%, above the 99.9% NFR-3 threshold. The daemon view at checkpoint 2 is 3,423 calls and 0 errors = 100%; checkpoint 1 is 3,582 calls and 0 errors.
- tool_errors_total: 0 at checkpoint 2; 0 at checkpoint 1; current /health also reports 0.
- consecutive_health_failures: 0; all three checkpoints have health_ok true and no health_error.
- RSS start: 7,770,112 bytes. Newest checkpoint RSS: 10,989,568 bytes. Growth: 10,989,568 - 7,770,112 = 3,219,456 bytes; 3,219,456 / 1,048,576 = 3.0703 MiB, rounded to 3.07 in the summary. This is below the 50 MiB (52,428,800 byte) limit, with 49,209,344 bytes of headroom.
- Checkpoint 1 cross-check: 10,665,984 - 7,770,112 = 2,895,872 bytes = 2.7617 MiB; the record and summary both say 2.76.
- Load cumulative at checkpoint 2: 7,092 attempted, 7,092 ok, 0 failed; the interval from checkpoint 1 is 3,512 attempted, 3,512 ok, 0 failed.
- Current non-checkpoint /health snapshot at 14:37Z: tool_calls_total 4,126, tool_errors_total 0, recording steps 7,819, uptime 4,146,662 ms. state.cumulative at 14:37:46Z was 7,848 / 7,848 / 0. These are liveness values, not checkpoint values, and are not used for the verdict.

## Run 1 summary cross-check

The summary file NFR-3-soak-summary.txt matches checkpoint 2:

- last_checkpoint_index 2 and last_checkpoint_utc 2026-11-14T14:25:05.801Z match hourly line index 2.
- next_checkpoint_utc 2026-11-14T15:25:05.790Z equals started_ms 1794659105790 + 3 x 3,600,000 ms; it is the next scheduled checkpoint.
- rss_start_bytes 7,770,112, rss_latest_bytes 10,989,568, rss_growth_mb 3.07, and all RSS fields match the checkpoint 2 record.
- load_attempted 7,092, load_ok 7,092, load_failed 0, success_rate_pct 100, tool_errors_total 0, pending_inflight 1, and consecutive_health_failures 0 all match checkpoint 2.
- status running and verdict on track are consistent with a healthy in-progress run; the summary intentionally reflects the newest hourly checkpoint, not the 30-second state.json updates.

## Run 1 anomalies and limitations

1. Daemon replacement between checkpoint 1 and checkpoint 2. The original daemon uptime at checkpoint 1 was 3,600,323 ms. At checkpoint 2 it was 3,441,012 ms, and /health started_at is 1794662864986 = 2026-11-14T13:27:44.986Z. daemon.log begins at 13:27:44.989Z. This is a restart, consistent with a resume, but it resets daemon-scoped counters: tool_calls_total 3,582 -> 3,423 and recording.sessions 2 -> 4. The cumulative load counters and the original started_ms were not reset. There is no checkpoint gap or duplicate.
2. Resume-time discrepancy. The task description says the resume was at about 13:41Z; the only restart evidence is 13:27:44.986Z. I found no process, log, or state timestamp for a 13:41Z restart. The state start is unchanged either way; the exact stated resume time is not corroborated.
3. daemon.log was truncated by the restart and contains only the current process (5 lines). The original daemon log is not available. The baseline and checkpoint 1 records in hourly.jsonl plus state.started_ms remain the authoritative start evidence.
4. No explicit resume marker or resume counter exists in state.json. The resume is evidenced by process creation, daemon.log, health.started_at, and the daemon_uptime_ms reset. This is an observability gap, not a failed checkpoint.
5. pending_inflight is 1 at every checkpoint, including baseline. This is the metrics call observing itself; it is not a blocked gate. gates are not exposed by the current service API and are recorded as not_exposed, as the task allows.

## Run 1 acceptance criteria

1. **PASS**. state.started_ms = 1794659105790 = 2026-11-14T12:25:05.790Z, matching checkpoint 0's 12:25:05.822Z baseline and the summary started_utc. The replacement daemon did not reset it.
2. **PASS**. Indices 0, 1, 2 are contiguous; intervals are 3,599.974 s and 3,600.005 s; elapsed_seconds is monotonic; no duplicate or gap.
3. **PASS**. Recomputed load success is 7,092 / 7,092 = 100.00% >= 99.9%; tool_errors_total is 0; consecutive_health_failures is 0.
4. **PASS**. RSS growth math is 3,219,456 bytes = 3.0703 MiB, matching the reported 3.07 and below 50 MiB.
5. **PASS**. The summary matches checkpoint 2 for next_checkpoint_utc, cumulative load, RSS, tool calls, errors, success rate, pending, and health failures.
6. **PASS with notes**. No duplicate index, no gap, no clock jump, no checkpoint without health, and no invalid final flag. The daemon restart and exact resume-time discrepancy are documented above.

## Run 2 clean baseline

Run 2 is the clean restart that supersedes run 1.

- Started: 2026-11-14T14:41:37.572Z; state.started_ms 1794667297572.
- Evidence: C:/Users/Administrator/AppData/Local/Temp/corvus-nfr3-soak-clean-20261114T144131
- Ports: 29430 (WebSocket) and 29431 (HTTP MCP); daemon Windows PID 14464; mock browser mock:soak.
- Fixture: http://127.0.0.1:8811/perf-10k.html returns HTTP 200 and 70,226 bytes; tests/fixtures/perf-10k.html is also 70,226 bytes, revision bb7e702b1d27a35967497ea43760dafb8a534788e0fde0074c16b02ccbc4ef7c.
- Checkpoint 0 at 2026-11-14T14:41:37.605Z: elapsed 0.03 s; RSS 7,749,632 bytes; health_ok true; tool_calls_total 1; tool_errors_total 0; success_rate_pct 100; pending_inflight 1; gates not_exposed; sessions 2; steps 1; browsers_connected 1; daemon_uptime_ms 413.
- State at 2026-11-14T14:44:37.853Z: cumulative load 180 attempted / 180 ok / 0 failed; consecutive_health_failures 0; fatal_error null. Load is advancing with no failures.
- Summary: tests/report/NFR-3-soak-summary.txt now points at evidence_dir C:/Users/Administrator/AppData/Local/Temp/corvus-nfr3-soak-clean-20261114T144131, started_utc 2026-11-14T14:41:37.572Z, next_checkpoint_utc 2026-11-14T15:41:37.572Z, last_checkpoint_index 0, RSS 7,749,632, growth 0.
- Old daemon PID 28144 is absent (tasklist: no matching task). The only listeners on 29430 and 29431 are the new daemon PID 14464; no stale listener from run 1 remains.

### Run 2 baseline checks

| Check | Result |
|---|---|
| started_ms | 1794667297572 = 2026-11-14T14:41:37.572Z |
| checkpoint 0 RSS | 7,749,632 bytes |
| health | true |
| load advancing | 180 / 180 / 0 at 14:44:37.853Z |
| fixture | HTTP 200, 70,226 bytes |
| old listener | none; PID 28144 absent |
| summary evidence_dir | Run 2 clean directory |
| verdict | PASS |

## Overall status

Run 1 is **SUPERSEDED** for NFR-3 72 h evidence because the daemon was replaced at 2026-11-14T13:27:44.986Z; its checkpoint table remains valid continuity evidence but cannot prove continuous daemon uptime. Run 2 clean baseline is **PASS** and is the active run. QA did not restart, stop, or edit Run 2 and did not commit.

## Version note

Run 2 started at 2026-11-14T14:41:37.572Z, before the 1.0.0 version bump at about 14:50Z. Its daemon therefore reports version 0.1.0. The 1.0.0 bump changed only version strings (Cargo.toml, Cargo.lock package versions and extension constants); the runtime code is identical, so the soak still measures the released runtime behaviour. The final NFR-3 report must repeat this note and must not claim the run executed a binary that reports 1.0.0.
