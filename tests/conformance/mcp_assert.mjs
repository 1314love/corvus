#!/usr/bin/env node
// Assertions over MCP JSON-RPC response bodies captured by tests/conformance/run.sh.
//
// Usage:
//   node mcp_assert.mjs protocol <body-file>
//   node mcp_assert.mjs tools    <body-file>
//   node mcp_assert.mjs call     <body-file> success [<needle>]
//   node mcp_assert.mjs call     <body-file> error [<ErrorCode>]
//   node mcp_assert.mjs error-detail <body-file> <ErrorCode> [<needle>...]
//   node mcp_assert.mjs navigate <body-file> <true|false> <expected-url>
//   node mcp_assert.mjs snapshot <body-file> <expected-url> [<needle>]
//   node mcp_assert.mjs result   <body-file> <expected-json> [<needle>]
//   node mcp_assert.mjs command  <jsonl-file> <tool> <expected-args-json>
//   node mcp_assert.mjs command-any <jsonl-file> <tool> <expected-args-json>
//   node mcp_assert.mjs command-mode <jsonl-file> <tool> <expected-mode>
//   node mcp_assert.mjs session-list <body-file> <empty|initial|contains> [<session-id>] [<min-steps>]
//   node mcp_assert.mjs steps    <body-file> <session-id> <min-steps>
//   node mcp_assert.mjs step     <body-file> <tool> <expected-args-json> [<expected-result-json>]
//   node mcp_assert.mjs step-seq <body-file> <tool>
//   node mcp_assert.mjs replay   <body-file> <dry_run|live> [<tool>]
//   node mcp_assert.mjs replay-id <body-file>
//   node mcp_assert.mjs export   <body-file>
//   node mcp_assert.mjs metrics  <body-file> [<ErrorCode>...]
//   node mcp_assert.mjs logs     <body-file> [<level>] [<min-entries>]
//
// The body may be plain JSON or an SSE stream (event:/data: lines). Prints one
// diagnostic line and exits 0 on success, 1 on a failed assertion.

import { readFileSync } from "node:fs";

const EXPECTED_PROTOCOL = process.env.EXPECTED_PROTOCOL_VERSION ?? "2025-06-18";
const TOOLS = (
  process.env.EXPECTED_TOOLS ??
  [
    "browser_navigate",
    "page_snapshot",
    "browser_click",
    "browser_type",
    "page_text",
    "page_html",
    "page_find",
    "browser_hover",
    "browser_press",
    "browser_scroll",
    "browser_select",
    "browser_eval",
    "browser_wait",
    "browser_history",
    "tab_list",
    "tab_new",
    "tab_close",
    "tab_switch",
    "browser_list",
    "net_capture",
    "net_log",
    "net_clear",
    "net_intercept",
    "download_list",
    "download_wait",
    "upload_file",
    "session_list",
    "session_steps",
    "session_replay",
    "session_export",
    "metrics",
    "logs_tail",
    "browser_channel",
    "browser_console",
    "browser_cookies",
    "browser_dialog",
    "browser_emulate",
    "browser_batch",
    "cdp_events",
    "cdp_send",
    "cdp_status",
    "cdp_targets",
    "tool_help",
  ].join(",")
)
  .split(",")
  .map((name) => name.trim())
  .filter(Boolean)
  .sort();
const ERROR_CODES = [
  "BAD_ARGS",
  "NOT_FOUND",
  "NOT_INTERACTABLE",
  "TIMEOUT",
  "NAVIGATION_LOST",
  "CAPABILITY_UNAVAILABLE",
  "BROWSER_GONE",
  "TRANSPORT_DOWN",
  "INTERNAL",
];

const [, , mode, file, ...rest] = process.argv;

function fail(message) {
  console.error("FAIL " + message);
  process.exit(1);
}

function ok(message) {
  console.log(message);
}

function loadBody(path) {
  let raw;
  try {
    raw = readFileSync(path, "utf8").trim();
  } catch (error) {
    fail("cannot read " + path + ": " + error.message);
  }
  if (!raw) fail("empty response body");

  let text = raw;
  if (text.startsWith("event:") || text.startsWith("data:") || /\r?\ndata:/.test(text)) {
    const payloads = [];
    for (const block of text.split(/\r?\n\r?\n/)) {
      const data = block
        .split(/\r?\n/)
        .filter((line) => line.startsWith("data:"))
        .map((line) => line.slice(5).trimStart())
        .join("\n");
      if (data.length > 0) payloads.push(data);
    }
    if (payloads.length === 0) fail("SSE body has no data lines");
    text = payloads[payloads.length - 1];
  }

  try {
    return JSON.parse(text);
  } catch (error) {
    fail("body is not JSON: " + text.slice(0, 300));
  }
}

const message =
  mode === "command-mode" || mode === "command" || mode === "command-any"
    ? null
    : loadBody(file);
const expectation = rest[0];
const needle = rest[1];

// Returns null when actual contains every field in expected, or a path string.
function mismatch(actual, expected, path) {
  if (expected === null) {
    return actual === null ? null : path + "=" + JSON.stringify(actual) + " expected null";
  }
  if (Array.isArray(expected)) {
    if (!Array.isArray(actual)) return path + " is not an array";
    if (actual.length !== expected.length) {
      return path + " length=" + actual.length + " expected " + expected.length;
    }
    for (let index = 0; index < expected.length; index += 1) {
      const problem = mismatch(actual[index], expected[index], path + "[" + index + "]");
      if (problem) return problem;
    }
    return null;
  }
  if (typeof expected === "object") {
    if (!actual || typeof actual !== "object" || Array.isArray(actual)) {
      return path + " is not an object";
    }
    for (const [key, value] of Object.entries(expected)) {
      if (!(key in actual)) return path + "." + key + " is missing";
      const problem = mismatch(actual[key], value, path + "." + key);
      if (problem) return problem;
    }
    return null;
  }
  return Object.is(actual, expected)
    ? null
    : path + "=" + JSON.stringify(actual) + " expected " + JSON.stringify(expected);
}

