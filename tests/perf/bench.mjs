#!/usr/bin/env node
// Corvus M4 performance evidence benchmark.
// Modes: latency, concurrency, memory, live, size, summary.
// This file is QA test code. It does not import product modules.
import { readFileSync, readdirSync, statSync, writeFileSync, mkdirSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";

const PROTO = "2025-06-18";
const DEFAULT_SAMPLES = 60;
const DEFAULT_WARMUP = 8;
const DEFAULT_RATE_MS = 150;
const DEFAULT_WORKERS = 3;
const DEFAULT_DURATION_S = 900;
const DEFAULT_IDLE_S = 120;
const DEFAULT_SAMPLE_S = 2;
const NFR_CLICK_TYPE_P95_MS = 50;
const NFR_SNAPSHOT_P95_MS = 300;
const NFR_GROWTH_BYTES = 50 * 1024 * 1024;
const NFR_SUCCESS_RATE = 99.9;
const NFR_BINARY_BYTES = 25 * 1000 * 1000;
const NFR_EXTENSION_BYTES = 1 * 1000 * 1000;

function usage() {
  console.log("usage: bench.mjs <latency|concurrency|memory|live|size|summary> [options]");
  console.log("common: --url MCP_URL --browser-id ID --out FILE");
  console.log("latency: --fixture-url URL --samples N");
  console.log("concurrency: --fixture-url URL --tabs N --clients N");
  console.log("memory: --rss-pid PID --load-url URL --duration SECONDS --idle SECONDS --sample SECONDS --workers N --rate-ms N");
  console.log("live: --fixture-url URL --samples N");
  console.log("size: --binary FILE --extension DIR");
  console.log("summary: --dir EVIDENCE_DIR");
}

function fail(message) {
  console.error("FAIL " + message);
  process.exit(1);
}

function parseArgs(argv) {
  const opts = { _: [] };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg.startsWith("--")) {
      const body = arg.slice(2);
      const eq = body.indexOf("=");
      if (eq >= 0) {
        opts[body.slice(0, eq)] = body.slice(eq + 1);
      } else {
        const next = argv[i + 1];
        if (next === undefined || next.startsWith("--")) {
          opts[body] = true;
        } else {
          opts[body] = next;
          i++;
        }
      }
    } else {
      opts._.push(arg);
    }
  }
  return opts;
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
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function round(n) {
  return Number(Number(n).toFixed(2));
}

function stats(values) {
  if (!values.length) fail("no samples collected");
  const sorted = values.slice().sort((a, b) => a - b);
  const at = (p) => sorted[Math.min(sorted.length - 1, Math.max(0, Math.ceil((p / 100) * sorted.length) - 1))];
  let sum = 0;
  for (const v of values) sum += v;
  return {
    count: values.length,
    min_ms: round(sorted[0]),
    p50_ms: round(at(50)),
    p95_ms: round(at(95)),
    max_ms: round(sorted[sorted.length - 1]),
    mean_ms: round(sum / values.length)
  };
}

function writeOut(path, value) {
  if (!path || path === true) return;
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, JSON.stringify(value, null, 2) + "\n");
}

function emit(tag, value) {
  console.log(tag + " " + JSON.stringify(value));
}

function parseRpc(text) {
  const trimmed = (text || "").trim();
  if (!trimmed) return null;
  try {
    return JSON.parse(trimmed);
  } catch (e) {
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
  } catch (e) {
    return null;
  }
}

async function post(url, session, body) {
  const headers = {
    "content-type": "application/json",
    "accept": "application/json, text/event-stream",
    "mcp-protocol-version": PROTO
  };
  if (session) headers["mcp-session-id"] = session;
  const res = await fetch(url, { method: "POST", headers, body: JSON.stringify(body) });
  const text = await res.text();
  return { res, text };
}

async function initialize(url, name) {
  const response = await post(url, null, {
    jsonrpc: "2.0",
    id: 1,
    method: "initialize",
    params: {
      protocolVersion: PROTO,
      capabilities: {},
      clientInfo: { name, version: "1.0" }
    }
  });
  if (response.res.status !== 200) fail("initialize HTTP " + response.res.status + ": " + response.text.slice(0, 300));
  const body = parseRpc(response.text);
  if (!body || !body.result) fail("initialize missing result: " + response.text.slice(0, 300));
  const sid = response.res.headers.get("mcp-session-id");
  if (!sid) fail("initialize missing Mcp-Session-Id header");
  const ready = await post(url, sid, { jsonrpc: "2.0", method: "notifications/initialized" });
  if (![200, 202, 204].includes(ready.res.status)) fail("notifications/initialized HTTP " + ready.res.status);
  return sid;
}

