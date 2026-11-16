# Tool reference

[English](09-tools.md) | [中文](09-tools.zh.md)

Status: 1.6.0 is the current published release, released on 2026-11-15 from tag `v1.6.0`; 1.5.0 is the previous release. The working tree contains unreleased 1.7.0 work that expands the MCP surface to 62 tools in ten categories, including the new `storage` category and 18 new tools. The 1.7.0 surface has 46 CDP browser-facing tools, 42 extension handlers and 12 daemon-local tools; the extension adds `tabGroups` for a total of 11 permissions, keeps `all_frames: false`, and has 42 handlers. QA acceptance and local Windows packaging passed on the re-frozen 62-tool tree; no 1.7.0 CI run, published package or published asset exists, so every CI and published-release row remains Pending and the working tree remains unreleased. The 1.6.0 release adds opt-in daemon guardrails and no new tool; the tag object `c5daf98eb03b0fac310e53c3a59edfa77874f919` targets `b420c3787a8f240c60514ac8c745e67474f1c1fd`, main CI run 43716832915 passed eight jobs, release workflow 43717236667 succeeded, and GitHub Release 446071476 published the assets. QA PASS is recorded in [v1.6.0.md](../tests/acceptance/v1.6.0.md) on the working tree at HEAD `61ca85f`. Audience: MCP client authors, REST client authors and tool users.

Corvus exposes 62 MCP tools in ten categories in the 1.7.0 working tree. Forty-six browser-facing tools can run over a direct loopback CDP connection, 42 can run through the extension, and 12 are answered by the daemon; 38 tools are available on both browser channels, eight are CDP-only and four are extension-only. The published 1.6.0 surface has 43 tools. The 1.7.0 tree adds 18 tools: four page tools, three input tools, four tab and window tools, five storage tools and two diagnostics tools. The tool names, input schemas and result shapes in this reference describe that tree. The MCP surface was unchanged from 1.3.0 through 1.6.0: those releases add none. The optional daemon guardrails policy is described in [11-guardrails.md](11-guardrails.md). The same 62-tool catalogue in the working tree (43 in the published 1.6.0 release) and executor are also available over the REST surface at `/api/v1/tools` and `/api/v1/tools/call`; the envelope in section 1 is MCP-specific.

## 1. MCP result envelope

A successful `tools/call` returns exactly one content block and no `structuredContent`, except `browser_screenshot` with `inline: true`, which returns a text block followed by an image block:

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

This envelope applies to every tool, including `browser_batch`, `tool_help` and the CDP tools. The one exception is an inline screenshot: `tool_success` emits `content[0]` as the summary text and `content[1]` as `{ "type": "image", "data": "<base64>", "mimeType": "image/webp" }`, with no `structuredContent`. A `browser_screenshot` without `inline` returns the usual single text block. The shape inside the text is the tool result described below.

## 2. Two-level discovery

`tools/list` accepts two optional parameters:

| Parameter | Values | Default | Meaning |
|---|---|---|---|
| `detail` | `brief`, `full` | `brief` | `brief` replaces each long description with a one-line summary, removes `title`, and strips every property description from the input schema. `full` keeps the long description, title, and schema descriptions. |
| `category` | one category name | all | Returns only that category. Unknown categories return typed `BAD_ARGS`. |

The default brief result measured 20,493 bytes for the 43-tool 1.6.0 catalogue and is within the 30,000-byte brief budget; `detail: "full"` measured 37,496 bytes, is opt-in and is larger. The 62-tool 1.7.0 catalogue measured a 31,367-byte raw brief body (31,368 B with the trailing newline), within the 32,000-byte brief budget, and a 52,328-byte raw full body (52,329 B with the newline); no 44-tool measurement applies to the new surface.

`tool_help` is the per-tool escape hatch:

```json
{ "name": "page_snapshot" }
```

returns one full entry with `name`, `title`, `category`, `summary`, `description`, `inputSchema` and `annotations`. `{ "category": "page" }` returns that category's tools, and `{}` returns the catalogue index. `name` and `category` are mutually exclusive; both together return `BAD_ARGS` with `tool_help accepts name or category, not both`.

Every definition also carries `_meta["corvus.category"]`, so clients can group tools without parsing descriptions. Every tool schema accepts an optional `browser_id` routing field; omitting it uses the only connected browser.

## 3. Categories