function requireObject(source, label) {
  if (!source || typeof source !== "object" || Array.isArray(source)) {
    fail(label + " has no JSON object body");
  }
  return source;
}

function parseExpectedJson(label, index = 1) {
  const raw = rest[index];
  if (!raw) fail("usage: " + label + " <file> <expected-json>");
  try {
    return JSON.parse(raw);
  } catch (error) {
    fail(label + " expected JSON is invalid: " + error.message);
  }
}

function structuredToolText(source, label) {
  if (source.error) fail(label + " returned JSON-RPC error " + JSON.stringify(source.error));
  const result = source.result;
  if (!result || typeof result !== "object") fail(label + " result missing");
  if (result.isError === true) fail(label + " reported isError: " + JSON.stringify(result).slice(0, 400));
  if (result.structuredContent !== undefined) {
    fail(label + " must not include structuredContent in v1.3");
  }
  if (!Array.isArray(result.content) || result.content.length === 0) {
    fail(label + " result has no content array");
  }
  const text = result.content[0]?.text;
  if (typeof text !== "string" || text.length === 0) fail(label + " content[0].text missing");
  let parsed;
  try {
    parsed = JSON.parse(text);
  } catch {
    fail(label + " text is not JSON: " + text.slice(0, 300));
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    fail(label + " text is not a JSON object: " + text.slice(0, 300));
  }
  return { result, text, parsed };
}

function loadJsonl(path) {
  let raw;
  try {
    raw = readFileSync(path, "utf8");
  } catch (error) {
    fail("cannot read " + path + ": " + error.message);
  }
  const envelopes = [];
  for (const line of raw.split(/\r?\n/)) {
    if (!line.trim()) continue;
    try {
      envelopes.push(JSON.parse(line));
    } catch {
      // Recorder lines are JSON; skip anything malformed rather than masking a
      // missing command envelope.
    }
  }
  return envelopes;
}

if (mode === "command-mode") {
  const [toolName, expectedMode] = rest;
  if (!toolName || !expectedMode) fail("usage: command-mode <jsonl> <tool> <expected-mode>");
  let found = null;
  for (const envelope of loadJsonl(file)) {
    if (envelope.kind === "command" && envelope.tool === toolName) found = envelope;
  }
  if (!found) fail("no command envelope for " + toolName + " in " + file);
  const got = found.args?.mode;
  if (got !== expectedMode) {
    fail(toolName + " args.mode=" + JSON.stringify(got) + " expected " + JSON.stringify(expectedMode));
  }
  ok(toolName + " args.mode=" + JSON.stringify(got));
  process.exit(0);
}

if (mode === "command") {
  const [toolName] = rest;
  const expected = parseExpectedJson("command");
  if (!toolName) fail("usage: command <jsonl> <tool> <expected-args-json>");
  let found = null;
  for (const envelope of loadJsonl(file)) {
    if (envelope.kind === "command" && envelope.tool === toolName) found = envelope;
  }
  if (!found) fail("no command envelope for " + toolName + " in " + file);
  if (typeof found.id !== "string" || found.id.length === 0) fail(toolName + " command has no id");
  if (typeof found.browser !== "string" || found.browser.length === 0) {
    fail(toolName + " command has no browser");
  }
  const problem = mismatch(found.args, expected, toolName + ".args");
  if (problem) fail(problem);
  ok(toolName + " command args match " + JSON.stringify(expected));
  process.exit(0);
}

if (mode === "command-any") {
  const [toolName] = rest;
  const expected = parseExpectedJson("command-any");
  if (!toolName) fail("usage: command-any <jsonl> <tool> <expected-args-json>");
  let candidates = 0;
  let firstProblem = null;
  for (const envelope of loadJsonl(file)) {
    if (envelope.kind !== "command" || envelope.tool !== toolName) continue;
    candidates += 1;
    if (typeof envelope.id !== "string" || envelope.id.length === 0) continue;
    if (typeof envelope.browser !== "string" || envelope.browser.length === 0) continue;
    const problem = mismatch(envelope.args, expected, toolName + ".args");
    if (!problem) {
      ok(toolName + " command args match " + JSON.stringify(expected) + " (" + candidates + " candidates)");
      process.exit(0);
    }
    if (!firstProblem) firstProblem = problem;
  }
  if (candidates === 0) fail("no command envelope for " + toolName + " in " + file);
  fail(firstProblem || toolName + " command envelopes have no id/browser");
}

if (mode === "health") {
  requireObject(message, "health");
  if (typeof message.version !== "string" || message.version.length === 0) fail("health.version missing");
  const protocolOk = message.protocol === 1 || message.protocol === EXPECTED_PROTOCOL;
  if (!protocolOk) {
    fail(
      "health.protocol=" +
        JSON.stringify(message.protocol) +
        " expected 1 or " +
        JSON.stringify(EXPECTED_PROTOCOL),
    );
  }
  if (typeof message.ws_addr !== "string" || message.ws_addr.length === 0) fail("health.ws_addr missing");
  if (
    typeof message.http_addr !== "undefined" &&
    (typeof message.http_addr !== "string" || message.http_addr.length === 0)
  ) {
    fail("health.http_addr is present but not a non-empty string");
  }
  if (!Array.isArray(message.browsers)) fail("health.browsers is not an array");
  ok(
    "version=" +
      message.version +
      " protocol=" +
      message.protocol +
      " ws=" +
      message.ws_addr +
      " http=" +
      (message.http_addr ?? "(not reported)") +
      " browsers=" +
      message.browsers.length,
  );
  process.exit(0);
}