let nextId = 100;

async function callTool(url, session, tool, args, requestId) {
  const id = requestId === undefined ? nextId++ : requestId;
  const started = Date.now();
  const response = await post(url, session, {
    jsonrpc: "2.0",
    id,
    method: "tools/call",
    params: { name: tool, arguments: args }
  });
  const elapsed = Date.now() - started;
  const body = parseRpc(response.text);
  const result = body && body.result ? body.result : null;
  let text = "";
  if (result && Array.isArray(result.content)) {
    for (const part of result.content) {
      if (part && part.type === "text" && typeof part.text === "string") text += part.text;
    }
  }
  let parsed = null;
  if (result && result.isError !== true && text) {
    try {
      parsed = JSON.parse(text);
    } catch (e) {
      parsed = null;
    }
  }
  return {
    id,
    http_status: response.res.status,
    elapsed_ms: elapsed,
    is_error: Boolean(result && result.isError),
    error: body && body.error ? body.error : null,
    result,
    parsed,
    text
  };
}

function expectOk(call, label) {
  if (call.error) fail(label + " JSON-RPC error: " + JSON.stringify(call.error));
  if (call.http_status !== 200) fail(label + " HTTP " + call.http_status + ": " + call.text.slice(0, 300));
  if (call.is_error) fail(label + " tool error: " + call.text.slice(0, 400));
  if (call.parsed === null && call.result === null) fail(label + " returned no result");
  return call;
}

async function createTab(url, session, browserId, targetUrl, active) {
  const call = expectOk(await callTool(url, session, "tab_new", { browser_id: browserId, url: targetUrl, active: active === true }), "tab_new");
  const value = call.parsed || {};
  const tabId = value.tab_id || value.tabId || (value.tab && value.tab.tab_id);
  if (!tabId) fail("tab_new did not return tab_id: " + call.text.slice(0, 300));
  return { tabId, value, call };
}

async function closeTab(url, session, browserId, tabId) {
  if (!tabId) return;
  try {
    await callTool(url, session, "tab_close", { browser_id: browserId, tab_id: tabId });
  } catch (e) {
    // Best effort cleanup only.
  }
}

async function measure(url, session, browserId, tabId, label, args, samples, warmup) {
  for (let i = 0; i < warmup; i++) {
    expectOk(await callTool(url, session, args.tool, Object.assign({ browser_id: browserId, tab_id: tabId }, args.args)), label + " warmup");
  }
  const values = [];
  let last = null;
  for (let i = 0; i < samples; i++) {
    last = expectOk(await callTool(url, session, args.tool, Object.assign({ browser_id: browserId, tab_id: tabId }, args.args)), label);
    values.push(last.elapsed_ms);
  }
  return { label, tool: args.tool, stats: stats(values), last: last && last.parsed ? last.parsed : null };
}

async function latency(opts) {
  const url = opts.url;
  const browserId = opts["browser-id"];
  const fixtureUrl = opts["fixture-url"];
  if (!url || !browserId || !fixtureUrl) fail("latency needs --url --browser-id --fixture-url");
  const samples = integerOption(opts.samples, DEFAULT_SAMPLES, 5, 1000);
  const warmup = integerOption(opts.warmup, DEFAULT_WARMUP, 0, 100);
  const session = await initialize(url, "corvus-perf-latency");
  const tab = await createTab(url, session, browserId, fixtureUrl);
  const results = {};
  try {
    const nav = expectOk(await callTool(url, session, "browser_navigate", { browser_id: browserId, tab_id: tab.tabId, url: fixtureUrl }), "navigate to fixture");
    const navValue = nav.parsed || {};
    if (navValue.loaded === false) fail("fixture navigation reported loaded=false");
    const definitions = [
      ["click", { tool: "browser_click", args: { target: "#perf-click" } }],
      ["type", { tool: "browser_type", args: { target: "#perf-type", text: "perf", clear: true } }],
      ["snapshot", { tool: "page_snapshot", args: {} }],
      ["snapshot_full", { tool: "page_snapshot", args: { mode: "full", max_nodes: 3000 } }]
    ];
    for (const pair of definitions) {
      results[pair[0]] = await measure(url, session, browserId, tab.tabId, pair[0], pair[1], samples, warmup);
    }
    const out = {
      mode: "latency",
      samples_per_tool: samples,
      warmup_per_tool: warmup,
      tab_id: tab.tabId,
      fixture_url: fixtureUrl,
      results
    };
    writeOut(opts.out, out);
    emit("LATENCY", {
      click_p50: results.click.stats.p50_ms,
      click_p95: results.click.stats.p95_ms,
      type_p50: results.type.stats.p50_ms,
      type_p95: results.type.stats.p95_ms,
      snapshot_p50: results.snapshot.stats.p50_ms,
      snapshot_p95: results.snapshot.stats.p95_ms,
      snapshot_full_p50: results.snapshot_full.stats.p50_ms,
      snapshot_full_p95: results.snapshot_full.stats.p95_ms
    });
  } finally {
    await closeTab(url, session, browserId, tab.tabId);
  }
}