| Category | Count | Tools |
|---|---:|---|
| `page` | 15 | `browser_navigate`, `page_snapshot`, `page_text`, `page_html`, `page_find`, `browser_wait`, `browser_history`, `browser_screenshot`, `page_pdf`, `page_links`, `page_forms`, `page_accessibility`, `browser_dialog`, `browser_emulate`, `browser_batch` |
| `input` | 10 | `browser_click`, `browser_type`, `browser_hover`, `browser_press`, `browser_scroll`, `browser_select`, `browser_eval`, `browser_drag`, `browser_focus`, `browser_highlight` |
| `tabs` | 8 | `tab_list`, `tab_new`, `tab_close`, `tab_switch`, `tab_group`, `tab_duplicate`, `window_list`, `window_new` |
| `network` | 5 | `net_capture`, `net_log`, `net_clear`, `net_intercept`, `browser_cookies` |
| `storage` | 5 | `cookie_set`, `cookie_clear`, `storage_get`, `storage_set`, `storage_clear` |
| `files` | 3 | `upload_file`, `download_list`, `download_wait` |
| `cdp` | 5 | `cdp_status`, `cdp_targets`, `cdp_send`, `cdp_events`, `browser_channel` |
| `sessions` | 4 | `session_list`, `session_steps`, `session_replay`, `session_export` |
| `diagnostics` | 6 | `browser_list`, `metrics`, `logs_tail`, `browser_console`, `health`, `version` |
| `help` | 1 | `tool_help` |
| **Total** | **62** | |

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
| `browser_screenshot` | page | both | Capture the viewport, full page or one element. |
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
| `browser_cookies` | network | cdp | Read browser or page cookies. A guardrails `redact_secrets` policy replaces each top-level cookie `value` with `[redacted]`. |
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
| `health` | diagnostics | daemon | Redacted daemon health, version and connection summary. |
| `version` | diagnostics | daemon | Product version, tool count and protocol version. |
| `tool_help` | help | daemon | Catalogue index, category or full tool help. |
| `page_pdf` | page | both | Save the page as a PDF file. |
| `page_links` | page | both | Extract links from the document and open shadow roots. |
| `page_forms` | page | both | Inspect forms and their fields. |
| `page_accessibility` | page | both | Read the accessibility tree. |
| `browser_drag` | input | both | Drag from one element reference to another. |
| `browser_focus` | input | both | Focus an element and scroll it into view. |
| `browser_highlight` | input | both | Briefly highlight a reference or selector. |
| `tab_group` | tabs | extension | Group tabs and set the group title and colour. |
| `tab_duplicate` | tabs | extension | Duplicate one tab. |
| `window_list` | tabs | extension | List browser windows and optionally their tabs. |
| `window_new` | tabs | extension | Create a browser window. |
| `cookie_set` | storage | both | Set one cookie without echoing its value. |
| `cookie_clear` | storage | both | Clear cookies by URL or domain. |
| `storage_get` | storage | both | Read page local or session storage. |
| `storage_set` | storage | both | Write page local or session storage. |
| `storage_clear` | storage | both | Clear page local or session storage. |

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

Snapshot password values always render as `***`. `browser_type` differs by channel: the extension returns `masked: true` for password fields and a truncated `value` otherwise, while CDP omits `value` and does not emit `masked`, so no typed value is returned. With a daemon `content_boundary` policy, compact `page_snapshot` text is wrapped in untrusted-content markers; JSON nodes are unchanged. See [11-guardrails.md](11-guardrails.md).

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
- Extension tab ids are numeric (for example `7`); CDP target ids are 32-hex strings from `cdp_targets`. The current release does not translate between the two namespaces. Omit `tab_id` to use the active tab in either channel.
- `format: "json"` is the compatibility path for clients that need node objects. `include_bbox` adds boxes to compact lines on both channels; on the extension path JSON already includes `bbox` in its node objects and `include_bbox` does not change JSON.
- `viewport_only` intersects the node box with the current viewport. It does not scroll the page.

### Compact diffs

`diff: true` is available only for compact output; passing it with `format: "json"` is typed `BAD_ARGS`. The daemon application layer computes the diff once per call from a baseline cache of the 64 most recent snapshots, keyed by browser, session and tab. The browser channels do not compute diffs, so the extension and CDP paths share one grammar, and a `browser_batch` step gets the same treatment exactly once.

