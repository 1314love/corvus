#!/usr/bin/env node
// Corvus NFR-3 soak driver: continuous mixed load, hourly checkpoints, resumable state.
// QA test code only. It does not import product modules.
import { appendFileSync, existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { dirname, resolve } from "node:path";

const PROTO = "2025-06-18";
const NFR_GROWTH_BYTES = 50 * 1024 * 1024;
const NFR_SUCCESS_RATE = 99.9;
const DEFAULT_DURATION_SECONDS = 72 * 3600;
const DEFAULT_CHECKPOINT_SECONDS = 3600;

let opts;

function usage() {
  console.log("usage: soak.mjs [options]");
  console.log("  --mcp-url URL             MCP endpoint (default http://127.0.0.1:29431/mcp)");
  console.log("  --health-url URL          health endpoint; derived from --mcp-url when omitted");
  console.log("  --browser-id ID           browser_id sent on every tool call");
  console.log("  --fixture-url URL         URL used for tab_new and browser_navigate");
  console.log("  --rss-pid PID             Windows daemon PID for WorkingSet64 sampling");
  console.log("  --evidence-dir DIR        resumable evidence directory");
  console.log("  --summary-file FILE       small text summary under tests/report");
  console.log("  --duration-seconds N      total run length (default 259200 = 72h)");
  console.log("  --checkpoint-seconds N    checkpoint interval (default 3600)");
  console.log("  --workers N               concurrent load workers (default 2)");
  console.log("  --rate-ms N               per-worker delay between calls (default 2000)");
  console.log("  --live                    use the safe live-browser tool mix");
}

function fail(message) {
  console.error("FAIL " + message);
  process.exit(1);
}

function parseArgs(argv) {
  const out = { _: [] };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg.startsWith("--")) {
      const body = arg.slice(2);
      const eq = body.indexOf("=");
      if (eq >= 0) {
        out[body.slice(0, eq)] = body.slice(eq + 1);
      } else {
        const next = argv[i + 1];
        if (next === undefined || next.startsWith("--")) {
          out[body] = true;
        } else {
          out[body] = next;
          i++;
        }
      }
    } else {
      out._.push(arg);
    }
  }
  return out;
}

function numberOption(value, fallback) {
  if (value === undefined || value === true || value === "") return fallback;
  const n = Number(value);
  if (!Number.isFinite(n)) fail("invalid number option: " + value);
  return n;
}

function integerOption(value, fallback, min, max) {
  const n = Math.floor(numberOption(value, fallback));
  if (n < min || n > max) fail("option out of range: " + value + " (expected " + min + ".." + max + ")");
  return n;
}

function sleep(ms) {
  return new Promise((resolvePromise) => setTimeout(resolvePromise, ms));
}

function round(n) {
  return Number(Number(n).toFixed(2));
}

function parseRpc(text) {
  const trimmed = (text || "").trim();
  if (!trimmed) return null;
  try {
    return JSON.parse(trimmed);
  } catch {
    // Fall through to SSE data lines.
  }
  const lines = trimmed.split(/\r?\n/);
  const data = [];
  for (const line of lines) {
    if (line.startsWith("data:")) {
      const chunk = line.slice(5).trim();
      if (chunk) data.push(chunk);
    }
  }
  if (!data.length) return null;
  try {
    return JSON.parse(data[data.length - 1]);
  } catch {
    return null;
  }
}

async function post(url, session, body) {
  const headers = {
    "content-type": "application/json",
    accept: "application/json, text/event-stream",
    "mcp-protocol-version": PROTO
  };
  if (session) headers["mcp-session-id"] = session;
  const res = await fetch(url, {
    method: "POST",
    headers,
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(60000)
  });
  const text = await res.text();
  return { status: res.status, headers: res.headers, text, body: parseRpc(text) };
}

async function openSession(name) {
  const response = await post(opts.mcpUrl, null, {
    jsonrpc: "2.0",
    id: 1,
    method: "initialize",
    params: {
      protocolVersion: PROTO,
      capabilities: {},
      clientInfo: { name, version: "1.0" }
    }
  });
  if (response.status !== 200) throw new Error("initialize HTTP " + response.status + ": " + response.text.slice(0, 300));
  if (!response.body || !response.body.result) throw new Error("initialize missing result: " + response.text.slice(0, 300));
  const sid = response.headers.get("mcp-session-id");
  if (!sid) throw new Error("initialize missing Mcp-Session-Id header");
  const ready = await post(opts.mcpUrl, sid, { jsonrpc: "2.0", method: "notifications/initialized" });
  if (![200, 202, 204].includes(ready.status)) throw new Error("notifications/initialized HTTP " + ready.status);
  return sid;
}