if (mode === "protocol") {
  if (message.error) fail("initialize returned JSON-RPC error " + JSON.stringify(message.error));
  const result = message.result;
  if (!result || typeof result !== "object") fail("initialize result missing");
  if (result.protocolVersion !== EXPECTED_PROTOCOL) {
    fail("protocolVersion=" + JSON.stringify(result.protocolVersion) + " expected " + EXPECTED_PROTOCOL);
  }
  const info = result.serverInfo;
  if (!info || typeof info.name !== "string" || info.name.length === 0) fail("serverInfo.name missing");
  if (!info.version || typeof info.version !== "string") fail("serverInfo.version missing");
  if (!result.capabilities || typeof result.capabilities !== "object") fail("capabilities missing");
  ok("protocolVersion=" + result.protocolVersion + " serverInfo=" + info.name + "@" + info.version);
  process.exit(0);
}

if (mode === "tools") {
  if (message.error) fail("tools/list returned JSON-RPC error " + JSON.stringify(message.error));
  const tools = message.result?.tools;
  if (!Array.isArray(tools)) fail("result.tools is not an array");

  const names = tools.map((tool) => tool?.name).filter((name) => typeof name === "string").sort();
  if (names.length !== tools.length) fail("a tool entry has no string name");
  const missing = TOOLS.filter((name) => !names.includes(name));
  const extra = names.filter((name) => !TOOLS.includes(name));
  if (missing.length > 0) fail("missing tools: " + missing.join(", "));
  if (extra.length > 0) fail("unexpected tools: " + extra.join(", "));
  if (names.length !== TOOLS.length) {
    fail("expected exactly " + TOOLS.length + " tools, got " + names.length);
  }

  const requiredHints = ["readOnlyHint", "destructiveHint", "idempotentHint", "openWorldHint"];
  for (const tool of tools) {
    if (typeof tool.description !== "string" || tool.description.trim().length === 0) {
      fail("tool " + tool.name + " has no description");
    }
    const schema = tool.inputSchema;
    if (!schema || typeof schema !== "object") fail("tool " + tool.name + " has no inputSchema");
    if (schema.type !== "object") {
      fail("tool " + tool.name + " inputSchema.type=" + JSON.stringify(schema.type) + ' expected "object"');
    }
    const annotations = tool.annotations;
    if (!annotations || typeof annotations !== "object") fail("tool " + tool.name + " has no annotations");
    for (const hint of requiredHints) {
      if (typeof annotations[hint] !== "boolean") {
        fail("tool " + tool.name + " annotations." + hint + " is not boolean");
      }
    }
  }

  const shapes = {
    browser_navigate: { required: ["url"], properties: ["url", "tab_id"] },
    page_snapshot: {
      required: [],
      properties: ["tab_id", "mode", "max_nodes"],
      defaults: [["mode", "interactive"]],
      ranges: [["max_nodes", 40, 3000]],
      enums: [["mode", ["interactive", "full"]]],
    },
    browser_click: { required: ["target"], properties: ["target", "tab_id"] },
    browser_type: {
      required: ["target", "text"],
      properties: ["target", "text", "clear", "blur", "tab_id"],
      defaults: [["clear", true], ["blur", false]],
    },
    page_text: {
      required: [],
      properties: ["tab_id", "max_chars"],
      defaults: [["max_chars", 100000]],
      ranges: [["max_chars", 1, 200000]],
    },
    page_html: {
      required: [],
      properties: ["tab_id", "target", "max_chars"],
      defaults: [["max_chars", 200000]],
      ranges: [["max_chars", 1, 500000]],
    },
    page_find: {
      required: ["query"],
      properties: ["query", "tab_id", "role", "limit", "regex"],
      defaults: [["limit", 20], ["regex", false]],
      ranges: [["limit", 1, 100]],
    },
    browser_hover: { required: ["target"], properties: ["target", "tab_id"] },
    browser_press: { required: ["keys"], properties: ["keys", "target", "tab_id"] },
    browser_scroll: {
      required: ["direction"],
      properties: ["direction", "pages", "target", "tab_id"],
      defaults: [["pages", 1]],
      ranges: [["pages", 0, 50]],
      enums: [["direction", ["up", "down", "left", "right"]]],
    },
    browser_select: {
      required: ["target", "values"],
      properties: ["target", "values", "tab_id"],
    },
    browser_eval: {
      required: ["expression"],
      properties: ["expression", "world", "tab_id"],
      defaults: [["world", "isolated"]],
      enums: [["world", ["main", "isolated"]]],
    },
    browser_wait: {
      required: [],
      properties: ["tab_id", "selector", "text", "url_contains", "timeout_ms"],
      defaults: [["timeout_ms", 10000]],
      ranges: [["timeout_ms", 100, 120000]],
    },
    browser_history: {
      required: ["action"],
      properties: ["action", "tab_id"],
      enums: [["action", ["back", "forward", "reload"]]],
    },
    tab_list: { required: [], properties: [] },
    tab_new: {
      required: [],
      properties: ["url", "active"],
      defaults: [["active", true]],
    },
    tab_close: { required: ["tab_id"], properties: ["tab_id"] },
    tab_switch: { required: ["tab_id"], properties: ["tab_id"] },
    browser_list: { required: [], properties: [] },
    net_capture: {
      required: ["action"],
      properties: ["action", "tab_id"],
      enums: [["action", ["start", "stop", "status"]]],
    },
    net_log: {
      required: [],
      properties: [
        "tab_id",
        "since_seq",
        "limit",
        "url_contains",
        "method",
        "resource_type",
        "status_min",
        "status_max",
        "include_headers",
      ],
      defaults: [["since_seq", 0], ["limit", 100], ["include_headers", false]],
      ranges: [["limit", 1, 1000]],
    },
    net_clear: { required: [], properties: ["tab_id"] },
    net_intercept: {
      required: ["action"],
      properties: ["action", "tab_id", "rules"],
      enums: [["action", ["set", "clear"]]],
    },
    download_list: {
      required: [],
      properties: ["limit"],
      defaults: [["limit", 50]],
      ranges: [["limit", 1, 200]],
    },
    download_wait: {
      required: [],
      properties: ["id", "timeout_ms"],
      defaults: [["timeout_ms", 30000]],
      ranges: [["timeout_ms", 100, 120000]],
    },
    upload_file: {
      required: ["target", "files"],
      properties: ["target", "files"],
      arrays: [["files", 1, 10]],
    },
    session_list: {
      required: [],
      properties: ["limit", "since_ms"],
      defaults: [["limit", 50]],
      ranges: [["limit", 1, 200]],
    },
    session_steps: {
      required: ["session_id"],
      properties: ["session_id", "from_seq", "limit"],
      defaults: [["from_seq", 0], ["limit", 100]],
      ranges: [["limit", 1, 500]],
    },
    session_replay: {
      required: ["session_id"],
      properties: ["session_id", "from_seq", "to_seq", "mode", "browser_id", "continue_on_error"],
      defaults: [["mode", "dry_run"], ["continue_on_error", false]],
      enums: [["mode", ["dry_run", "live"]]],
    },
    session_export: {
      required: ["session_id"],
      properties: ["session_id", "format"],
      defaults: [["format", "jsonl"]],
      enums: [["format", ["jsonl"]]],
    },
    metrics: { required: [], properties: [] },
    logs_tail: {
      required: [],
      properties: ["limit", "level", "since_ms"],
      defaults: [["limit", 200]],
      ranges: [["limit", 1, 1000]],
      enums: [["level", ["debug", "info", "warn", "error"]]],
    },
  };
  const problems = [];
  for (const tool of tools) {
    const shape = shapes[tool.name];
    if (!shape) continue;
    const schema = tool.inputSchema ?? {};
    const properties =
      schema.properties && typeof schema.properties === "object" ? schema.properties : {};
    const required = Array.isArray(schema.required) ? schema.required : [];
    const browserId = properties.browser_id;
    if (!browserId || browserId.type !== "string") {
      problems.push(tool.name + " missing optional browser_id string");
    }
    for (const name of shape.properties) {
      if (!(name in properties)) problems.push(tool.name + " missing property " + name);
    }
    for (const name of shape.required) {
      if (!required.includes(name)) problems.push(tool.name + " missing required " + name);
    }
    for (const [name, expectedDefault] of shape.defaults ?? []) {
      if (!properties[name] || properties[name].default !== expectedDefault) {
        problems.push(
          tool.name + "." + name + " default must be " + JSON.stringify(expectedDefault),
        );
      }
    }
    for (const [name, minimum, maximum] of shape.ranges ?? []) {
      const field = properties[name];
      if (!field || field.minimum !== minimum || field.maximum !== maximum) {
        problems.push(tool.name + "." + name + " range must be " + minimum + ".." + maximum);
      }
    }
    for (const [name, values] of shape.enums ?? []) {
      const field = properties[name];
      const actual = Array.isArray(field?.enum) ? field.enum : [];
      for (const value of values) {
        if (!actual.includes(value)) problems.push(tool.name + "." + name + " enum missing " + value);
      }
    }
    for (const [name, minimum, maximum] of shape.arrays ?? []) {
      const field = properties[name];
      if (!field || field.type !== "array") {
        problems.push(tool.name + "." + name + " must be an array");
      } else if (field.minItems !== minimum || field.maxItems !== maximum) {
        problems.push(tool.name + "." + name + " items must be " + minimum + ".." + maximum);
      }
    }
  }
  if (problems.length > 0) fail("schema shape problems: " + problems.join("; "));

  ok("exactly " + names.length + " tools: " + names.join(", "));
  ok(
    tools
      .map(
        (tool) =>
          tool.name +
          "[ro=" +
          tool.annotations.readOnlyHint +
          ",destructive=" +
          tool.annotations.destructiveHint +
          ",idempotent=" +
          tool.annotations.idempotentHint +
          ",openWorld=" +
          tool.annotations.openWorldHint +
          "]",
      )
      .join(" "),
  );
  ok("input schemas match the frozen 43-tool argument shapes and declare browser_id");
  process.exit(0);
}

