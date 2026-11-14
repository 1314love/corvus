#!/usr/bin/env node
// Assertions over MCP JSON-RPC response bodies captured by tests/conformance/run.sh.
//
// Usage:
//   node mcp_assert.mjs protocol <body-file>
//   node mcp_assert.mjs tools    <body-file>
//   node mcp_assert.mjs call     <body-file> success [<needle>]
//   node mcp_assert.mjs call     <body-file> error [<ErrorCode>]
//   node mcp_assert.mjs navigate <body-file> <true|false> <expected-url>
//   node mcp_assert.mjs snapshot <body-file> <expected-url> [<needle>]
//   node mcp_assert.mjs result   <body-file> <expected-json> [<needle>]
//   node mcp_assert.mjs command  <jsonl-file> <tool> <expected-args-json>
//   node mcp_assert.mjs command-any <jsonl-file> <tool> <expected-args-json>
//   node mcp_assert.mjs command-mode <jsonl-file> <tool> <expected-mode>
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
  };
  const problems = [];
  for (const tool of tools) {
    const shape = shapes[tool.name];
    if (!shape) continue;
    const schema = tool.inputSchema ?? {};
    const properties =
      schema.properties && typeof schema.properties === "object" ? schema.properties : {};
    const required = Array.isArray(schema.required) ? schema.required : [];
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
  ok("input schemas match the frozen 18-tool argument shapes");
  process.exit(0);
}

if (mode === "call") {
  if (expectation === "success") {
    if (message.error) fail("tools/call returned JSON-RPC error " + JSON.stringify(message.error));
    const result = message.result;
    if (!result || typeof result !== "object") fail("tools/call result missing");
    if (result.isError === true) fail("tools/call reported isError: " + JSON.stringify(result).slice(0, 400));
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

fail("unknown mode " + JSON.stringify(mode) + " (use protocol, tools, call, navigate, snapshot, result, command or command-mode)");