let nextRequestId = 1000;

async function callTool(session, tool, args) {
  const id = nextRequestId++;
  const started = Date.now();
  const response = await post(opts.mcpUrl, session, {
    jsonrpc: "2.0",
    id,
    method: "tools/call",
    params: { name: tool, arguments: args }
  });
  const body = response.body;
  const result = body && body.result ? body.result : null;
  let text = "";
  if (result && Array.isArray(result.content)) {
    for (const part of result.content) {
      if (part && part.type === "text" && typeof part.text === "string") text += part.text;
    }
  }
  let parsed = result && result.structuredContent ? result.structuredContent : null;
  if (parsed === null && text) {
    try {
      parsed = JSON.parse(text);
    } catch {
      parsed = null;
    }
  }
  return {
    id,
    http_status: response.status,
    elapsed_ms: Date.now() - started,
    is_error: Boolean(result && result.isError),
    error: body && body.error ? body.error : null,
    result,
    parsed,
    text
  };
}

function toolFailure(call, label) {
  if (call.error) return label + " JSON-RPC error: " + JSON.stringify(call.error);
  if (call.http_status !== 200) return label + " HTTP " + call.http_status + ": " + call.text.slice(0, 300);
  if (call.is_error) return label + " tool error: " + (call.text || JSON.stringify(call.parsed)).slice(0, 400);
  if (call.parsed === null && call.result === null) return label + " returned no result";
  return null;
}

function expectOk(call, label) {
  const problem = toolFailure(call, label);
  if (problem) throw new Error(problem);
  return call;
}

function healthUrlFrom(mcpUrl) {
  const url = new URL(mcpUrl);
  url.pathname = url.pathname.replace(/\/mcp\/?$/, "/health");
  if (url.pathname === "" || url.pathname === "/") url.pathname = "/health";
  return url.toString();
}

function readRss(pid) {
  if (!pid) return null;
  try {
    const out = execFileSync(
      "powershell.exe",
      ["-NoProfile", "-NonInteractive", "-Command", "(Get-Process -Id " + pid + " -ErrorAction Stop).WorkingSet64"],
      { encoding: "utf8", windowsHide: true, timeout: 15000 }
    ).trim();
    const n = Number(out);
    return Number.isFinite(n) && n > 0 ? n : null;
  } catch {
    return null;
  }
}

async function readHealth() {
  try {
    const res = await fetch(opts.healthUrl, { signal: AbortSignal.timeout(10000) });
    if (!res.ok) return { ok: false, error: "HTTP " + res.status };
    return { ok: true, data: await res.json() };
  } catch (e) {
    return { ok: false, error: String((e && e.message) || e) };
  }
}

let state = null;
let checkpointSession = null;
let loadSession = null;
let ownTabId = null;
let previousTabId = null;
let stopping = false;
let persistTimer = null;
let workerPromises = [];
let setupError = null;

function statePath() {
  return resolve(opts.evidenceDir, "state.json");
}

function hourlyPath() {
  return resolve(opts.evidenceDir, "hourly.jsonl");
}

function writeState() {
  if (!state) return;
  state.updated_at = new Date().toISOString();
  const target = statePath();
  const tmp = target + ".tmp";
  mkdirSync(dirname(target), { recursive: true });
  writeFileSync(tmp, JSON.stringify(state, null, 2) + "\n");
  renameSync(tmp, target);
}

