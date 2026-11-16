#!/usr/bin/env node
// M4 conformance client: concurrent browser_wait, MCP cancellation and schema
// snapshot comparison over streamable HTTP. Reads responses as JSON or SSE.
//
// Usage:
//   node m4_client.mjs parallel <url> <session> <browser-id> <tab-a> <tab-b> <delay-ms>
//   node m4_client.mjs cancel <url> <session> <browser-id> <tab> <timing-jsonl> [wait-ms]
//   node m4_client.mjs cancel-noop <url> <session>
//   node m4_client.mjs timing <timing-jsonl> <overlap|serial>
//   node m4_client.mjs schema <url>
//   node m4_client.mjs schema-check <tools-json> <snapshot-json>
import { readFileSync } from "node:fs";

const PROTOCOL = process.env.EXPECTED_PROTOCOL_VERSION ?? "2025-06-18";

function fail(message) {
  console.error("FAIL " + message);
  process.exit(1);
}

function parseRpc(text) {
  const trimmed = text.trim();
  if (!trimmed) return null;
  if (trimmed.startsWith("{")) return JSON.parse(trimmed);
  const data = [];
  for (const line of trimmed.split(/\r?\n/)) {
    if (line.startsWith("data:")) data.push(line.slice(5).trim());
  }
  let parsed = null;
  for (const item of data) {
    if (!item) continue;
    const value = JSON.parse(item);
    if (value && ("result" in value || "error" in value)) parsed = value;
  }
  return parsed;
}

async function post(url, session, payload) {
  const headers = {
    "Content-Type": "application/json",
    Accept: "application/json, text/event-stream",
    "MCP-Protocol-Version": PROTOCOL,
  };
  if (session) headers["Mcp-Session-Id"] = session;
  const response = await fetch(url, {
    method: "POST",
    headers,
    body: JSON.stringify(payload),
  });
  return response;
}

async function initialize(url) {
  const response = await post(url, "", {
    jsonrpc: "2.0",
    id: 1,
    method: "initialize",
    params: {
      protocolVersion: PROTOCOL,
      capabilities: {},
      clientInfo: { name: "corvus-m4-conformance", version: "1.0" },
    },
  });
  const session = response.headers.get("mcp-session-id") ?? "";
  const body = parseRpc(await response.text());
  if (response.status !== 200 || !body || !body.result) {
    fail("initialize returned HTTP " + response.status);
  }
  const notify = await post(url, session, {
    jsonrpc: "2.0",
    method: "notifications/initialized",
  });
  if (![200, 202, 204].includes(notify.status)) {
    fail("notifications/initialized returned HTTP " + notify.status);
  }
  return session;
}

async function callTool(url, session, id, name, args) {
  const started = Date.now();
  const response = await post(url, session, {
    jsonrpc: "2.0",
    id,
    method: "tools/call",
    params: { name, arguments: args },
  });
  const body = parseRpc(await response.text());
  const elapsed = Date.now() - started;
  if (response.status !== 200) {
    return { id, status: response.status, elapsed_ms: elapsed, http_error: true };
  }
  if (!body) return { id, status: response.status, elapsed_ms: elapsed, empty: true };
  if (body.error) {
    return { id, status: response.status, elapsed_ms: elapsed, error: body.error };
  }
  const result = body.result ?? {};
  const text = Array.isArray(result.content)
    ? result.content.map((item) => item.text ?? "").join(" ")
    : "";
  let parsed = null;
  if (result.isError !== true && typeof text === "string" && text.length > 0) {
    try {
      parsed = JSON.parse(text);
    } catch {
      parsed = null;
    }
  }
  return {
    id,
    status: response.status,
    elapsed_ms: elapsed,
    is_error: result.isError === true,
    text,
    parsed,
    result,
  };
}