A compact request with `diff: false` refreshes the baseline. A diff request never refreshes it, and JSON requests do not touch it. If a baseline exists for the same URL, the result adds `diff: { baseline: true, unchanged, changed, added, removed }` and every current line is prefixed: two spaces for unchanged, `+ ` for added and `~ ` for changed. Baseline lines with no current match are appended in their old order with `- `. If there is no baseline, or the tab moved to a different URL, `diff` is `{ baseline: false, unchanged: 0, changed: 0, added: 0, removed: 0 }` and the text is unmarked.

### browser_screenshot

`browser_screenshot` captures the visible viewport, the full scrollable page or one element. It is available on both channels. The daemon finalizes every result: the image is always written under `<home>/screenshots/` as `screenshot-<unix-millis>-<uuid8>.<ext>`, and the result replaces the base64 payload with `width`, `height`, `bytes`, `sha256`, `path`, `summary` and `inline`.

| Input | Type | Default | Meaning |
|---|---|---|---|
| `target` | string | `viewport` | `viewport`, `full_page` or `element`. |
| `ref` | string | none | Required only for `element`; a `@c<backendNodeId>` reference on CDP or a `@e<number>` reference on the extension. A reference from the other channel is typed `BAD_ARGS`. |
| `format` | string | `webp` | `webp`, `png` or `jpeg`. |
| `quality` | integer | `80` | `1` to `100` for `webp` and `jpeg`; ignored for `png`. |
| `inline` | boolean | `false` | Also return the bytes as MCP image content. The file is always saved. |
| `tab_id` | string | active tab | Tab to capture. |

`full_page` uses the page layout metrics and captures beyond the viewport; `element` resolves the node box from the snapshot reference and captures that clip. The extension uses the existing debugger channel, with a `chrome.tabs.captureVisibleTab` fallback for an active-tab PNG viewport; 1.7.0 raises the extension to 11 permissions, 42 handlers and `all_frames: false`.

`inline: true` is MCP-only: `content[0]` is the summary text and `content[1]` is the image, with no `structuredContent`. A `browser_batch` screenshot step must set `inline: false`; `inline: true` is typed `BAD_ARGS` before dispatch. Screenshots are read-only for guardrails, but pixels are not redacted or boundary-wrapped, so an image can contain secrets or untrusted page content.

### New tools in 1.7.0

Eighteen tools are new in the 1.7.0 working tree. Every browser tool accepts the optional `browser_id`; the page and input tools also accept `tab_id` and use the active tab when it is omitted. References use `@c<backendNodeId>` on CDP and `@e<number>` on the extension; the wrong kind is typed `BAD_ARGS`. The four extension-only tools return `CAPABILITY_UNAVAILABLE` on CDP. The five storage tools are not batchable.