async function concurrency(opts) {
  const url = opts.url;
  const browserId = opts["browser-id"];
  const fixtureUrl = opts["fixture-url"];
  if (!url || !browserId || !fixtureUrl) fail("concurrency needs --url --browser-id --fixture-url");
  const tabsCount = integerOption(opts.tabs, 10, 2, 50);
  const clientsCount = integerOption(opts.clients, 10, 2, 50);
  const session = await initialize(url, "corvus-perf-concurrency");
  const tabs = [];
  for (let i = 0; i < tabsCount; i++) tabs.push(await createTab(url, session, browserId, fixtureUrl));
  try {
    const waitStart = Date.now();
    const waits = await Promise.all(tabs.map((tab) => callTool(url, session, "browser_wait", {
      browser_id: browserId,
      tab_id: tab.tabId,
      selector: "#delay",
      timeout_ms: 8000
    })));
    const waitWall = Date.now() - waitStart;
    const waitOk = waits.filter((call) => !call.error && !call.is_error && call.http_status === 200).length;
    const navStart = Date.now();
    const navs = await Promise.all(tabs.map((tab, index) => callTool(url, session, "browser_navigate", {
      browser_id: browserId,
      tab_id: tab.tabId,
      url: fixtureUrl + "?nav=" + index
    })));
    const navWall = Date.now() - navStart;
    const navOk = navs.filter((call) => !call.error && !call.is_error && call.http_status === 200).length;

    const sessionStart = Date.now();
    const sessions = await Promise.all(Array.from({ length: clientsCount }, (_, index) => initialize(url, "corvus-perf-client-" + index)));
    const sessionOpenWall = Date.now() - sessionStart;
    const clientStart = Date.now();
    const clientCalls = await Promise.all(sessions.map((sid) => callTool(url, sid, "browser_list", { browser_id: browserId })));
    const clientWall = Date.now() - clientStart;
    const clientOk = clientCalls.filter((call) => !call.error && !call.is_error && call.http_status === 200).length;

    const pass = waitOk === tabsCount && navOk === tabsCount && sessions.length === clientsCount && clientOk === clientsCount;
    const out = {
      mode: "concurrency",
      tabs: tabsCount,
      waits: { ok: waitOk, total: tabsCount, wall_ms: waitWall },
      navigates: { ok: navOk, total: tabsCount, wall_ms: navWall },
      clients: {
        connected: sessions.length,
        connected_total: clientsCount,
        ok: clientOk,
        open_wall_ms: sessionOpenWall,
        calls_wall_ms: clientWall
      },
      pass
    };
    writeOut(opts.out, out);
    emit("CONCURRENCY", out);
  } finally {
    for (const tab of tabs) await closeTab(url, session, browserId, tab.tabId);
  }
}

function readRss(pid) {
  let output;
  try {
    output = execFileSync("powershell.exe", [
      "-NoProfile",
      "-NonInteractive",
      "-Command",
      "(Get-Process -Id " + pid + " -ErrorAction Stop).WorkingSet64"
    ], { encoding: "utf8", windowsHide: true }).trim();
  } catch (e) {
    fail("cannot read RSS for pid " + pid + ": " + String(e.message || e));
  }
  const value = Number(output);
  if (!Number.isFinite(value) || value <= 0) fail("invalid RSS output for pid " + pid + ": " + output.slice(0, 120));
  return Math.round(value);
}