async function parallel([url, session, browserId, tabA, tabB, delayRaw]) {
  const delay = Number(delayRaw);
  if (!url || !session || !browserId || !tabA || !tabB || !Number.isFinite(delay)) {
    fail("parallel needs url session browser-id tab-a tab-b delay-ms");
  }
  const started = Date.now();
  const first = callTool(url, session, 401, "browser_wait", {
    browser_id: browserId,
    tab_id: tabA,
    selector: "#delay",
    timeout_ms: delay + 2500,
  });
  const second = callTool(url, session, 402, "browser_wait", {
    browser_id: browserId,
    tab_id: tabB,
    selector: "#delay",
    timeout_ms: delay + 2500,
  });
  const [a, b] = await Promise.all([first, second]);
  const wall = Date.now() - started;
  const okA = !a.error && a.is_error !== true;
  const okB = !b.error && b.is_error !== true;
  const out = {
    mode: "parallel",
    wall_ms: wall,
    delay_ms: delay,
    tab_a: tabA,
    tab_b: tabB,
    first: a,
    second: b,
  };
  console.log(JSON.stringify(out));
  if (!okA || !okB) fail("one or both parallel waits failed");
}

async function readTiming(path) {
  let raw = "";
  try {
    raw = readFileSync(path, "utf8");
  } catch {
    return [];
  }
  return raw
    .split(/\r?\n/)
    .filter(Boolean)
    .map((line) => {
      try {
        return JSON.parse(line);
      } catch {
        return null;
      }
    })
    .filter(Boolean);
}

async function waitForStart(path, timeoutMs, baseline) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const events = (await readTiming(path)).slice(baseline);
    if (events.some((item) => item.event === "wait_start")) return true;
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  return false;
}

async function cancel([url, session, browserId, tab, timingPath, waitRaw]) {
  if (!url || !session || !browserId || !tab || !timingPath) {
    fail("cancel needs url session browser-id tab timing-jsonl [wait-ms]");
  }
  const waitMs = Number(waitRaw ?? 2500);
  const baseline = (await readTiming(timingPath)).length;
  const firstPromise = callTool(url, session, 501, "browser_wait", {
    browser_id: browserId,
    tab_id: tab,
    selector: "#delay",
    timeout_ms: 30000,
  });
  const started = Date.now();
  const observed = await waitForStart(timingPath, waitMs, baseline);
  if (!observed) fail("mock never recorded wait_start for the cancel request");
  const cancelResponse = await post(url, session, {
    jsonrpc: "2.0",
    method: "notifications/cancelled",
    params: { requestId: 501, reason: "qa conformance" },
  });
  const first = await firstPromise;
  const firstMs = Date.now() - started;
  const followup = await callTool(url, session, 502, "page_text", {
    browser_id: browserId,
    tab_id: tab,
  });
  const cancelCode = first.error?.code;
  const out = {
    mode: "cancel",
    cancel_http: cancelResponse.status,
    first_ms: firstMs,
    first,
    cancel_code: cancelCode,
    followup: { id: followup.id, is_error: followup.is_error, error: followup.error },
  };
  console.log(JSON.stringify(out));
  if (![200, 202, 204].includes(cancelResponse.status)) fail("cancel notification HTTP " + cancelResponse.status);
  if (cancelCode !== -32800) fail("first response was not JSON-RPC -32800");
  if (firstMs >= 2000) fail("cancelled response took " + firstMs + " ms");
  if (followup.error || followup.is_error) fail("follow-up page_text failed");
}

async function cancelNoop([url, session, browserId]) {
  if (!url || !session || !browserId) fail("cancel-noop needs url session browser-id");
  const quick = await callTool(url, session, 601, "page_text", { browser_id: browserId });
  const green = await post(url, session, {
    jsonrpc: "2.0",
    method: "notifications/cancelled",
    params: { requestId: 601, reason: "finished" },
  });
  const unknown = await post(url, session, {
    jsonrpc: "2.0",
    method: "notifications/cancelled",
    params: { requestId: 999999, reason: "unknown" },
  });
  const after = await callTool(url, session, 602, "page_text", { browser_id: browserId });
  const out = {
    mode: "cancel-noop",
    first: { is_error: quick.is_error, error: quick.error },
    finished_http: green.status,
    unknown_http: unknown.status,
    after: { is_error: after.is_error, error: after.error },
  };
  console.log(JSON.stringify(out));
  if (quick.error || quick.is_error) fail("initial page_text failed");
  if (![200, 202, 204].includes(green.status)) fail("finished-id cancel HTTP " + green.status);
  if (![200, 202, 204].includes(unknown.status)) fail("unknown-id cancel HTTP " + unknown.status);
  if (after.error || after.is_error) fail("page_text after no-op cancels failed");
}