function writeSummary() {
  if (!state) return;
  const last = state.last_checkpoint || {};
  const rss = last.rss_bytes === undefined ? null : last.rss_bytes;
  const rssStart = state.first_rss_bytes;
  const growthBytes = rss !== null && rssStart !== null ? rss - rssStart : null;
  const growthMb = growthBytes === null ? null : round(growthBytes / (1024 * 1024));
  const total = state.duration_seconds;
  const elapsedSeconds = Math.max(0, (Date.now() - state.started_ms) / 1000);
  const nextIndex = (state.last_checkpoint_index || 0) + 1;
  const nextAt = nextIndex * state.checkpoint_seconds <= total ? new Date(state.started_ms + nextIndex * state.checkpoint_seconds * 1000).toISOString() : "none";
  const rate = last.success_rate_pct === undefined ? null : last.success_rate_pct;
  let verdict;
  if (state.status === "passed") verdict = "PASS";
  else if (state.status === "failed") verdict = "FAIL";
  else if ((state.last_checkpoint_index || 0) < 1) verdict = "awaiting first hourly checkpoint";
  else if (last.health_ok && rate !== null && rate >= NFR_SUCCESS_RATE && (growthBytes === null || growthBytes < NFR_GROWTH_BYTES)) verdict = "on track";
  else verdict = "attention";
  const lines = [
    (state.mode === "live" ? "NFR-3 live-path soak summary" : "NFR-3 72h soak summary"),
    "status: " + state.status,
    "verdict: " + verdict,
    "mode: " + state.mode,
    "started_utc: " + state.started_at,
    "elapsed_hours: " + round(elapsedSeconds / 3600),
    "duration_hours: " + round(total / 3600),
    "next_checkpoint_utc: " + nextAt,
    "last_checkpoint_index: " + (state.last_checkpoint_index || 0),
    "last_checkpoint_utc: " + (last.at || "none"),
    "evidence_dir: " + resolve(opts.evidenceDir),
    "mcp_url: " + opts.mcpUrl,
    "browser_id: " + opts.browserId,
    "rss_start_bytes: " + (rssStart === null ? "null" : rssStart),
    "rss_latest_bytes: " + (rss === null ? "null" : rss),
    "rss_growth_mb: " + (growthMb === null ? "null" : growthMb),
    "tool_calls_total: " + (last.tool_calls_total === undefined || last.tool_calls_total === null ? "null" : last.tool_calls_total),
    "tool_errors_total: " + (last.tool_errors_total === undefined || last.tool_errors_total === null ? "null" : last.tool_errors_total),
    "success_rate_pct: " + (rate === null ? "null" : rate),
    "pending_inflight: " + (last.pending_inflight === undefined || last.pending_inflight === null ? "null" : last.pending_inflight),
    "gates: not_exposed",
    "load_attempted: " + state.cumulative.attempted,
    "load_ok: " + state.cumulative.ok,
    "load_failed: " + state.cumulative.failed,
    "consecutive_health_failures: " + (state.consecutive_health_failures || 0),
    "fatal_error: " + (state.fatal_error || "none")
  ];
  const target = opts.summaryFile;
  const tmp = target + ".tmp";
  mkdirSync(dirname(target), { recursive: true });
  writeFileSync(tmp, lines.join("\n") + "\n");
  renameSync(tmp, target);
}