async function memory(opts) {
  const url = opts.url;
  const browserId = opts["browser-id"];
  const loadUrl = opts["load-url"] || opts["fixture-url"];
  const pid = Number(opts["rss-pid"]);
  if (!url || !browserId || !loadUrl || !Number.isInteger(pid) || pid <= 0) fail("memory needs --url --browser-id --rss-pid --load-url");
  const duration = integerOption(opts.duration, DEFAULT_DURATION_S, 1, 3600);
  const idle = integerOption(opts.idle, DEFAULT_IDLE_S, 0, 600);
  const sampleSeconds = integerOption(opts.sample, DEFAULT_SAMPLE_S, 1, 60);
  const workers = integerOption(opts.workers, DEFAULT_WORKERS, 1, 16);
  const rateMs = integerOption(opts["rate-ms"], DEFAULT_RATE_MS, 0, 5000);
  const session = await initialize(url, "corvus-perf-memory");
  const tab = await createTab(url, session, browserId, loadUrl);
  const samples = [];
  const success = { count: 0 };
  const errors = [];
  let stopped = false;

  function sample() {
    samples.push({ at_ms: Date.now(), rss_bytes: readRss(pid) });
  }

  sample();
  const timer = setInterval(sample, sampleSeconds * 1000);
  const toolMix = [
    { name: "browser_list", args: {}, tabScoped: false },
    { name: "tab_list", args: {}, tabScoped: false },
    { name: "page_text", args: {} },
    { name: "page_find", args: { query: "Perf" } },
    { name: "browser_click", args: { target: "#perf-click" } },
    { name: "browser_type", args: { target: "#perf-type", text: "perf", clear: true } },
    { name: "browser_wait", args: { selector: "#perf-click", timeout_ms: 1000 } },
    { name: "net_log", args: { limit: 5 } },
    { name: "download_list", args: { limit: 5 }, tabScoped: false },
    { name: "logs_tail", args: { limit: 5 }, tabScoped: false }
  ];

  async function worker(index) {
    let iteration = 0;
    while (!stopped) {
      const pick = toolMix[(index + iteration * 3) % toolMix.length];
      iteration++;
      try {
        const requestArgs = Object.assign({ browser_id: browserId }, pick.args);
        if (pick.tabScoped !== false) requestArgs.tab_id = tab.tabId;
        const call = await callTool(url, session, pick.name, requestArgs);
        if (call.error || call.is_error || call.http_status !== 200) {
          errors.push({ tool: pick.name, error: call.error || call.text.slice(0, 200) });
        } else {
          success.count++;
        }
      } catch (e) {
        errors.push({ tool: pick.name, error: String(e.message || e) });
      }
      if (rateMs > 0) await sleep(rateMs);
    }
  }

  try {
    const workerPromises = Array.from({ length: workers }, (_, index) => worker(index));
    await sleep(duration * 1000);
    stopped = true;
    await Promise.race([Promise.allSettled(workerPromises), sleep(10000)]);
    await sleep(idle * 1000);
  } finally {
    stopped = true;
    clearInterval(timer);
    await closeTab(url, session, browserId, tab.tabId);
  }
  sample();
  const failureCount = errors.length;
  const totalCalls = success.count + failureCount;
  const successRate = totalCalls > 0 ? (100 * success.count) / totalCalls : 0;
  const first = samples[0].rss_bytes;
  const last = samples[samples.length - 1].rss_bytes;
  let peak = 0;
  for (const item of samples) peak = Math.max(peak, item.rss_bytes);
  const out = {
    mode: "memory",
    pid,
    duration_s: duration,
    idle_s: idle,
    sample_s: sampleSeconds,
    workers,
    rate_ms: rateMs,
    samples: samples.length,
    call_ok: success.count,
    call_errors: failureCount,
    success_rate_percent: round(successRate),
    rss_start_bytes: first,
    rss_end_bytes: last,
    rss_max_bytes: peak,
    growth_bytes: last - first,
    peak_growth_bytes: peak - first,
    growth_mb: round((last - first) / (1024 * 1024)),
    peak_growth_mb: round((peak - first) / (1024 * 1024)),
    error_examples: errors.slice(0, 10),
    pass: last - first < NFR_GROWTH_BYTES && successRate >= NFR_SUCCESS_RATE
  };
  writeOut(opts.out, out);
  emit("MEMORY", out);
}