| Tool | Category | Channel | Inputs | Result and notes |
|---|---|---|---|---|
| `page_pdf` | page | both | `browser_id?`, `tab_id?`, `paper_width` 1-100 (8.5 in), `paper_height` 1-100 (11 in), `landscape` false, `print_background` true, `scale` 0.1-2 (1) | PDF file: `format`, `mime_type`, `tab_id`, `url`, `path`, `bytes`, `sha256`, paper fields and `summary`; no inline or base64 field. |
| `page_links` | page | both | `selector?` (512 chars), `max_results` 1-1000 (200), `include_hidden` false | `url`, `count`, `truncated`, `links[{text,url,title?,rel?,target?}]`; scans the top document and open shadow roots, not same-origin iframes. |
| `page_forms` | page | both | `selector?` (512 chars), `include_values` false, `max_forms` 1-100 (20) | `url`, `count`, `truncated`, `forms[{index,action,method,name?,id?,fields}]`; password values are never returned. |
| `page_accessibility` | page | both | `root?` (512 chars), `max_nodes` 1-2000 (500), `max_depth` 1-50 (12) | `url`, `count`, `truncated`, `nodes[{role,name,value?,description?,disabled?,checked?,expanded?,selected?,level?,depth}]`; no debugger means `CAPABILITY_UNAVAILABLE`. |
| `browser_drag` | input | both | `from_ref`, `to_ref`, `steps` 1-50 (10) | Pointer drag from one reference to another: `dragged`, `from`, `to`, `tab_id`, `url`; HTML5 drag-and-drop is not guaranteed. |
| `browser_focus` | input | both | `ref` | Focus and scroll into view: `focused`, `ref`, `tab_id`, `url`. |
| `browser_highlight` | input | both | exactly one of `ref` or `selector`; `duration_ms` 0-10000 (1200); `color` `#rrggbb` (`#ffd400`) | Transient highlight: `highlighted`, the reference or selector, `duration_ms`, `tab_id`, `url`. |
| `tab_group` | tabs | extension | `tab_ids` 1-50 distinct, `title` (64 chars), `color` grey-blue-red-yellow-green-pink-purple-cyan-orange (blue) | `group_id`, `tab_ids`, `title`, `color`, `window_id`; needs the `tabGroups` permission. |
| `tab_duplicate` | tabs | extension | `tab_id` | `duplicated`, `source_tab_id`, `tab`. |
| `window_list` | tabs | extension | `include_tabs` true, `max_windows` 1-50 (20) | `count`, `truncated`, `windows`; each window's tabs are capped at 200. |
| `window_new` | tabs | extension | `url?` (4096 chars), `incognito` false, `focused` true, `width`/`height` 200-10000 | `created`, `window_id`, `tab_id`, `url`, `incognito`, `width`, `height`; only http, https and `about:blank` URLs. |
| `cookie_set` | storage | both | `name` 1-256, `value` 0-4096, optional `url`, `domain`, `path`, `secure`, `http_only`, `same_site`, `expires` | `set`, cookie attributes, `tab_id?`, `url`; never echoes the value. |
| `cookie_clear` | storage | both | `url?` or `domain?` (exactly one), `name?` | `cleared`, the selector, and `names` capped at 100. |
| `storage_get` | storage | both | `area` local or session (local), `keys` 1-100 | `area`, `origin`, `count`, `truncated`, `entries[{key,value}]`; the recorded step and a guardrails redaction replace each value. |
| `storage_set` | storage | both | `area`, `entries` 1-100 string values | `area`, `origin`, `set`, `keys`; never echoes values; total 64 KiB. |
| `storage_clear` | storage | both | `area`, `keys` up to 100 | `area`, `origin`, `cleared`. |
| `health` | diagnostics | daemon | none | `status`, `version`, `uptime_ms`, `started_at`, browser counts, `channel`, `database`, `guardrails` and `tools:{count:62,categories:10}`; no paths, URLs or secrets. |
| `version` | diagnostics | daemon | none | `name`, `version`, `tools:62`, `categories:10`, `protocol_version:1`, `schema_snapshot:"v1.7.0"`. |

`page_pdf` is finalized by the daemon. It decodes the PDF, enforces a 32 MiB decoded-size cap (33,554,432 bytes), writes a unique file under `<home>/pdf/` named `page-<unix-millis>-<uuid8>.pdf` with create-new semantics, and returns metadata only. It is read-only for guardrails and excluded from `browser_batch`; recording strips the raw `data` field and replay skips the step with `page_pdf_not_replayable`.

`cookie_set` and `storage_set` never echo secret values. Recording replaces `cookie_set` `value`, every `storage_set` entry value and every `storage_get` result value with `[redacted]`; guardrails `redact_secrets` independently redacts `storage_get` values and adds `redacted: true`. The storage category is deliberately not batchable.

The extension handles all 16 new browser tools, including the four extension-only tab and window tools, with the existing debugger channel plus the new `tabGroups` permission. `health` and `version` are daemon-local only: they are not extension handlers and not CDP browser-facing tools.

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

`browser_batch` cannot call itself, `browser_channel`, `metrics`, `logs_tail`, `page_pdf`, `tab_group`, `tab_duplicate`, `window_new`, `health`, `version`, any session tool or `tool_help`. It may call the 42 tools in the page, input, tabs, network, files, cdp and diagnostics categories except those ten; that includes `page_links`, `page_forms`, `page_accessibility`, `browser_drag`, `browser_focus`, `browser_highlight`, `window_list`, `browser_list`, `browser_console` and `browser_screenshot`. The storage category is not batchable, and sessions and `help` were already excluded. A `browser_screenshot` step must set `inline: false`; `inline: true` is rejected before dispatch with typed `BAD_ARGS`. The allowlist is enforced before execution; a disallowed name returns typed `BAD_ARGS`.

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
| `{ "category": "page" }` | `{ "category": "page", "count": 15, "tools": [{ "name", "title", "summary" }] }` |
| `{ "name": "browser_batch" }` | One full entry: `name`, `title`, `category`, `summary`, `description`, `inputSchema`, `annotations`. |

Unknown names and unknown categories return typed `BAD_ARGS`. `tool_help` is daemon-local and always available, including when no browser is connected.

## 8. Channels and availability

`browser_channel` reports and changes the active mode:

