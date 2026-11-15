# Tool reference

[English](09-tools.md) | [中文](09-tools.zh.md)

Status: 1.4.0. Audience: MCP client authors, REST client authors and tool users.

Corvus exposes 43 MCP tools in nine categories. Thirty-three tools operate on a browser through either the extension or a direct loopback CDP connection; ten tools are answered by the daemon. The tool names, input schemas and result shapes in this reference describe 1.4.0. The MCP surface is unchanged from 1.3.0: 1.4.0 adds no tools. The same 43-tool catalogue and executor are also available over the REST surface at `/api/v1/tools` and `/api/v1/tools/call`; the envelope in section 1 is MCP-specific.

## 1. MCP result envelope

A successful `tools/call` returns exactly one content block and no `structuredContent`:

```json
{
  "content": [
    {
      "type": "text",
      "text": "{\"format\":\"compact\",\"tab_id\":\"1\",\"url\":\"https://example.com/\",\"node_count\":42,\"truncated\":false,\"text\":\"@e1 button \\\"Save\\\"\"}"
    }
  ],
  "isError": false
}
```

The text is compact JSON with no embedded newlines. Clients must parse `content[0].text` as JSON. Do not read `structuredContent`: Corvus does not send it. A tool error also returns one text block, with `isError: true` and a compact JSON object carrying the typed error code, message, optional details and optional trace.

This envelope applies to every tool, including `browser_batch`, `tool_help` and the CDP tools. The shape inside the text is the tool result described below.

## 2. Two-level discovery

`tools/list` accepts two optional parameters:

| Parameter | Values | Default | Meaning |
|---|---|---|---|
| `detail` | `brief`, `full` | `brief` | `brief` replaces each long description with a one-line summary, removes `title`, and strips every property description from the input schema. `full` keeps the long description, title, and schema descriptions. |
| `category` | one category name | all | Returns only that category. Unknown categories return typed `BAD_ARGS`. |

The default brief result is 20,452 bytes for all 43 tools and is within the 30,000-byte brief budget; `detail: "full"` is 37,067 bytes, is opt-in and is larger.

`tool_help` is the per-tool escape hatch:

```json
{ "name": "page_snapshot" }
```

returns one full entry with `name`, `title`, `category`, `summary`, `description`, `inputSchema` and `annotations`. `{ "category": "page" }` returns that category's tools, and `{}` returns the catalogue index. `name` and `category` are mutually exclusive; both together return `BAD_ARGS` with `tool_help accepts name or category, not both`.

Every definition also carries `_meta["corvus.category"]`, so clients can group tools without parsing descriptions. Every tool schema accepts an optional `browser_id` routing field; omitting it uses the only connected browser.

## 3. Categories

| Category | Count | Tools |
|---|---:|---|
| `page` | 10 | `browser_navigate`, `page_snapshot`, `page_text`, `page_html`, `page_find`, `browser_wait`, `browser_history`, `browser_dialog`, `browser_emulate`, `browser_batch` |
| `input` | 7 | `browser_click`, `browser_type`, `browser_hover`, `browser_press`, `browser_scroll`, `browser_select`, `browser_eval` |
| `tabs` | 4 | `tab_list`, `tab_new`, `tab_close`, `tab_switch` |
| `network` | 5 | `net_capture`, `net_log`, `net_clear`, `net_intercept`, `browser_cookies` |
| `files` | 3 | `upload_file`, `download_list`, `download_wait` |
| `cdp` | 5 | `cdp_status`, `cdp_targets`, `cdp_send`, `cdp_events`, `browser_channel` |
| `sessions` | 4 | `session_list`, `session_steps`, `session_replay`, `session_export` |
| `diagnostics` | 4 | `browser_list`, `metrics`, `logs_tail`, `browser_console` |
| `help` | 1 | `tool_help` |
| **Total** | **43** | |

`browser_batch` belongs to `page` but is daemon-local: it validates and schedules steps, then each step is routed normally. `browser_channel` belongs to `cdp` but is also daemon-local.

## 4. Tool catalogue

Channel values: `both` means the tool works over either browser channel; `cdp` means it requires a connected CDP endpoint; `daemon` means no browser channel is used.