async function checkpoint(index, meta) {
  const now = Date.now();
  const rss = readRss(opts.rssPid);
  const health = await readHealth();
  let metrics = null;
  let metricsError = null;
  try {
    const call = await callTool(checkpointSession, "metrics", {});
    const problem = toolFailure(call, "metrics");
    if (problem) metricsError = problem;
    else metrics = call.parsed;
  } catch (e) {
    metricsError = String((e && e.message) || e);
  }
  const data = health.ok ? health.data : null;
  const calls = data && Number.isFinite(data.tool_calls_total) ? data.tool_calls_total : metrics && Number.isFinite(metrics.tool_calls_total) ? metrics.tool_calls_total : null;
  const errors = data && Number.isFinite(data.tool_errors_total) ? data.tool_errors_total : metrics && Number.isFinite(metrics.tool_errors_total) ? metrics.tool_errors_total : null;
  const completed = state.cumulative.ok + state.cumulative.failed;
  const rate = opts.live
    ? (completed > 0 ? round((state.cumulative.ok / completed) * 100) : null)
    : (calls !== null && errors !== null && calls > 0 ? round(((calls - errors) / calls) * 100) : null);
  const prev = state.last_checkpoint_cumulative || { attempted: 0, ok: 0, failed: 0 };
  const cumulative = state.cumulative;
  const interval = {
    attempted: cumulative.attempted - prev.attempted,
    ok: cumulative.ok - prev.ok,
    failed: cumulative.failed - prev.failed
  };
  if (rss !== null && state.first_rss_bytes === null) state.first_rss_bytes = rss;
  const growth = rss !== null && state.first_rss_bytes !== null ? rss - state.first_rss_bytes : null;
  const record = {
    index,
    at: new Date(now).toISOString(),
    at_ms: now,
    elapsed_seconds: round((now - state.started_ms) / 1000),
    baseline: Boolean(meta && meta.baseline),
    catchup: Boolean(meta && meta.catchup),
    final: Boolean(meta && meta.final),
    rss_bytes: rss,
    rss_growth_bytes: growth,
    health_ok: health.ok,
    health_error: health.ok ? null : health.error,
    tool_calls_total: calls,
    tool_errors_total: errors,
    success_rate_pct: rate,
    pending_inflight: metrics && Number.isFinite(metrics.inflight) ? metrics.inflight : null,
    gates: "not_exposed",
    recording: data ? data.recording || null : null,
    browsers_connected: data && Array.isArray(data.browsers) ? data.browsers.length : null,
    daemon_uptime_ms: data && Number.isFinite(data.uptime_ms) ? data.uptime_ms : null,
    metrics_error: metricsError,
    load: {
      interval,
      cumulative: { attempted: cumulative.attempted, ok: cumulative.ok, failed: cumulative.failed }
    }
  };
  if (!state.last_checkpoint_cumulative) state.last_checkpoint_cumulative = { attempted: 0, ok: 0, failed: 0 };
  state.last_checkpoint_cumulative = { attempted: cumulative.attempted, ok: cumulative.ok, failed: cumulative.failed };
  state.last_checkpoint = record;
  state.last_checkpoint_index = Math.max(state.last_checkpoint_index || 0, index);
  state.last_checkpoint_ms = now;
  state.last_health = data ? { status: data.status, protocol: data.protocol, version: data.version, tool_calls_total: data.tool_calls_total, tool_errors_total: data.tool_errors_total, sessions: data.recording ? data.recording.sessions : null, steps: data.recording ? data.recording.steps : null } : null;
  state.consecutive_health_failures = health.ok ? 0 : (state.consecutive_health_failures || 0) + 1;
  appendFileSync(hourlyPath(), JSON.stringify(record) + "\n");
  writeState();
  writeSummary();
  console.log("[checkpoint " + index + "] rss=" + (rss === null ? "null" : rss) + " calls=" + (calls === null ? "null" : calls) + " errors=" + (errors === null ? "null" : errors) + " rate=" + (rate === null ? "null" : rate) + " health=" + health.ok + (meta && meta.baseline ? " baseline" : "") + (metricsError ? " metricsError=" + metricsError : ""));
  if (state.consecutive_health_failures >= 3) {
    state.fatal_error = "daemon health failed for " + state.consecutive_health_failures + " consecutive checkpoints";
    stopping = true;
  }
}

async function createOwnTab() {
  const call = expectOk(await callTool(loadSession, "tab_new", { browser_id: opts.browserId, url: opts.fixtureUrl, active: true }), "tab_new");
  const value = call.parsed || {};
  const tabId = value.tab_id || value.tabId || (value.tab && value.tab.tab_id);
  if (!tabId) throw new Error("tab_new did not return tab_id: " + call.text.slice(0, 300));
  ownTabId = tabId;
  previousTabId = value.previous_tab_id || null;
  return tabId;
}

async function cleanupOwnTab() {
  if (!ownTabId || !loadSession) return;
  if (opts.live && previousTabId) {
    try {
      await callTool(loadSession, "tab_switch", { browser_id: opts.browserId, tab_id: previousTabId });
    } catch {
      // Best effort only.
    }
  }
  try {
    await callTool(loadSession, "tab_close", { browser_id: opts.browserId, tab_id: ownTabId });
  } catch {
    // Best effort only.
  }
}

const MOCK_MIX = [
  { name: "page_text", args: {} },
  { name: "page_snapshot", args: {} },
  { name: "page_html", args: {} },
  { name: "page_find", args: { query: "Perf" } },
  { name: "browser_click", args: { target: "#perf-click" } },
  { name: "browser_type", args: { target: "#perf-type", text: "soak", clear: true } },
  { name: "browser_wait", args: { selector: "#perf-click", timeout_ms: 1000 } },
  { name: "browser_navigate", args: { url: null } },
  { name: "net_log", args: { limit: 5 } },
  { name: "download_list", args: { limit: 5 } },
  { name: "logs_tail", args: { limit: 5 } },
  { name: "browser_eval", args: { expression: "2+2" } },
  { name: "browser_list", args: {} },
  { name: "tab_list", args: {} }
];

const LIVE_MIX = [
  { name: "browser_navigate", args: { url: null } },
  { name: "page_text", args: {} },
  { name: "page_snapshot", args: {} },
  { name: "browser_eval", args: { expression: "document.title" } },
  { name: "browser_wait", args: { selector: "#blank", timeout_ms: 1000 } },
  { name: "browser_list", args: {} },
  { name: "tab_list", args: {} }
];