if (mode === "call") {
  if (expectation === "success") {
    if (message.error) fail("tools/call returned JSON-RPC error " + JSON.stringify(message.error));
    const result = message.result;
    if (!result || typeof result !== "object") fail("tools/call result missing");
    if (result.isError === true) fail("tools/call reported isError: " + JSON.stringify(result).slice(0, 400));
    if (result.structuredContent !== undefined) fail("tools/call must not include structuredContent in v1.3");
    if (!Array.isArray(result.content)) fail("result.content is not an array (MCP requires a content array)");
    if (result.content.length === 0) fail("result.content is empty");
    const serialized = JSON.stringify(result);
    if (needle && !serialized.includes(needle)) {
      fail("result does not contain " + JSON.stringify(needle) + ": " + serialized.slice(0, 400));
    }
    ok(
      "isError=false contentItems=" +
        result.content.length +
        " bytes=" +
        serialized.length +
        (needle ? " needle=" + JSON.stringify(needle) : ""),
    );
    process.exit(0);
  }

  if (expectation === "error") {
    if (message.error && typeof message.error === "object") {
      if (typeof message.error.code !== "number") fail("JSON-RPC error has no numeric code");
      if (typeof message.error.message !== "string" || message.error.message.trim().length === 0) {
        fail("JSON-RPC error has no message");
      }
      ok("structured error jsonrpc code=" + message.error.code + " message=" + JSON.stringify(message.error.message));
      process.exit(0);
    }
    if (message.result && message.result.isError === true) {
      if (!Array.isArray(message.result.content) || message.result.content.length === 0) {
        fail("isError result has no content array");
      }
      const serialized = JSON.stringify(message.result);
      const code = ERROR_CODES.find((candidate) => serialized.includes(candidate));
      if (!code) fail("isError result does not carry an ErrorCode: " + serialized.slice(0, 400));
      if (needle && code !== needle) {
        fail("structured error isError code=" + code + " expected " + needle);
      }
      ok("structured error isError code=" + code);
      process.exit(0);
    }
    fail("tool call was not a structured error: " + JSON.stringify(message).slice(0, 300));
  }

  fail("unknown call expectation " + JSON.stringify(expectation) + " (use success or error)");
}