| Tool | Category | Channel | Purpose |
|---|---|---|---|
| `browser_navigate` | page | both | Open a URL in a tab and wait for the document. |
| `page_snapshot` | page | both | Compact page tree with clickable element references. |
| `page_text` | page | both | Visible page text, capped by `max_chars`. |
| `page_html` | page | both | Document or element HTML, capped by `max_chars`. |
| `page_find` | page | both | Find page text and return element references. |
| `browser_wait` | page | both | Wait for a selector, text or URL condition. |
| `browser_history` | page | both | Go back, go forward or reload. |
| `browser_dialog` | page | cdp | Accept or dismiss a JavaScript dialog. |
| `browser_emulate` | page | cdp | Apply or reset device and network emulation. |
| `browser_batch` | page | daemon | Run up to 20 browser tool calls in order. |
| `browser_click` | input | both | Click an element reference or CSS selector. |
| `browser_type` | input | both | Focus an element and replace its text. |
| `browser_hover` | input | both | Move the pointer over an element. |
| `browser_press` | input | both | Send one key or a key sequence. |
| `browser_scroll` | input | both | Scroll the page or an element. |
| `browser_select` | input | both | Choose values in a select element. |
| `browser_eval` | input | both | Evaluate JavaScript in the page. |
| `tab_list` | tabs | both | List open tabs. |
| `tab_new` | tabs | both | Open a new tab. |
| `tab_close` | tabs | both | Close a tab. |
| `tab_switch` | tabs | both | Make a tab active. |
| `net_capture` | network | both | Start, stop or inspect network capture. |
| `net_log` | network | both | Read captured network requests. |
| `net_clear` | network | both | Clear captured network entries. |
| `net_intercept` | network | both | Set or clear request interception rules. |
| `browser_cookies` | network | cdp | Read browser or page cookies. |
| `upload_file` | files | both | Set local files on a file input. |
| `download_list` | files | both | List recent downloads. |
| `download_wait` | files | both | Wait for a download to finish. |
| `cdp_status` | cdp | cdp | CDP connection, descriptor and ring counts. |
| `cdp_targets` | cdp | cdp | List CDP targets. |
| `cdp_send` | cdp | cdp | Send a raw CDP command. |
| `cdp_events` | cdp | cdp | Read the bounded CDP event ring. |
| `browser_channel` | cdp | daemon | Inspect or switch the browser channel. |
| `session_list` | sessions | daemon | List recorded MCP sessions. |
| `session_steps` | sessions | daemon | Read the steps of one session. |
| `session_replay` | sessions | daemon | Plan or replay a recorded session. |
| `session_export` | sessions | daemon | Export a session as JSON Lines. |
| `browser_list` | diagnostics | daemon | List connected browsers and channel state. |
| `metrics` | diagnostics | daemon | Uptime, tool counters, sessions and recording state. |
| `logs_tail` | diagnostics | daemon | Read the in-process daemon log buffer. |
| `browser_console` | diagnostics | cdp | Recent console and exception entries. |
| `tool_help` | help | daemon | Catalogue index, category or full tool help. |

## 5. page_snapshot

`page_snapshot` returns the page tree used for references. The default output is compact text; `format: "json"` keeps the legacy node objects. The tree includes open shadow roots and same-origin child frames; cross-origin frames are never read.

| Input | Type | Default | Meaning |
|---|---|---|---|
| `tab_id` | string | active tab | Tab to capture. Omit it to use the active tab. |
| `mode` | `interactive` or `full` | `interactive` | `interactive` returns actionable nodes; `full` returns the whole tree. |
| `max_nodes` | integer 40-3000 | 300 | Upper bound on returned nodes. Truncation sets `truncated: true`. |
| `format` | `compact` or `json` | `compact` | `compact` uses the line grammar below; `json` returns node objects. |
| `viewport_only` | boolean | `false` | Keep only nodes intersecting the current viewport. |
| `include_bbox` | boolean | `false` | Add viewport-relative bounding boxes. |
| `diff` | boolean | `false` | Compact output only: mark lines against the previous compact snapshot for the same tab. With `format: "json"` it returns typed `BAD_ARGS`. |

The compact result is:

```json
{
  "format": "compact",
  "tab_id": "1",
  "url": "https://example.com/",
  "title": "Example",
  "viewport": { "width": 1280, "height": 720, "scrollX": 0.0, "scrollY": 0.0 },
  "mode": "interactive",
  "node_count": 3,
  "truncated": false,
  "text": "@e1 button \"Save\"\n@e2 textbox \"Name\" value=\"Ada\"\n# \"Hello\""
}
```

When the page has child frames the compact result also carries `frames`: `{ "total": 2, "included": 1, "skipped": [{ "frame": "f1", "url": "https://other.example/", "reason": "cross_origin" }] }`. `total` counts every child frame, `included` counts the documents that were read, and `skipped` gives a reason of `cross_origin`, `unavailable` or `geometry_unavailable`. A skipped frame is never silently dropped.