function toolArgs(item) {
  const args = Object.assign({}, item.args);
  if (args.url === null) args.url = opts.fixtureUrl;
  args.browser_id = opts.browserId;
  if (ownTabId && (item.name !== "browser_list" && item.name !== "tab_list")) args.tab_id = ownTabId;
  return args;
}

async function workerLoop(workerIndex) {
  const mix = opts.live ? LIVE_MIX : MOCK_MIX;
  let cursor = workerIndex;
  while (!stopping) {
    const item = mix[cursor % mix.length];
    cursor++;
    const args = toolArgs(item);
    state.cumulative.attempted++;
    try {
      const call = await callTool(loadSession, item.name, args);
      const problem = toolFailure(call, item.name);
      if (problem) {
        state.cumulative.failed++;
        state.last_load_error = { at: new Date().toISOString(), tool: item.name, error: problem.slice(0, 600) };
      } else {
        state.cumulative.ok++;
      }
    } catch (e) {
      state.cumulative.failed++;
      state.last_load_error = { at: new Date().toISOString(), tool: item.name, error: String((e && e.message) || e).slice(0, 600) };
    }
    await sleep(opts.rateMs);
  }
}

async function runCheckpoints() {
  const total = Math.floor(state.duration_seconds / state.checkpoint_seconds);
  for (let i = (state.last_checkpoint_index || 0) + 1; i < total; i++) {
    if (stopping) break;
    const due = state.started_ms + i * state.checkpoint_seconds * 1000;
    const currentIndex = Math.floor((Date.now() - state.started_ms) / (state.checkpoint_seconds * 1000));
    if (i < currentIndex) continue;
    const wait = due - Date.now();
    if (wait > 0) await sleep(wait);
    if (stopping) break;
    await checkpoint(i, { catchup: Date.now() > due + 5000 });
    if (stopping) break;
  }
}

async function main() {
  const now = Date.now();
  mkdirSync(opts.evidenceDir, { recursive: true });
  const existing = existsSync(statePath()) ? JSON.parse(readFileSync(statePath(), "utf8")) : null;
  if (existing && existing.status === "passed") {
    console.log("RESULT: PASS (soak already complete)");
    process.exit(0);
  }
  if (existing && existing.status === "failed" && existing.ended_at) {
    console.log("RESULT: FAIL (soak already complete and marked failed)");
    process.exit(1);
  }
  state = existing || {
    version: 1,
    mode: opts.live ? "live" : "mock",
    started_ms: now,
    started_at: new Date(now).toISOString(),
    duration_seconds: opts.durationSeconds,
    checkpoint_seconds: opts.checkpointSeconds,
    browser_id: opts.browserId,
    mcp_url: opts.mcpUrl,
    fixture_url: opts.fixtureUrl,
    summary_file: opts.summaryFile,
    status: "running",
    cumulative: { attempted: 0, ok: 0, failed: 0 },
    last_checkpoint_cumulative: { attempted: 0, ok: 0, failed: 0 },
    last_checkpoint_index: 0,
    last_checkpoint_ms: null,
    last_checkpoint: null,
    first_rss_bytes: null,
    consecutive_health_failures: 0,
    last_health: null,
    last_load_error: null,
    fatal_error: null
  };
  state.duration_seconds = opts.durationSeconds;
  state.checkpoint_seconds = opts.checkpointSeconds;
  state.summary_file = opts.summaryFile;
  if (!state.cumulative) state.cumulative = { attempted: 0, ok: 0, failed: 0 };
  if (!state.last_checkpoint_cumulative) state.last_checkpoint_cumulative = { attempted: 0, ok: 0, failed: 0 };
  if (!state.ended_at) {
    state.status = "running";
    state.fatal_error = null;
  }
  writeState();
  console.log("SOAK start mode=" + state.mode + " started=" + state.started_at + " evidence=" + resolve(opts.evidenceDir));

  checkpointSession = await openSession("corvus-nfr3-checkpoint");
  loadSession = await openSession("corvus-nfr3-load");
  await createOwnTab();
  console.log("SOAK tab=" + ownTabId + " previous=" + (previousTabId || "none") + " browser=" + opts.browserId);
  if (!state.last_checkpoint) {
    await checkpoint(0, { baseline: true });
  }

  const total = Math.floor(state.duration_seconds / state.checkpoint_seconds);
  if (Date.now() - state.started_ms < state.duration_seconds * 1000) {
    persistTimer = setInterval(() => {
      try {
        writeState();
      } catch (e) {
        console.error("state write failed: " + ((e && e.message) || e));
      }
    }, 30000);
    workerPromises = [];
    for (let i = 0; i < opts.workers; i++) workerPromises.push(workerLoop(i));
    await runCheckpoints();
    if (state.fatal_error) console.error("SOAK fatal: " + state.fatal_error);
  } else {
    console.log("SOAK deadline already reached; finalizing");
  }

  stopping = true;
  if (persistTimer) clearInterval(persistTimer);
  await Promise.allSettled(workerPromises);
  await cleanupOwnTab();
  await checkpoint(total, { final: true });
  const last = state.last_checkpoint || {};
  const growth = last.rss_bytes !== null && state.first_rss_bytes !== null ? last.rss_bytes - state.first_rss_bytes : null;
  const memoryOk = opts.live ? (growth === null || growth < NFR_GROWTH_BYTES) : (growth !== null && growth < NFR_GROWTH_BYTES);
  const pass = !state.fatal_error && last.health_ok && last.success_rate_pct !== null && last.success_rate_pct >= NFR_SUCCESS_RATE && memoryOk;
  state.status = pass ? "passed" : "failed";
  state.ended_ms = Date.now();
  state.ended_at = new Date(state.ended_ms).toISOString();
  writeState();
  writeSummary();
  console.log("RESULT: " + (pass ? "PASS" : "FAIL") + " checkpoints=" + state.last_checkpoint_index + " calls=" + (last.tool_calls_total === null || last.tool_calls_total === undefined ? "null" : last.tool_calls_total) + " errors=" + (last.tool_errors_total === null || last.tool_errors_total === undefined ? "null" : last.tool_errors_total) + " rate=" + (last.success_rate_pct === null ? "null" : last.success_rate_pct) + " rss_growth_mb=" + (growth === null ? "null" : round(growth / (1024 * 1024))));
  process.exit(pass ? 0 : 1);
}

