#!/usr/bin/env node
// Conformance check: a policy denial must be a typed POLICY_DENIED error
// (MCP isError result) and the REST envelope must carry retryable=false.
// Starts its own isolated daemon on CORVUS_GUARDRAILS_WS_PORT/HTTP_PORT
// (default 29332/29333) with a read-only policy.
import { spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const BIN = process.env.CORVUSD_BIN || "";
const WS_PORT = Number(process.env.CORVUS_GUARDRAILS_WS_PORT || 29332);
const HTTP_PORT = Number(process.env.CORVUS_GUARDRAILS_HTTP_PORT || 29333);
const MCP_URL = "http://127.0.0.1:" + HTTP_PORT + "/mcp";
const HEALTH_URL = "http://127.0.0.1:" + HTTP_PORT + "/health";
const REST_URL = "http://127.0.0.1:" + HTTP_PORT + "/api/v1/tools/call";

function fail(message) {
  console.error("FAIL guardrails POLICY_DENIED typed retryable=false: " + message);
  process.exit(1);
}

if (!BIN) fail("CORVUSD_BIN is required");

const work = process.env.CORVUS_CONFORMANCE_TMP
  ? join(process.env.CORVUS_CONFORMANCE_TMP, "guardrails-check")
  : mkdtempSync(join(tmpdir(), "corvus-guardrails-"));
mkdirSync(work, { recursive: true });
const policy = join(work, "read-only-policy.json");
writeFileSync(policy, JSON.stringify({ version: 1, read_only: true }) + "\n", "utf8");

const checks = [];
const failures = [];
function check(name, condition, detail) {
  if (condition) checks.push(name);
  else failures.push(name + " (" + detail + ")");
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

const child = spawn(
  BIN,
  [
    "serve",
    "--ws-port", String(WS_PORT),
    "--http-port", String(HTTP_PORT),
    "--db", join(work, "guardrails.db"),
    "--home", join(work, "home"),
    "--no-log-file",
    "--channel", "extension",
    "--guardrails", policy,
  ],
  { stdio: ["ignore", "ignore", "pipe"], windowsHide: true },
);
let stderr = "";
child.stderr.on("data", (chunk) => { stderr += chunk.toString(); });

async function stopDaemon() {
  if (child.exitCode === null) {
    child.kill();
    await Promise.race([
      new Promise((resolve) => child.once("exit", resolve)),
      sleep(3000),
    ]);
  }
  if (child.exitCode === null) {
    spawn("taskkill", ["/F", "/T", "/PID", String(child.pid)], { stdio: "ignore", windowsHide: true });
    await sleep(500);
  }
}

async function finish(code) {
  await stopDaemon();
  if (!process.env.KEEP_TMP) rmSync(work, { recursive: true, force: true });
  process.exit(code);
}

let health = null;
for (let attempt = 0; attempt < 100; attempt += 1) {
  try {
    const response = await fetch(HEALTH_URL);
    if (response.ok) {
      health = await response.json();
      break;
    }
  } catch {
    // daemon still starting
  }
  if (child.exitCode !== null) break;
  await sleep(100);
}
if (!health) {
  const detail = stderr.trim() || "no health response";
  await finish(1);
  fail("daemon did not become healthy: " + detail.slice(0, 600));
}

check("health guardrails enabled", health.guardrails && health.guardrails.enabled === true, JSON.stringify(health.guardrails));
check("health read_only", health.guardrails && health.guardrails.read_only === true, JSON.stringify(health.guardrails));
check("health has no policy path", !JSON.stringify(health.guardrails || {}).includes("read-only-policy.json"), JSON.stringify(health.guardrails));

async function mcpCall(name, args) {
  const response = await fetch(MCP_URL, {
    method: "POST",
    headers: { "content-type": "application/json", accept: "application/json, text/event-stream" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "tools/call", params: { name, arguments: args } }),
  });
  const text = await response.text();
  let body = null;
  try { body = JSON.parse(text); } catch { fail("MCP " + name + " returned non-JSON: " + text.slice(0, 300)); }
  return { status: response.status, body, text };
}

function toolText(body) {
  const content = body && body.result && Array.isArray(body.result.content) ? body.result.content : [];
  return content.map((item) => (typeof item.text === "string" ? item.text : "")).join("\n");
}

const denied = await mcpCall("browser_click", { target: "#guard-btn" });
const deniedText = toolText(denied.body);
check("MCP denial HTTP 200", denied.status === 200, "HTTP " + denied.status);
check("MCP denial isError", denied.body && denied.body.result && denied.body.result.isError === true, deniedText.slice(0, 300));
check("MCP denial code POLICY_DENIED", deniedText.startsWith("POLICY_DENIED:"), deniedText.slice(0, 300));
check("MCP denial reason read_only", deniedText.includes("read_only"), deniedText.slice(0, 300));
check("MCP denial has no structuredContent", !(denied.body && denied.body.result && denied.body.result.structuredContent !== undefined), "structuredContent present");

const batch = await mcpCall("browser_batch", { steps: [{ tool: "page_text", args: {} }] });
const batchText = toolText(batch.body);
check("read_only denies browser_batch", batch.body && batch.body.result && batch.body.result.isError === true && batchText.startsWith("POLICY_DENIED:"), batchText.slice(0, 300));

const allowed = await mcpCall("browser_list", {});
check("read_only allows browser_list", allowed.body && allowed.body.result && allowed.body.result.isError === false, toolText(allowed.body).slice(0, 300));

const restResponse = await fetch(REST_URL, {
  method: "POST",
  headers: { "content-type": "application/json" },
  body: JSON.stringify({ name: "browser_click", arguments: { target: "#guard-btn" } }),
});
const restText = await restResponse.text();
let restBody = null;
try { restBody = JSON.parse(restText); } catch { fail("REST returned non-JSON: " + restText.slice(0, 300)); }
check("REST denial HTTP 403", restResponse.status === 403, "HTTP " + restResponse.status);
check("REST error code POLICY_DENIED", restBody && restBody.error && restBody.error.code === "POLICY_DENIED", restText.slice(0, 300));
check("REST error retryable=false", restBody && restBody.error && restBody.error.retryable === false, restText.slice(0, 300));
check("REST error reason read_only", restBody && restBody.error && restBody.error.details && restBody.error.details.reason === "read_only", restText.slice(0, 300));

if (failures.length > 0) {
  await finish(1);
  fail(failures.join("; "));
}
console.log("PASS guardrails POLICY_DENIED typed retryable=false (" + checks.length + " checks: MCP isError + REST 403; ports " + WS_PORT + "/" + HTTP_PORT + ")");
await finish(0);