async function live(opts) {
  const url = opts.url;
  const browserId = opts["browser-id"];
  const fixtureUrl = opts["fixture-url"];
  if (!url || !browserId || !fixtureUrl) fail("live needs --url --browser-id --fixture-url");
  const samples = integerOption(opts.samples, DEFAULT_SAMPLES, 5, 500);
  const warmup = integerOption(opts.warmup, 5, 0, 50);
  const session = await initialize(url, "corvus-perf-live");
  const before = expectOk(await callTool(url, session, "tab_list", { browser_id: browserId }), "tab_list before");
  const tab = await createTab(url, session, browserId, fixtureUrl, true);
  try {
    const nav = expectOk(await callTool(url, session, "browser_navigate", { browser_id: browserId, tab_id: tab.tabId, url: fixtureUrl }), "live navigate");
    if (nav.parsed && nav.parsed.loaded === false) fail("live fixture navigation reported loaded=false");
    const evalCall = expectOk(await callTool(url, session, "browser_eval", {
      browser_id: browserId,
      tab_id: tab.tabId,
      expression: "document.querySelectorAll('*').length",
      world: "main"
    }), "live dom count");
    const domValue = evalCall.parsed && typeof evalCall.parsed.value === "number" ? evalCall.parsed.value : null;
    if (domValue === null) fail("live browser_eval did not return numeric value: " + evalCall.text.slice(0, 300));
    const results = {};
    results.snapshot = await measure(url, session, browserId, tab.tabId, "live_snapshot", { tool: "page_snapshot", args: {} }, samples, warmup);
    results.snapshot_full = await measure(url, session, browserId, tab.tabId, "live_snapshot_full", { tool: "page_snapshot", args: { mode: "full", max_nodes: 3000 } }, samples, warmup);
    results.click = await measure(url, session, browserId, tab.tabId, "live_click", { tool: "browser_click", args: { target: "#perf-click" } }, samples, warmup);
    results.type = await measure(url, session, browserId, tab.tabId, "live_type", { tool: "browser_type", args: { target: "#perf-type", text: "perf", clear: true } }, samples, warmup);
    const snapshotNodes = results.snapshot.last && typeof results.snapshot.last.node_count === "number" ? results.snapshot.last.node_count : null;
    const fullNodes = results.snapshot_full.last && typeof results.snapshot_full.last.node_count === "number" ? results.snapshot_full.last.node_count : null;
    const pass = domValue === 10000 && results.snapshot_full.stats.p95_ms <= NFR_SNAPSHOT_P95_MS;
    const out = {
      mode: "live",
      browser_id: browserId,
      tab_id: tab.tabId,
      fixture_url: fixtureUrl,
      tabs_before: before.parsed && Array.isArray(before.parsed.tabs) ? before.parsed.tabs.length : null,
      dom_elements: domValue,
      snapshot_node_count: snapshotNodes,
      snapshot_full_node_count: fullNodes,
      samples_per_tool: samples,
      results,
      pass
    };
    writeOut(opts.out, out);
    emit("LIVE", {
      dom_elements: domValue,
      snapshot_nodes: snapshotNodes,
      snapshot_full_nodes: fullNodes,
      snapshot_p95: results.snapshot.stats.p95_ms,
      snapshot_full_p95: results.snapshot_full.stats.p95_ms,
      click_p95: results.click.stats.p95_ms,
      type_p95: results.type.stats.p95_ms,
      pass
    });
  } finally {
    await closeTab(url, session, browserId, tab.tabId);
  }
}

function directoryBytes(path) {
  let bytes = 0;
  let files = 0;
  for (const entry of readdirSync(path, { withFileTypes: true })) {
    const full = join(path, entry.name);
    if (entry.isDirectory()) {
      const nested = directoryBytes(full);
      bytes += nested.bytes;
      files += nested.files;
    } else if (entry.isFile()) {
      bytes += statSync(full).size;
      files++;
    }
  }
  return { bytes, files };
}

function size(opts) {
  const binary = opts.binary;
  const extension = opts.extension;
  if (!binary || !extension) fail("size needs --binary --extension");
  const binaryBytes = statSync(binary).size;
  const extensionInfo = directoryBytes(extension);
  const out = {
    mode: "size",
    binary: binary,
    binary_bytes: binaryBytes,
    binary_mb: round(binaryBytes / (1000 * 1000)),
    extension: extension,
    extension_bytes: extensionInfo.bytes,
    extension_files: extensionInfo.files,
    extension_mb: round(extensionInfo.bytes / (1000 * 1000)),
    pass: binaryBytes < NFR_BINARY_BYTES && extensionInfo.bytes < NFR_EXTENSION_BYTES
  };
  writeOut(opts.out, out);
  emit("SIZE", out);
}

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function fmt(value) {
  return Number(value).toFixed(2);
}