The JSON result has the same metadata but replaces `text` with `nodes`. On the extension channel, JSON includes `node_cap`, `element_count`, `text_count` and `ref_count`. On CDP, JSON nodes carry `kind`, `ref`, `tag`, `role`, `name`, `depth`, `states`, `attributes`, and optional `value` and `bbox`; the CDP JSON result does not add the extension-only counts. A JSON node inside a child frame carries `frame` only when the frame path is non-empty; top-document nodes omit the field.

### Compact line grammar

An element line is:

```text
@e12 button "Save" [disabled,focused] value="Save draft" bbox=40,120,96,32
```

```text
@c42 textbox "Name" value="Ada"
```

A text line is:

```text
# "Hello world"
```

The parts are fixed:

1. `@e<number>` on the extension channel, or `@c<backendNodeId>` on CDP. References are valid for the snapshot that returned them.
2. A lowercase ARIA role, or the lowercase tag when no role exists.
3. A double-quoted accessible name. Backslashes and quotes are escaped; newlines and tabs become spaces; names are capped at 120 characters.
4. Optional `[states]` in fixed order: `disabled`, `checked` or `mixed`, `focused`, `required`, `expanded`, `selected`.
5. Optional `value="..."`, capped at 80 characters.
6. Optional `bbox=x,y,w,h`, only with `include_bbox: true`; coordinates are relative to the viewport.
7. Optional `frame="<path>"` as the final field when the node is inside a child frame. The top document has no suffix. Paths follow document order: the first child frame is `f0`, its first child is `f0.0`, and the next sibling is `f1`.

Text lines use `# "..."`, cap text at 240 characters and use the same escaping; text inside a child frame carries the same `frame` suffix. `max_nodes` is one budget across the top document, open shadow roots and included frames, and truncation sets `truncated: true`.

### References and JSON compatibility

- Extension references are `@eN`; CDP references are `@c<backendNodeId>`. A reference from one channel is not valid on the other. Cross-channel use returns typed `BAD_ARGS` with a `details.hint` that says to use the matching reference or omit `tab_id` for the active tab.
- References remain valid while the page snapshot that produced them is current. Re-snapshot after navigation or DOM changes instead of reusing old references.
- Extension tab ids are numeric (for example `7`); CDP target ids are 32-hex strings from `cdp_targets`. 1.4.0 does not translate between the two namespaces. Omit `tab_id` to use the active tab in either channel.
- `format: "json"` is the compatibility path for clients that need node objects. `include_bbox` adds boxes to compact lines on both channels; on the extension path JSON already includes `bbox` in its node objects and `include_bbox` does not change JSON.
- `viewport_only` intersects the node box with the current viewport. It does not scroll the page.

### Compact diffs

`diff: true` is available only for compact output; passing it with `format: "json"` is typed `BAD_ARGS`. The daemon application layer computes the diff once per call from a baseline cache of the 64 most recent snapshots, keyed by browser, session and tab. The browser channels do not compute diffs, so the extension and CDP paths share one grammar, and a `browser_batch` step gets the same treatment exactly once.

A compact request with `diff: false` refreshes the baseline. A diff request never refreshes it, and JSON requests do not touch it. If a baseline exists for the same URL, the result adds `diff: { baseline: true, unchanged, changed, added, removed }` and every current line is prefixed: two spaces for unchanged, `+ ` for added and `~ ` for changed. Baseline lines with no current match are appended in their old order with `- `. If there is no baseline, or the tab moved to a different URL, `diff` is `{ baseline: false, unchanged: 0, changed: 0, added: 0, removed: 0 }` and the text is unmarked.

## 6. browser_batch

`browser_batch` runs up to 20 browser tool calls in order:

```json
{
  "steps": [
    { "tool": "browser_navigate", "args": { "url": "https://example.com/" } },
    { "tool": "page_snapshot", "args": { "format": "compact" } },
    { "tool": "browser_click", "args": { "target": "@e1" } }
  ],
  "stop_on_error": true
}
```

| Input | Type | Default | Meaning |
|---|---|---|---|
| `steps` | array of 1-20 objects | required | Each object has a non-empty `tool` and an optional object `args`; unknown fields are rejected. |
| `stop_on_error` | boolean | `true` | Stop after the first failed step and set `stopped: true`. |

`browser_batch` cannot call itself, `browser_channel`, `metrics`, `logs_tail`, any session tool or `tool_help`. It may call the 34 tools in the page, input, tabs, network, files, cdp and diagnostics categories except `browser_batch`, `browser_channel`, `metrics` and `logs_tail`; that includes `browser_list` and `browser_console`. The allowlist is enforced before execution; a disallowed name returns typed `BAD_ARGS`.

The result is:

```json
{
  "ok": false,
  "count": 2,
  "stopped": true,
  "results": [
    { "step": 1, "tool": "browser_navigate", "ok": true, "data": { "url": "https://example.com/" } },
    { "step": 2, "tool": "page_snapshot", "ok": false, "error": { "code": "CAPABILITY_UNAVAILABLE", "message": "..." } }
  ]
}
```