- `auto` (default) prefers a connected CDP endpoint; it falls back to the extension only when the CDP route reports `Unsupported`.
- `cdp` requires CDP and never falls back. 1.7.0 supports all 46 CDP browser-facing tools over forced CDP, including the eight CDP-only tools and the 12 new browser tools that CDP implements.
- `extension` ignores CDP. It supports 42 browser-facing tools and rejects the eight CDP-only tools with typed `CAPABILITY_UNAVAILABLE`.

The 12 daemon-local tools work in every mode. `browser_channel` changes only the running daemon; the next start uses `--channel` or `CORVUS_CHANNEL`.

| Availability group | Count | Tools |
|---|---:|---|
| Both channels | 38 | page 12, input 10, tabs 4, network 4, files 3, plus `browser_screenshot` |
| CDP only | 8 | `browser_dialog`, `browser_emulate`, `browser_cookies`, `browser_console`, `cdp_status`, `cdp_targets`, `cdp_send`, `cdp_events` |
| Extension only | 4 | `tab_group`, `tab_duplicate`, `window_list`, `window_new` |
| Daemon local | 12 | `browser_list`, `browser_channel`, `tool_help`, `browser_batch`, four session tools, `metrics`, `logs_tail`, `health`, `version` |

The extension remains the zero-setup path. A forced `cdp` process can drive every browser-facing tool without an extension, but the browser must be launched with remote debugging and a dedicated profile. See [07-cdp.md](07-cdp.md) for endpoint setup, routing and security.

## 9. Errors and retries

Every failure carries a typed code. The common ones for tool callers:

| Code | Meaning | Retry |
|---|---|---|
| `BAD_ARGS` | Invalid argument, unknown tool/category, cross-channel reference, or a disallowed batch step. Details usually carry `hint` or the rejected field. | No; fix the request. |
| `NOT_FOUND` | The requested target, session, tab or dialog does not exist. `browser_dialog` uses `no dialog is showing` when nothing is open. | After the state changes. |
| `CAPABILITY_UNAVAILABLE` | The active channel or browser cannot serve this call, for example a CDP-only tool under forced `extension`, or forced `cdp` with no endpoint. | Change channel or reconnect. |
| `POLICY_DENIED` | The daemon guardrails policy denied the tool call or navigation. Details carry `reason`, the tool or URL, and `policy`. | No; change the request or the policy. |
| `BROWSER_GONE` | The browser or its CDP connection disappeared while a call was pending. | Re-list tabs or targets, then retry. |
| `TIMEOUT` | A bounded wait or command exceeded its deadline. | Retry only when the operation is idempotent. |

`auto` does not replay a failed CDP action over the extension. Only an `Unsupported` result is eligible for fallback. Every other transport or protocol error is returned to the caller.

## 10. Measured size and version

The release-candidate benchmark measured compact output against the JSON compatibility path:

| Measure | Compact text | JSON | Reduction |
|---|---:|---:|---:|
| Mean snapshot payload | 364.83 B / 91.21 tokens | 2,077.33 B / 519.33 tokens | 82.44% smaller |
| Mean tool result payload | 646.33 B | 2,077.33 B | 68.89% smaller |

Token counts use the benchmark's tokenizer estimate. The 1.5.0 QA report measures the default brief `tools/list` at 20,493 B (array 20,449 B) for 43 tools and full detail at 37,372 B (array 37,328 B); the release notes cite that report. The 1.6.0 report measures the 43-tool brief at 20,493 B and full at 37,496 B. The 62-tool 1.7.0 catalogue measured a 31,367 B raw brief body (31,368 B with the trailing newline) and a 52,328 B raw full body (52,329 B with the newline); the brief body is within the 32,000-byte brief budget.

Version: 1.6.0 is the current published release, released on 2026-11-15 from tag `v1.6.0`; 1.5.0 is the previous release. QA acceptance PASS is recorded in [v1.6.0.md](../tests/acceptance/v1.6.0.md), and the release is published as GitHub Release 446071476 with main CI run 43716832915 and release workflow 43717236667. The working tree provides 62 tools in ten categories, including the new `storage` category and 18 new tools; 1.7.0 QA acceptance and local Windows packaging passed on the re-frozen 62-tool tree (263 Rust tests on 25 result lines, 137 conformance checks, 91 Vitest tests in 13 files, 3 Playwright E2E tests and 160/160 local packaging checks); no 1.7.0 CI run, published package or published asset exists, so every CI and published-release row is Pending and the working tree remains unreleased.