if (mode === "navigate") {
  const expectedLoaded = rest[0];
  const expectedUrl = rest[1];
  if (expectedLoaded !== "true" && expectedLoaded !== "false") {
    fail("usage: navigate <body-file> <true|false> <expected-url>");
  }
  if (!expectedUrl) fail("usage: navigate <body-file> <true|false> <expected-url>");
  const { text, parsed } = structuredToolText(message, "browser_navigate");
  if (typeof parsed.loaded !== "boolean") {
    fail("navigate text has no boolean loaded field: " + text.slice(0, 300));
  }
  if (String(parsed.loaded) !== expectedLoaded) {
    fail("navigate loaded=" + JSON.stringify(parsed.loaded) + " expected " + expectedLoaded);
  }
  if (parsed.url !== expectedUrl) {
    fail("navigate url=" + JSON.stringify(parsed.url) + " expected " + JSON.stringify(expectedUrl));
  }
  if (!text.includes("loaded") || !text.includes(expectedUrl)) {
    fail("navigate text must carry loaded and the final url: " + text.slice(0, 400));
  }
  ok("isError=false loaded=" + parsed.loaded + " url=" + parsed.url);
  process.exit(0);
}

if (mode === "snapshot") {
  const expectedUrl = rest[0];
  const snapshotNeedle = rest[1];
  if (!expectedUrl) fail("usage: snapshot <body-file> <expected-url> [<needle>]");
  const { text, parsed } = structuredToolText(message, "page_snapshot");
  if (parsed.url !== expectedUrl) {
    fail("snapshot url=" + JSON.stringify(parsed.url) + " expected " + JSON.stringify(expectedUrl));
  }
  if (!Array.isArray(parsed.nodes)) fail("snapshot text has no nodes array: " + text.slice(0, 300));
  if (snapshotNeedle && !text.includes(snapshotNeedle)) {
    fail("snapshot text does not contain " + JSON.stringify(snapshotNeedle) + ": " + text.slice(0, 400));
  }
  ok(
    "isError=false url=" +
      parsed.url +
      " nodes=" +
      parsed.nodes.length +
      (snapshotNeedle ? " needle=" + JSON.stringify(snapshotNeedle) : ""),
  );
  process.exit(0);
}

if (mode === "error-detail") {
  const code = rest[0];
  const needles = rest.slice(1);
  if (!code) fail("usage: error-detail <body-file> <ErrorCode> [<needle>...]");
  if (message.error) fail("expected a structured tool error, got JSON-RPC error " + JSON.stringify(message.error));
  const result = message.result;
  if (!result || result.isError !== true) {
    fail("tool call did not report isError: " + JSON.stringify(result).slice(0, 300));
  }
  if (result.structuredContent !== undefined) fail("isError result must not include structuredContent in v1.3");
  if (!Array.isArray(result.content) || result.content.length === 0) {
    fail("isError result has no content array");
  }
  const text = result.content
    .map((item) => (typeof item?.text === "string" ? item.text : ""))
    .join("\n");
  if (!text.includes(code)) {
    fail("error text does not contain " + JSON.stringify(code) + ": " + text.slice(0, 400));
  }
  for (const needle of needles) {
    if (!text.includes(needle)) {
      fail("error text does not contain " + JSON.stringify(needle) + ": " + text.slice(0, 400));
    }
  }
  ok("isError=true code=" + code + (needles.length ? " needles=" + JSON.stringify(needles) : ""));
  process.exit(0);
}

if (mode === "result") {
  const expected = parseExpectedJson("result", 0);
  const { text, parsed } = structuredToolText(message, "tool result");
  const problem = mismatch(parsed, expected, "result");
  if (problem) fail(problem + " (raw " + text.slice(0, 300) + ")");
  if (needle && !text.includes(needle)) {
    fail("result text does not contain " + JSON.stringify(needle) + ": " + text.slice(0, 400));
  }
  ok(
    "isError=false result matches " +
      JSON.stringify(expected) +
      (needle ? " needle=" + JSON.stringify(needle) : ""),
  );
  process.exit(0);
}