const raw = parseArgs(process.argv.slice(2));
const live = Boolean(raw.live);
const defaultEvidence = live
  ? resolve(process.env.TEMP || process.env.TMP || "/tmp", "corvus-nfr3-live")
  : resolve(process.env.TEMP || process.env.TMP || "/tmp", "corvus-nfr3-soak");
opts = {
  mcpUrl: raw["mcp-url"] || (live ? "http://127.0.0.1:7331/mcp" : "http://127.0.0.1:29431/mcp"),
  healthUrl: raw["health-url"] || null,
  browserId: raw["browser-id"] || (live ? "chromium:7abc8aafb8098216" : "mock:soak"),
  fixtureUrl: raw["fixture-url"] || (live ? "http://127.0.0.1:8811/blank" : "http://127.0.0.1:8811/perf-10k.html"),
  rssPid: raw["rss-pid"] || null,
  evidenceDir: raw["evidence-dir"] ? resolve(raw["evidence-dir"]) : defaultEvidence,
  summaryFile: resolve(raw["summary-file"] || (live ? "tests/report/NFR-3-live-summary.txt" : "tests/report/NFR-3-soak-summary.txt")),
  durationSeconds: integerOption(raw["duration-seconds"], live ? 600 : DEFAULT_DURATION_SECONDS, 60, 30 * 24 * 3600),
  checkpointSeconds: integerOption(raw["checkpoint-seconds"], live ? 60 : DEFAULT_CHECKPOINT_SECONDS, 30, 24 * 3600),
  workers: integerOption(raw.workers, 2, 1, 10),
  rateMs: integerOption(raw["rate-ms"], live ? 1000 : 2000, 10, 60000),
  live
};
if (!opts.healthUrl) opts.healthUrl = healthUrlFrom(opts.mcpUrl);
if (opts.checkpointSeconds > opts.durationSeconds) fail("--checkpoint-seconds must not exceed --duration-seconds");
if (!raw.help && raw._.length) fail("unexpected positional arguments: " + raw._.join(" "));
if (raw.help) {
  usage();
  process.exit(0);
}

main().catch((e) => {
  console.error("FAIL " + ((e && e.stack) || e));
  if (state) {
    state.status = "failed";
    state.fatal_error = String((e && e.message) || e);
    try {
      writeState();
      writeSummary();
    } catch {
      // Best effort.
    }
  }
  process.exit(1);
});