async function timing([path, expected, windowRaw]) {
  const events = await readTiming(path);
  const starts = events.filter((item) => item.event === "wait_start");
  const ends = events.filter((item) => item.event === "wait_end");
  if (starts.length < 2 || ends.length < 2) {
    fail("expected two wait_start and two wait_end records, got " + starts.length + "/" + ends.length);
  }
  const window = windowRaw === "last" ? "last" : "first";
  const s = window === "last" ? starts.slice(-2) : starts.slice(0, 2);
  const e = window === "last" ? ends.slice(-2) : ends.slice(0, 2);
  const firstEnd = Math.min(e[0].ms, e[1].ms);
  const secondStart = Math.max(s[0].ms, s[1].ms);
  const overlapped = secondStart < firstEnd;
  const serialized = secondStart >= firstEnd;
  const out = {
    mode: "timing",
    expected,
    starts: s,
    ends: e,
    second_start_minus_first_end_ms: secondStart - firstEnd,
    overlapped,
    serialized,
  };
  console.log(JSON.stringify(out));
  if (expected === "overlap" && !overlapped) fail("waits did not overlap");
  if (expected === "serial" && !serialized) fail("same-tab waits overlapped");
}

async function schema([url]) {
  if (!url) fail("schema needs url");
  const session = await initialize(url);
  const response = await post(url, session, {
    jsonrpc: "2.0",
    id: 2,
    method: "tools/list",
    params: {},
  });
  const body = parseRpc(await response.text());
  if (response.status !== 200 || !body?.result?.tools) fail("tools/list failed");
  console.log(JSON.stringify({ tools: body.result.tools }, null, 2));
}

function normalizeSchema(value) {
  if (Array.isArray(value)) return "[" + value.map(normalizeSchema).join(",") + "]";
  if (value && typeof value === "object") {
    return "{" + Object.keys(value)
      .sort()
      .map((key) => JSON.stringify(key) + ":" + normalizeSchema(value[key]))
      .join(",") + "}";
  }
  return JSON.stringify(value);
}

async function schemaCheck([toolsPath, snapshotPath]) {
  const body = parseRpc(readFileSync(toolsPath, "utf8"));
  const snapshot = JSON.parse(readFileSync(snapshotPath, "utf8"));
  const currentTools = body?.result?.tools ?? body?.tools;
  if (!Array.isArray(snapshot) || snapshot.length < 1) fail("snapshot is empty");
  const expectedCount = snapshot.length;
  if (!Array.isArray(currentTools) || currentTools.length !== expectedCount) {
    fail("tools/list has " + (Array.isArray(currentTools) ? currentTools.length : 0) + " tools, expected " + expectedCount);
  }
  const byName = new Map(currentTools.map((tool) => [tool.name, tool]));
  const diffs = [];
  for (const expected of snapshot) {
    const actual = byName.get(expected.name);
    if (!actual) {
      diffs.push(expected.name + ": missing");
      continue;
    }
    if (normalizeSchema(actual.inputSchema ?? {}) !== normalizeSchema(expected.inputSchema ?? {})) {
      diffs.push(expected.name + ": schema changed");
    }
  }
  if (diffs.length) fail("schema drift: " + diffs.join("; "));
  console.log("schema snapshot matches " + expectedCount + " tools");
}

const [, , mode, ...args] = process.argv;
const handlers = { parallel, cancel, "cancel-noop": cancelNoop, timing, schema, "schema-check": schemaCheck };
const handler = handlers[mode];
if (!handler) fail("unknown mode " + mode);
await handler(args);