if (mode === "session-list") {
  const expectationName = rest[0] || "";
  const sessionId = rest[1] || "";
  const minSteps = Number(rest[2] || 0);
  const { parsed } = structuredToolText(message, "session_list");
  if (!Array.isArray(parsed.sessions)) fail("session_list has no sessions array");
  if (typeof parsed.total !== "number") fail("session_list has no numeric total");
  if (parsed.total < parsed.sessions.length) fail("session_list total is smaller than the returned array");

  if (expectationName === "empty") {
    if (parsed.sessions.length !== 0 || parsed.total !== 0) {
      fail(
        "session_list expected empty, got " +
          parsed.sessions.length +
          " session(s), total=" +
          parsed.total +
          ": " +
          JSON.stringify(parsed.sessions).slice(0, 300),
      );
    }
    ok("session_list empty total=0");
    process.exit(0);
  }

  if (expectationName === "initial") {
    if (!sessionId) fail("usage: session-list <body> initial <session-id>");
    if (parsed.sessions.length !== 1 || parsed.total !== 1) {
      fail(
        "session_list expected only the current session, got " +
          parsed.sessions.length +
          " session(s), total=" +
          parsed.total,
      );
    }
    const found = parsed.sessions[0];
    if (!found || found.session_id !== sessionId) {
      fail(
        "session_list initial session is not " +
          JSON.stringify(sessionId) +
          ": " +
          JSON.stringify(found).slice(0, 300),
      );
    }
    for (const key of ["session_id", "started_at", "ended_at", "status", "transport", "agent", "steps", "errors"]) {
      if (!(key in found)) fail("session " + sessionId + " missing " + key);
    }
    if (typeof found.started_at !== "number") fail("session " + sessionId + " started_at is not numeric");
    if (found.steps !== 0) fail("session " + sessionId + " initial steps=" + found.steps + " expected 0");
    if (found.errors !== 0) fail("session " + sessionId + " initial errors=" + found.errors + " expected 0");
    if (found.status !== "active") fail("session " + sessionId + " initial status=" + JSON.stringify(found.status));
    if (found.transport !== "http") fail("session " + sessionId + " initial transport=" + JSON.stringify(found.transport));
    ok("session_list initial current session steps=0 errors=0 total=1");
    process.exit(0);
  }

  if (expectationName === "contains") {
    if (!sessionId) fail("usage: session-list <body> contains <session-id> [<min-steps>]");
    const found = parsed.sessions.find((session) => session && session.session_id === sessionId);
    if (!found) {
      fail(
        "session_list has no " +
          JSON.stringify(sessionId) +
          ": " +
          JSON.stringify(parsed.sessions).slice(0, 300),
      );
    }
    for (const key of ["session_id", "started_at", "ended_at", "status", "transport", "agent", "steps", "errors"]) {
      if (!(key in found)) fail("session " + sessionId + " missing " + key);
    }
    if (typeof found.started_at !== "number") fail("session " + sessionId + " started_at is not numeric");
    if (typeof found.steps !== "number") fail("session " + sessionId + " steps is not numeric");
    if (typeof found.errors !== "number") fail("session " + sessionId + " errors is not numeric");
    if (typeof found.status !== "string" || found.status.length === 0) fail("session " + sessionId + " status missing");
    if (typeof found.transport !== "string" || found.transport.length === 0) {
      fail("session " + sessionId + " transport missing");
    }
    if (found.steps < minSteps) {
      fail("session " + sessionId + " steps=" + found.steps + " expected at least " + minSteps);
    }
    let previousStart = Infinity;
    for (const session of parsed.sessions) {
      if (typeof session.started_at !== "number") fail("session_list entry has no numeric started_at");
      if (session.started_at > previousStart) fail("session_list is not newest-first");
      previousStart = session.started_at;
    }
    ok(
      "session_list contains " +
        sessionId +
        " status=" +
        found.status +
        " transport=" +
        found.transport +
        " steps=" +
        found.steps +
        " errors=" +
        found.errors,
    );
    process.exit(0);
  }

  fail("unknown session-list expectation " + JSON.stringify(expectationName) + " (use empty, initial or contains)");
}

if (mode === "steps") {
  const sessionId = rest[0] || "";
  const minSteps = Number(rest[1] || 0);
  if (!sessionId) fail("usage: steps <body-file> <session-id> <min-steps>");
  const { parsed } = structuredToolText(message, "session_steps");
  if (parsed.session_id !== sessionId) {
    fail("session_steps session_id=" + JSON.stringify(parsed.session_id) + " expected " + JSON.stringify(sessionId));
  }
  if (!Array.isArray(parsed.steps)) fail("session_steps has no steps array");
  if (typeof parsed.total_steps !== "number") fail("session_steps has no numeric total_steps");
  if (parsed.steps.length < minSteps) {
    fail("session_steps returned " + parsed.steps.length + " steps, expected at least " + minSteps);
  }
  let previousSeq = -1;
  for (const step of parsed.steps) {
    for (const key of [
      "seq",
      "ts",
      "tool",
      "browser_id",
      "tab_id",
      "ok",
      "error_code",
      "duration_ms",
      "args",
      "args_truncated",
      "result",
      "result_truncated",
    ]) {
      if (!(key in step)) fail("session step missing " + key);
    }
    if (!Number.isInteger(step.seq)) fail("session step seq is not an integer");
    if (step.seq <= previousSeq) fail("session step seq is not strictly ascending: " + step.seq);
    if (typeof step.ts !== "number") fail("session step ts is not numeric");
    if (typeof step.tool !== "string" || step.tool.length === 0) fail("session step tool missing");
    if (typeof step.ok !== "boolean") fail("session step ok is not boolean");
    if (typeof step.duration_ms !== "number") fail("session step duration_ms is not numeric");
    if (typeof step.args_truncated !== "boolean") fail("session step args_truncated is not boolean");
    if (typeof step.result_truncated !== "boolean") fail("session step result_truncated is not boolean");
    if (step.error_code !== null && typeof step.error_code !== "string") fail("session step error_code is not string/null");
    if (step.browser_id !== null && typeof step.browser_id !== "string") fail("session step browser_id is not string/null");
    if (step.tab_id !== null && typeof step.tab_id !== "string") fail("session step tab_id is not string/null");
    previousSeq = step.seq;
  }
  if (parsed.steps.length < parsed.total_steps) {
    if (!Number.isInteger(parsed.next_seq)) fail("session_steps has more steps but no integer next_seq");
  }
  ok(
    "isError=false session=" +
      sessionId +
      " steps=" +
      parsed.steps.length +
      " total_steps=" +
      parsed.total_steps +
      " seq=" +
      (parsed.steps.length ? parsed.steps[0].seq + ".." + parsed.steps[parsed.steps.length - 1].seq : "none"),
  );
  process.exit(0);
}