function summary(opts) {
  const dir = opts.dir;
  if (!dir) fail("summary needs --dir");
  const latency = readJson(join(dir, "latency.json"));
  const concurrency = readJson(join(dir, "concurrency.json"));
  const memory = readJson(join(dir, "memory.json"));
  const liveData = readJson(join(dir, "live.json"));
  const sizeData = readJson(join(dir, "size.json"));
  const rows = [];
  rows.push(["NFR-1", "browser_click p95", fmt(latency.results.click.stats.p95_ms) + " ms", "< " + NFR_CLICK_TYPE_P95_MS + " ms", latency.results.click.stats.p95_ms < NFR_CLICK_TYPE_P95_MS]);
  rows.push(["NFR-1", "browser_type p95", fmt(latency.results.type.stats.p95_ms) + " ms", "< " + NFR_CLICK_TYPE_P95_MS + " ms", latency.results.type.stats.p95_ms < NFR_CLICK_TYPE_P95_MS]);
  rows.push(["NFR-1", "page_snapshot p95", fmt(latency.results.snapshot.stats.p95_ms) + " ms", "< " + NFR_SNAPSHOT_P95_MS + " ms", latency.results.snapshot.stats.p95_ms < NFR_SNAPSHOT_P95_MS]);
  rows.push(["NFR-1", "page_snapshot full p95", fmt(latency.results.snapshot_full.stats.p95_ms) + " ms", "< " + NFR_SNAPSHOT_P95_MS + " ms", latency.results.snapshot_full.stats.p95_ms < NFR_SNAPSHOT_P95_MS]);
  rows.push(["NFR-2", "10 tab waits", concurrency.waits.ok + "/" + concurrency.waits.total + " in " + concurrency.waits.wall_ms + " ms", "all success", concurrency.waits.ok === concurrency.waits.total]);
  rows.push(["NFR-2", "10 tab navigates", concurrency.navigates.ok + "/" + concurrency.navigates.total + " in " + concurrency.navigates.wall_ms + " ms", "all success", concurrency.navigates.ok === concurrency.navigates.total]);
  rows.push(["NFR-2", "10 MCP clients", concurrency.clients.ok + "/" + concurrency.clients.connected_total + " calls", "all connected", concurrency.clients.ok === concurrency.clients.connected_total]);
  rows.push(["NFR-3", "RSS growth", fmt(memory.growth_mb) + " MB", "< 50 MB", memory.growth_bytes < NFR_GROWTH_BYTES]);
  rows.push(["NFR-3", "mixed-load success", fmt(memory.success_rate_percent) + "%", ">= 99.9%", memory.success_rate_percent >= NFR_SUCCESS_RATE]);
  rows.push(["NFR-4", "release binary", fmt(sizeData.binary_mb) + " MB", "< 25 MB", sizeData.binary_bytes < NFR_BINARY_BYTES]);
  rows.push(["NFR-4", "extension bundle", fmt(sizeData.extension_mb) + " MB", "< 1 MB", sizeData.extension_bytes < NFR_EXTENSION_BYTES]);
  rows.push(["LIVE", "headless snapshot full p95", fmt(liveData.results.snapshot_full.stats.p95_ms) + " ms", "< " + NFR_SNAPSHOT_P95_MS + " ms", liveData.results.snapshot_full.stats.p95_ms < NFR_SNAPSHOT_P95_MS]);
  rows.push(["LIVE", "10k DOM verified", String(liveData.dom_elements), "10000", liveData.dom_elements === 10000]);
  console.log("NFR | measurement | observed | target | verdict");
  console.log("----|-------------|----------|--------|--------");
  let allPass = true;
  for (const row of rows) {
    if (!row[4]) allPass = false;
    console.log(row[0] + " | " + row[1] + " | " + row[2] + " | " + row[3] + " | " + (row[4] ? "PASS" : "MISS"));
  }
  console.log("RESULT: " + (allPass ? "PASS" : "FAIL") + " " + rows.length + " checks");
  if (!allPass) process.exit(1);
}

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  const command = opts._[0];
  if (!command || command === "help" || command === "--help") {
    usage();
    return;
  }
  if (command === "latency") return latency(opts);
  if (command === "concurrency") return concurrency(opts);
  if (command === "memory") return memory(opts);
  if (command === "live") return live(opts);
  if (command === "size") return size(opts);
  if (command === "summary") return summary(opts);
  fail("unknown command: " + command);
}

main().catch((error) => {
  console.error("FAIL " + (error && error.stack ? error.stack : String(error)));
  process.exit(1);
});