`count` is the number of executed steps. `ok` is true only when every executed step succeeded. `stopped` is true when `stop_on_error` halted the batch; a later step is not executed and does not appear in `results`. Each step is routed through the normal channel selection; the batch itself is daemon-local and never sent to a browser.

## 7. tool_help

`tool_help` has three modes:

| Input | Result |
|---|---|
| `{}` | `{ "categories": [{ "category", "count", "tools" }], "usage": "..." }` |
| `{ "category": "page" }` | `{ "category": "page", "count": 10, "tools": [{ "name", "title", "summary" }] }` |
| `{ "name": "browser_batch" }` | One full entry: `name`, `title`, `category`, `summary`, `description`, `inputSchema`, `annotations`. |

Unknown names and unknown categories return typed `BAD_ARGS`. `tool_help` is daemon-local and always available, including when no browser is connected.

## 8. Channels and availability

`browser_channel` reports and changes the active mode:

- `auto` (default) prefers a connected CDP endpoint; it falls back to the extension only when the CDP route reports `Unsupported`.
- `cdp` requires CDP and never falls back. 1.4.0 supports all 33 browser-facing tools over forced CDP, including the eight tools that previously required the extension.
- `extension` ignores CDP. It supports the original 25 browser-facing tools and rejects the eight CDP-only tools with typed `CAPABILITY_UNAVAILABLE`.

The ten daemon-local tools work in every mode. `browser_channel` changes only the running daemon; the next start uses `--channel` or `CORVUS_CHANNEL`.

| Availability group | Count | Tools |
|---|---:|---|
| Both channels | 25 | page 7, input 7, tabs 4, network 4, files 3 |
| CDP only | 8 | `browser_dialog`, `browser_emulate`, `browser_cookies`, `browser_console`, `cdp_status`, `cdp_targets`, `cdp_send`, `cdp_events` |
| Extension only | 0 | None in 1.4.0 |
| Daemon local | 10 | `browser_list`, `browser_channel`, `tool_help`, `browser_batch`, four session tools, `metrics`, `logs_tail` |

The extension remains the zero-setup path. A forced `cdp` process can drive every browser-facing tool without an extension, but the browser must be launched with remote debugging and a dedicated profile. See [07-cdp.md](07-cdp.md) for endpoint setup, routing and security.

## 9. Errors and retries

Every failure carries a typed code. The common ones for tool callers:

| Code | Meaning | Retry |
|---|---|---|
| `BAD_ARGS` | Invalid argument, unknown tool/category, cross-channel reference, or a disallowed batch step. Details usually carry `hint` or the rejected field. | No; fix the request. |
| `NOT_FOUND` | The requested target, session, tab or dialog does not exist. `browser_dialog` uses `no dialog is showing` when nothing is open. | After the state changes. |
| `CAPABILITY_UNAVAILABLE` | The active channel or browser cannot serve this call, for example a CDP-only tool under forced `extension`, or forced `cdp` with no endpoint. | Change channel or reconnect. |
| `BROWSER_GONE` | The browser or its CDP connection disappeared while a call was pending. | Re-list tabs or targets, then retry. |
| `TIMEOUT` | A bounded wait or command exceeded its deadline. | Retry only when the operation is idempotent. |

`auto` does not replay a failed CDP action over the extension. Only an `Unsupported` result is eligible for fallback. Every other transport or protocol error is returned to the caller.

## 10. Measured size and version

The release-candidate benchmark measured compact output against the JSON compatibility path:

| Measure | Compact text | JSON | Reduction |
|---|---:|---:|---:|
| Mean snapshot payload | 364.83 B / 91.21 tokens | 2,077.33 B / 519.33 tokens | 82.44% smaller |
| Mean tool result payload | 646.33 B | 2,077.33 B | 68.89% smaller |

Token counts use the benchmark's tokenizer estimate. The 1.5.0 QA report measures the default brief `tools/list` at 20,493 B (array 20,449 B) for 43 tools and full detail at 37,372 B (array 37,328 B); the release notes cite that report.

Version: 1.4.0 is the current published release. The tree also contains unreleased v1.5.0 work with source metadata at 1.5.0: snapshot perception v2 and compact diffs, documented above. Tool catalogues and schemas were checked against the tree; the MCP catalogue is unchanged from 1.3.0 and still has 43 tools in nine categories. No 1.5.0 release or published asset exists yet; QA acceptance PASS is recorded in [v1.5.0.md](../tests/acceptance/v1.5.0.md) at dirty HEAD 73092ea.