if (mode === "step") {
  const toolName = rest[0] || "";
  const expectedArgs = parseExpectedJson("step", 1);
  const expectedResultRaw = rest[2] || "";
  if (!toolName) fail("usage: step <body-file> <tool> <expected-args-json> [<expected-result-json>]");
  let expectedResult = null;
  if (expectedResultRaw) {
    try {
      expectedResult = JSON.parse(expectedResultRaw);
    } catch (error) {
      fail("step expected result JSON is invalid: " + error.message);
    }
  }
  const { parsed } = structuredToolText(message, "session_steps");
  if (!Array.isArray(parsed.steps)) fail("session_steps has no steps array");
  let firstProblem = "";
  for (const step of parsed.steps) {
    if (!step || step.tool !== toolName || step.ok !== true) continue;
    if (!step.args || typeof step.args !== "object" || Array.isArray(step.args)) continue;
    const argsProblem = mismatch(step.args, expectedArgs, "args");
    if (argsProblem) {
      if (!firstProblem) firstProblem = argsProblem;
      continue;
    }
    if (expectedResult !== null) {
      if (!step.result || typeof step.result !== "object" || Array.isArray(step.result)) {
        firstProblem = "result is not an object";
        continue;
      }
      const resultProblem = mismatch(step.result, expectedResult, "result");
      if (resultProblem) {
        if (!firstProblem) firstProblem = resultProblem;
        continue;
      }
    }
    ok("step " + toolName + " seq=" + step.seq + " args=" + JSON.stringify(expectedArgs));
    process.exit(0);
  }
  const matching = parsed.steps.filter((step) => step && step.tool === toolName).length;
  fail(
    "no successful parsed step for " +
      toolName +
      " matching " +
      JSON.stringify(expectedArgs) +
      " (" +
      matching +
      " candidate(s))" +
      (firstProblem ? ": " + firstProblem : ""),
  );
}

if (mode === "step-seq") {
  const toolName = rest[0] || "";
  if (!toolName) fail("usage: step-seq <body-file> <tool>");
  const { parsed } = structuredToolText(message, "session_steps");
  if (!Array.isArray(parsed.steps)) fail("session_steps has no steps array");
  const matches = parsed.steps.filter((step) => step && step.tool === toolName && step.ok === true);
  if (matches.length === 0) fail("no successful session step for " + toolName);
  const step = matches[0];
  if (!Number.isInteger(step.seq)) fail("step seq is not an integer");
  ok(String(step.seq));
  process.exit(0);
}

if (mode === "replay") {
  const expectedMode = rest[0] || "";
  const expectedTool = rest[1] || "";
  if (expectedMode !== "dry_run" && expectedMode !== "live") {
    fail("usage: replay <body-file> <dry_run|live> [<tool>]");
  }
  const { parsed } = structuredToolText(message, "session_replay");
  if (parsed.mode !== expectedMode) {
    fail("session_replay mode=" + JSON.stringify(parsed.mode) + " expected " + JSON.stringify(expectedMode));
  }
  if (!Array.isArray(parsed.planned)) fail("session_replay has no planned array");
  if (!Array.isArray(parsed.executed)) fail("session_replay has no executed array");
  if (!Array.isArray(parsed.skipped)) fail("session_replay has no skipped array");
  if (typeof parsed.total !== "number") fail("session_replay has no numeric total");
  for (const item of parsed.planned) {
    if (!item || !Number.isInteger(item.seq) || typeof item.tool !== "string" || !("args" in item)) {
      fail("session_replay planned entry is malformed: " + JSON.stringify(item).slice(0, 200));
    }
  }
  if (expectedMode === "dry_run") {
    if (parsed.executed.length !== 0) {
      fail("dry_run executed " + parsed.executed.length + " step(s): " + JSON.stringify(parsed.executed).slice(0, 300));
    }
    if (expectedTool && !parsed.planned.some((item) => item.tool === expectedTool)) {
      fail("dry_run planned no " + expectedTool + ": " + JSON.stringify(parsed.planned).slice(0, 300));
    }
    const controls = [
      "session_list",
      "session_steps",
      "session_replay",
      "session_export",
      "metrics",
      "logs_tail",
      "browser_list",
    ];
    const plannedControl = parsed.planned.find((item) => controls.includes(item.tool));
    if (plannedControl) fail("dry_run planned a control tool: " + plannedControl.tool);
    if (!parsed.skipped.some((item) => item && item.reason === "control_tool")) {
      fail("dry_run skipped no control_tool entries: " + JSON.stringify(parsed.skipped).slice(0, 300));
    }
    ok(
      "isError=false mode=dry_run planned=" +
        parsed.planned.length +
        " skipped=" +
        parsed.skipped.length +
        " executed=0",
    );
    process.exit(0);
  }

  if (parsed.executed.length === 0) fail("live replay executed no steps");
  for (const item of parsed.executed) {
    if (!item || typeof item.tool !== "string" || typeof item.ok !== "boolean") {
      fail("session_replay executed entry is malformed: " + JSON.stringify(item).slice(0, 200));
    }
  }
  if (expectedTool && !parsed.executed.some((item) => item.tool === expectedTool && item.ok === true)) {
    fail("live replay did not execute " + expectedTool + " successfully: " + JSON.stringify(parsed.executed).slice(0, 300));
  }
  if (typeof parsed.replay_session_id !== "string" || parsed.replay_session_id.length === 0) {
    fail("live replay returned no replay_session_id");
  }
  ok(
    "isError=false mode=live planned=" +
      parsed.planned.length +
      " executed=" +
      parsed.executed.length +
      " replay_session=" +
      parsed.replay_session_id,
  );
  process.exit(0);
}

if (mode === "replay-id") {
  const { parsed } = structuredToolText(message, "session_replay");
  if (typeof parsed.replay_session_id !== "string" || parsed.replay_session_id.length === 0) {
    fail("session_replay has no replay_session_id");
  }
  ok(parsed.replay_session_id);
  process.exit(0);
}

if (mode === "export") {
  const { parsed } = structuredToolText(message, "session_export");
  if (parsed.format !== "jsonl") fail("session_export format=" + JSON.stringify(parsed.format) + " expected jsonl");
  if (typeof parsed.content !== "string") fail("session_export content is not a string");
  if (typeof parsed.bytes !== "number") fail("session_export bytes is not numeric");
  if (typeof parsed.truncated !== "boolean") fail("session_export truncated is not boolean");
  const lines = parsed.content.split(/\r?\n/).filter((line) => line.trim().length > 0);
  if (lines.length < 2) fail("session_export has fewer than two JSONL lines");
  const objects = [];
  for (const line of lines) {
    try {
      objects.push(JSON.parse(line));
    } catch (error) {
      fail("session_export has an invalid JSONL line: " + line.slice(0, 200));
    }
  }
  if (objects[0].type !== "session") fail("session_export first line type=" + JSON.stringify(objects[0].type));
  if (typeof objects[0].id !== "string" || objects[0].id.length === 0) {
    fail("session_export first line has no session id");
  }
  if (objects[0].id !== parsed.session_id) {
    fail("session_export first line id does not match the response");
  }
  const steps = objects.slice(1);
  if (steps.length === 0) fail("session_export has no step lines");
  let previousSeq = -1;
  for (const step of steps) {
    if (step.type !== "step") fail("session_export line type=" + JSON.stringify(step.type));
    if (!Number.isInteger(step.seq)) fail("session_export step seq is not an integer");
    if (step.seq <= previousSeq) fail("session_export step seq is not strictly ascending");
    previousSeq = step.seq;
  }
  ok(
    "session_export jsonl session=" +
      parsed.session_id +
      " steps=" +
      steps.length +
      " bytes=" +
      parsed.bytes +
      " truncated=" +
      parsed.truncated,
  );
  process.exit(0);
}

if (mode === "metrics") {
  const { parsed } = structuredToolText(message, "metrics");
  for (const key of [
    "uptime_ms",
    "started_at",
    "browsers_connected",
    "sessions_total",
    "sessions_active",
    "tool_calls_total",
    "tool_errors_total",
    "inflight",
  ]) {
    if (typeof parsed[key] !== "number") fail("metrics." + key + " is not numeric");
  }
  if (!Number.isInteger(parsed.protocol) || parsed.protocol < 1) {
    fail("metrics.protocol is not a wire protocol version integer");
  }
  if (!Array.isArray(parsed.browsers)) fail("metrics.browsers is not an array");
  if (!parsed.by_tool || typeof parsed.by_tool !== "object" || Array.isArray(parsed.by_tool)) {
    fail("metrics.by_tool is not an object");
  }
  if (!parsed.errors_by_code || typeof parsed.errors_by_code !== "object" || Array.isArray(parsed.errors_by_code)) {
    fail("metrics.errors_by_code is not an object");
  }
  for (const [code, count] of Object.entries(parsed.errors_by_code)) {
    if (typeof count !== "number") fail("metrics.errors_by_code." + code + " is not numeric");
  }
  if (!parsed.recording || typeof parsed.recording !== "object" || Array.isArray(parsed.recording)) {
    fail("metrics.recording is not an object");
  }
  if (typeof parsed.recording.enabled !== "boolean") fail("metrics.recording.enabled is not boolean");
  if (typeof parsed.recording.db_path !== "string" || parsed.recording.db_path.length === 0) {
    fail("metrics.recording.db_path missing");
  }
  if (typeof parsed.recording.sessions !== "number") fail("metrics.recording.sessions is not numeric");
  if (typeof parsed.recording.steps !== "number") fail("metrics.recording.steps is not numeric");
  if (parsed.tool_calls_total <= 0) fail("metrics.tool_calls_total is not positive");
  if (parsed.sessions_total <= 0) fail("metrics.sessions_total is not positive");
  for (const code of rest) {
    if (!(code in parsed.errors_by_code) || parsed.errors_by_code[code] <= 0) {
      fail("metrics.errors_by_code has no positive " + code + " count: " + JSON.stringify(parsed.errors_by_code));
    }
  }
  ok(
    "isError=false tool_calls=" +
      parsed.tool_calls_total +
      " errors=" +
      parsed.tool_errors_total +
      " sessions=" +
      parsed.sessions_total +
      " codes=" +
      Object.keys(parsed.errors_by_code).length,
  );
  process.exit(0);
}

if (mode === "logs") {
  const expectedLevel = rest[0] || "";
  const minEntries = rest[1] === undefined ? 1 : Number(rest[1]);
  const { parsed } = structuredToolText(message, "logs_tail");
  if (!Array.isArray(parsed.entries)) fail("logs_tail has no entries array");
  if (typeof parsed.dropped !== "number") fail("logs_tail.dropped is not numeric");
  const levels = ["debug", "info", "warn", "error"];
  for (const entry of parsed.entries) {
    if (!entry || typeof entry !== "object") fail("logs_tail entry is not an object");
    if (typeof entry.ts !== "number") fail("logs_tail entry ts is not numeric");
    if (!levels.includes(entry.level)) fail("logs_tail entry level=" + JSON.stringify(entry.level));
    if (typeof entry.target !== "string") fail("logs_tail entry target is not a string");
    if (typeof entry.message !== "string") fail("logs_tail entry message is not a string");
    if (!entry.fields || typeof entry.fields !== "object" || Array.isArray(entry.fields)) {
      fail("logs_tail entry fields is not an object");
    }
    if (expectedLevel && entry.level !== expectedLevel) {
      fail("logs_tail level=" + JSON.stringify(entry.level) + " expected " + JSON.stringify(expectedLevel));
    }
  }
  if (parsed.entries.length < minEntries) {
    fail("logs_tail returned " + parsed.entries.length + " entries, expected at least " + minEntries);
  }
  ok(
    "isError=false entries=" +
      parsed.entries.length +
      " dropped=" +
      parsed.dropped +
      (expectedLevel ? " level=" + expectedLevel : ""),
  );
  process.exit(0);
}

fail("unknown mode " + JSON.stringify(mode) + " (use protocol, tools, call, error-detail, navigate, snapshot, result, command, command-any, command-mode, session-list, steps, step, step-seq, replay, replay-id, export, metrics or logs)");
