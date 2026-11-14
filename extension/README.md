# Corvus browser extension

A Manifest V3 extension that bridges the local Corvus daemon and this browser.
Every file in this directory is shipped as is: there is no bundler, no npm
package, no build step, no remote code and no inline script.

The only visible surface is one popup: a read-only status card with a connection
dot, connection details, and one line noting that the debugger channel is managed
automatically. It has no controls.

## Files

| File | Role |
| --- | --- |
| manifest.json | MV3 manifest: permissions, content script, service worker, popup. |
| background.js | Service worker: protocol envelopes, command routing, tab resolution, content-script injection, network capture, downloads, uploads, the managed debugger channel and the watchdog alarm. |
| offscreen.html / offscreen.js | Offscreen document: owns the only WebSocket, reconnects with backoff, relays frames to the service worker. |
| content.js | Content script: page-scoped tools inside the page (snapshot, text, HTML, find, click, type, hover, press, scroll, select, wait probes, upload target resolution). |
| popup.html / popup.js | Read-only status card: connection dot and state, browser id, version, capabilities. |
| README.md | This document. |

## Message flow

1. The service worker starts on install, on browser start and on every worker
   wake-up. It loads the stored state, creates the thirty second alarm
   corvus.watchdog and ensures the offscreen document exists.
2. The offscreen document owns the WebSocket to ws://127.0.0.1:7330. When the
   socket opens it asks the service worker for a hello envelope and sends it.
3. The daemon answers with hello_ack. The service worker stores the assigned
   browser id in chrome.storage.local under corvus.browserId, stores the daemon
   version, and marks the connection as connected. The popup dot turns green.
4. The daemon routes a command envelope to this browser. The offscreen document
   relays it to the service worker with chrome.runtime.sendMessage. The service
   worker validates the envelope, validates the optional browser_id, resolves the
   target tab, chooses the execution path, and answers with a response envelope
   that carries the same id.
5. There are six execution paths:
   - Tab tools (tab_list, tab_new, tab_close, tab_switch) use chrome.tabs only
     and never need a content script.
   - browser_history reload uses chrome.tabs.reload. back and forward use the
     managed debugger channel.
   - Network tools (net_capture, net_intercept) and upload_file use the managed
     debugger channel and CDP. net_log and net_clear read the in-memory capture
     rings.
   - download_list and download_wait use chrome.downloads only.
   - browser_eval uses the managed debugger channel and CDP Runtime.evaluate.
   - The remaining tools are forwarded to content.js. The service worker pings
     the tab first and injects content.js when no listener is present yet.
6. The content script answers exactly once per command with either
   {ok:true, result} or {ok:false, error}. The service worker wraps that answer
   in the response envelope as raw args or as a top-level typed error.
7. The offscreen document relays the response to the daemon over the socket.

Resilience rules:

- Reconnect uses exponential backoff with jitter: 250 ms, 500 ms, 1000 ms and
  so on up to a cap of 8 s, then stays at the cap. A window online event forces
  an immediate retry and resets the backoff.
- The alarm corvus.watchdog fires every 30 s. It asks the offscreen document for
  the socket status and calls connect when the socket is not connected, so a
  dead socket is restarted even when no commands arrive.
- The offscreen document sends a keepalive message to the service worker every
  20 s to keep the worker alive while the daemon connection is idle.
- The offscreen document pings the daemon after 45 s of silence and treats 180 s
  of silence as a dead socket, then reconnects.
- A daemon ping is answered with a pong that echoes the id. The service worker
  answers a relayed ping the same way.
- Every command carries meta.idempotency_key. The service worker keeps the last
  200 responses keyed by that value and replays the cached response when the
  daemon sends the same key again after a reconnect, instead of executing the
  action twice.
- The debugger channel is only attached when a CDP-backed command needs it and
  is detached after thirty seconds without CDP activity, when its tab closes and
  on every reconnect. A network capture therefore ends when its debugger session
  ends; the entries stay available to net_log until net_clear or the tab closes.

## Envelope

All frames use protocol version 1 and the envelope fields from the architecture
document.

Command from the daemon:

~~~json
{
  "v": 1,
  "id": "01J8Z9K3V7Q2M4N6P8R0T2W4X6",
  "kind": "command",
  "browser": "edge:default",
  "session": "s-7f3a",
  "tool": "browser_click",
  "args": { "target": "@e12" },
  "meta": { "timeout_ms": 5000, "idempotency_key": "01J8Z9M1A2", "trace_id": "tr-44" }
}
~~~

Successful response, same id:

~~~json
{
  "v": 1,
  "id": "01J8Z9K3V7Q2M4N6P8R0T2W4X6",
  "kind": "response",
  "browser": "edge:7f3a91c0d2b4e6f8",
  "session": "s-7f3a",
  "tool": "browser_click",
  "args": { "clicked": true, "ref": "@e12", "selector": null, "tag": "button", "role": "button" }
}
~~~

Failed response, same id:

~~~json
{
  "v": 1,
  "id": "01J8Z9K3V7Q2M4N6P8R0T2W4X6",
  "kind": "response",
  "tool": "browser_click",
  "args": null,
  "error": { "code": "NOT_FOUND", "message": "no element matches the selector", "retryable": true }
}
~~~

The envelope carries a response in one of two shapes. On success args is the raw
tool result, with no wrapper and no extra fields, so the daemon can hand it
straight to MCP structured output. On failure args is null and the typed error
object sits in the top level error field; it is never a bare string.

Hello and hello_ack:

~~~json
{ "v": 1, "id": "01J8Z9K3V7Q2M4N6P8R0T2W4X6", "kind": "hello",
  "browser": "edge:7f3a91c0d2b4e6f8",
  "args": { "profile": "7f3a91c0d2b4e6f8", "version": "0.1.0",
            "capabilities": ["dom", "scripting", "debugger"] } }
~~~

~~~json
{ "v": 1, "id": "01J8Z9K3V7Q2M4N6P8R0T2W4X6", "kind": "hello_ack",
  "browser": "edge:default",
  "args": { "daemon": "corvus", "version": "0.1.0", "protocol": 1,
            "capabilities": ["dom", "scripting", "debugger"],
            "limits": { "max_timeout_ms": 125000, "default_timeout_ms": 60000,
                        "max_inflight": 64, "max_snapshot_nodes": 20000 } } }
~~~

The advertised capabilities are always dom, scripting and debugger. The debugger
name is part of the hello even before any session exists; the channel itself is
attached lazily by the first CDP-backed command and detached after thirty seconds
of inactivity, when its tab closes and on every reconnect. The browser token is
browser name plus a random profile token persisted in chrome.storage.local.
Extension APIs cannot read the real profile name.

## Tool contract

There are twenty-six tool names in the M2 surface. Twenty-five are handled by
this extension; browser_list is answered by the daemon transport registry and
never reaches the extension. The service worker rejects any other name with
BAD_ARGS.

Common rules:

- tab_id is an optional string on the page tools, upload_file, browser_history
  and the network tools. When omitted, the active tab of the last focused window
  is used, except that net_log and net_clear cover every captured tab of this
  browser. tab_close and tab_switch require tab_id. A numeric string is resolved
  as a tab id; any other non-empty string is an unknown tab and answers NOT_FOUND
  with details.tab_id.
- Every tab id in a result is a decimal string, matching the tab_id argument.
- browser_id is an optional string on every tool that routes to a browser. The
  daemon resolves it first: omitted with one connected browser selects that
  browser, omitted with none is TRANSPORT_DOWN, omitted with two or more is
  BAD_ARGS with details.reason "multiple_browsers", an unknown value is
  TRANSPORT_DOWN with details.browser_id, and an empty or non-string value is
  BAD_ARGS. When browser_id is present and differs from this browser's assigned
  id, the extension also answers TRANSPORT_DOWN with details.browser_id.
  browser_list takes no arguments and is answered by the daemon.
- target is an element reference ("@e12") or a CSS selector. The legacy aliases
  ref and selector are accepted for target; target wins when both are present.
  A ref comes from the latest page_snapshot or page_find in that tab and is
  stable while the document lives.
- Every command resolves. A tool-level failure is a response envelope whose
  args is null and whose top-level error carries one of the nine codes below.
- Page commands run in the top frame only.
- meta.timeout_ms defaults to 60000 and is capped at 125000. download_wait uses
  the larger of that value and its own timeout_ms plus 5 s, so a long download
  wait is not cut short by the daemon default.

### Payload table

| Tool | Arguments | Success result |
| --- | --- | --- |
| browser_navigate | { "url": string, "tab_id"?: string } | { navigated, loaded, url, previous, same_document, started_at } |
| page_snapshot | { "tab_id"?: string, "mode"?: "interactive" \| "full", "max_nodes"?: 40..3000 } | { url, title, viewport, mode, node_cap, node_count, element_count, text_count, ref_count, truncated, nodes } |
| page_text | { "tab_id"?: string, "max_chars"?: 1..200000 } | { url, title, text, truncated } |
| page_html | { "tab_id"?: string, "target"?: string, "max_chars"?: 1..500000 } | { url, html, truncated } |
| page_find | { "query": string, "tab_id"?: string, "role"?: string, "limit"?: 1..100, "regex"?: boolean } | { url, count, matches: [{ ref, tag, role, text }] } |
| browser_click | { "target": string, "tab_id"?: string } | { clicked, ref, selector, tag, role, name, bbox, point, events, url } |
| browser_type | { "target": string, "text": string, "clear"?: boolean, "blur"?: boolean, "tab_id"?: string } | { typed, ref, selector, tag, role, cleared, events, value, value_length, url } |
| browser_hover | { "target": string, "tab_id"?: string } | { hovered: true, url, target: { ref, tag, text? } } |
| browser_press | { "keys": string \| string[], "target"?: string, "tab_id"?: string } | { pressed: true, keys, url } |
| browser_scroll | { "direction": "up" \| "down" \| "left" \| "right", "pages"?: 0..50, "target"?: string, "tab_id"?: string } | { scrolled: true, x, y, url } |
| browser_select | { "target": string, "values": string[], "tab_id"?: string } | { selected, value, url } |
| browser_eval | { "expression": string, "world"?: "main" \| "isolated", "tab_id"?: string } | { ok: true, value } or { ok: false, error: { name, message } } |
| browser_wait | { "tab_id"?: string, "selector"?: string, "text"?: string, "url_contains"?: string, "timeout_ms"?: 100..120000 } | { matched: true, waited_ms, url } |
| browser_history | { "action": "back" \| "forward" \| "reload", "tab_id"?: string } | { action, url, loaded } |
| net_capture | { "action": "start" \| "stop" \| "status", "tab_id"?: string } | start: { capturing: true, tab_id, started_at, capacity }; stop: { capturing: false, tab_id, entries }; status: { capturing, tab_id, started_at?, entries } |
| net_log | { "tab_id"?: string, "since_seq"?: number, "limit"?: 1..1000, "url_contains"?: string, "method"?: string, "resource_type"?: string, "status_min"?: 0..999, "status_max"?: 0..999, "include_headers"?: boolean } | { entries, next_seq, capturing } |
| net_clear | { "tab_id"?: string } | { cleared } |
| net_intercept | { "action": "set" \| "clear", "tab_id"?: string, "rules"?: object[] } | { ok: true, scope: "tab" \| "browser", tab_id?, rules, applied } |
| download_list | { "limit"?: 1..200 } | { downloads: [{ id, filename, url, state, bytes_received, total_bytes, danger, exists, started_at, tab_id? }] } |
| download_wait | { "id"?: number \| string, "timeout_ms"?: 100..120000 } | { id, state, filename, url, bytes_received, total_bytes } |
| upload_file | { "target": string, "files": string[1..10], "tab_id"?: string } | { uploaded: true, files, target: { ref?, tag, text }, input: { accept?, multiple } } |
| tab_list | {} | { tabs: [{ tab_id, index, active, url, title, status }] } |
| tab_new | { "url"?: string, "active"?: boolean } | { tab_id, url, loaded, previous_tab_id } |
| tab_close | { "tab_id": string } | { closed: true, tab_id, next_tab_id? } |
| tab_switch | { "tab_id": string } | { tab_id, url, title, active: true } |
| browser_list | {} | { browsers: [{ browser_id, profile, version, capabilities, connected_at_ms }] } |

Defaults: page_snapshot mode is "interactive" and max_nodes is 700 (clamped to
40 through 3000); browser_type clear is true and blur is false; page_text
max_chars is 100000; page_html max_chars is 200000; page_find limit is 20 and
regex is false; browser_press keys is one chord when a string is given;
browser_scroll pages is 1; browser_eval world is "isolated"; browser_wait
timeout_ms is 10000; tab_new active is true; net_log since_seq is 0, limit is
100 and include_headers is false; net_intercept status for a mock rule is 200;
download_list limit is 50; download_wait timeout_ms is 30000.

### Navigation and snapshot

browser_navigate

~~~json
{ "url": "https://github.com/" }
~~~

Result:

~~~json
{ "navigated": true, "loaded": true, "url": "https://github.com/",
  "previous": "about:blank", "same_document": false, "started_at": 1763100000000 }
~~~

The content script answers before it starts the navigation, then sets
location.assign after a short delay so the reply is not lost by the unload. The
service worker then waits on chrome.tabs.onUpdated for status "complete" on the
same tab, under a navigation epoch guard so a redirect or a newer navigate does
not resolve the wrong wait. It reads the final URL back from chrome.tabs.get.

url is the final URL after redirects and loaded is true only when the new
document reached status complete. The wait budget is
min(meta.timeout_ms - 2000 ms, 15000 ms); if it expires the call still succeeds
with loaded false and the current URL, never TIMEOUT. If the tab is showing an
error page (chrome-error:// or edge-error://), the service worker navigates it
with chrome.tabs.update and retries content-script injection once after the load
wait. The URL must be http, https or file.

page_snapshot

~~~json
{ "mode": "interactive", "max_nodes": 700 }
~~~

Result: url, title, viewport {width, height, scrollX, scrollY}, mode, node_cap,
node_count, element_count, text_count, ref_count, truncated, and nodes. Each
element node is
{kind:"element", ref:"@e12", tag:"button", role:"button", name:"Submit",
bbox:{x,y,w,h}, depth, states, attributes?}. Each text node is
{kind:"text", depth, text}.

mode "interactive" (the default) keeps actionable elements: buttons, links,
form controls, and elements with an interactive role, tabindex or
contenteditable. Text nodes that surround those elements are kept as context;
the visible text of the actionable element itself is folded into its name.
mode "full" returns the complete compact tree. The node budget is clamped to
40 through 3000 nodes; the walk stops at the cap and sets truncated:true.
Hidden subtrees are skipped.

The refs come from a per-document element map and are stable while the document
lives: two snapshots in a row return the same @eN for the same element. A ref
from a previous document is stale and answers NOT_FOUND.

### Page content

page_text returns the visible text of the document as innerText (with a
textContent fallback), newline-normalized. max_chars is an integer from 1
through 200000 and defaults to 100000; a value outside that range or a
non-integer is BAD_ARGS. The text is sliced at the limit and truncated is set;
it is never an error for the document to exceed the limit.

page_html returns documentElement.outerHTML, or the outerHTML of target when one
is given. max_chars is an integer from 1 through 500000 and defaults to 200000;
a value outside that range or a non-integer is BAD_ARGS. The HTML is sliced at
the limit and truncated is set.

page_find searches visible text. With regex:false (the default) query is
whitespace-collapsed and matched as a case-insensitive substring. With
regex:true query is compiled as a JavaScript RegExp with the i flag; an invalid
expression is BAD_ARGS. role, when present, is compared case-insensitively with
the computed role. limit is an integer from 1 through 100 and defaults to 20.
The result holds at most one match per element, and refs come from the same
per-document map as page_snapshot, so a ref can be passed straight to
browser_click, browser_hover, browser_type and the other target tools.

### Interaction

browser_click

~~~json
{ "target": "@e12" }
~~~

or a CSS selector:

~~~json
{ "target": "button[type=submit]" }
~~~

Result: clicked, ref, selector, tag, role, name, bbox, point, events, url. The
handler scrolls the element into view, checks that the center point hit-tests to
the element or one of its descendants, focuses it, and then dispatches
pointerover, pointerenter, pointermove, pointerdown, mousedown, pointerup,
mouseup and a real element.click(), so native default actions and framework
handlers run.

browser_type

~~~json
{ "target": "@e5", "text": "hello world", "clear": true, "blur": false }
~~~

Result: typed, ref, selector, tag, role, cleared, events, value (masked for
password fields), value_length, url. The handler focuses the field, writes
through the native HTMLInputElement or HTMLTextAreaElement value setter, then
dispatches beforeinput, an InputEvent with data for input and a change event, so
framework-controlled inputs such as React, Vue and Angular forms update instead
of rolling back. Editable regions use insertText with a textContent fallback.
clear defaults to true; blur defaults to false.

browser_hover dispatches pointerover, pointerenter, pointermove, mouseover,
mouseenter and mousemove at the element center after scrolling it into view. It
returns the element ref, tag and accessible text.

browser_press accepts one chord or an array of chords. A chord is a key or a
modifier prefix plus a key: Control+, Shift+, Alt+ and Meta+ may be combined,
for example "Control+Shift+KeyK". Named keys are Enter, Tab, Escape, Backspace,
Delete, Space, ArrowUp, ArrowDown, ArrowLeft, ArrowRight, PageUp, PageDown, Home
and End. A single printable character, "A", "7" or "+", is also accepted.
Keydown, keypress for printable keys and keyup are dispatched with the correct
modifier flags. target is optional and defaults to the focused element, then
the body. keys in the result echoes the normalized chord strings.

browser_scroll scrolls the window by pages viewport units in the requested
direction, or scrolls target when it is a scroll container. pages is a number
from 0 through 50 and defaults to 1; 0 records the current offsets without
moving. x and y are the resulting scroll offsets.

browser_select sets one or more option values on a select. values must be an
array of strings; a single-select element requires exactly one value and a
multiple select accepts several. Every value must match an existing option
value or the call fails with NOT_FOUND. The handler writes the selected flags
and fires input followed by change. selected lists the resulting option values
and value is the select's value.

### Evaluation, waiting and history

browser_eval evaluates expression through the managed debugger channel with CDP
Runtime.evaluate (awaitPromise true, returnByValue true, userGesture true).
world "isolated" (the default) creates a private world named corvus-eval with
Page.createIsolatedWorld and evaluates there; "main" evaluates in the page world.

~~~json
{ "expression": "document.title" }
~~~

Result:

~~~json
{ "ok": true, "value": "Example Domain" }
~~~

If page code throws, the tool call itself succeeds and args is
{ "ok": false, "error": { "name": "TypeError", "message": "..." } }; the
envelope has no error field. Only tool-level problems such as a restricted page
or a missing tab set the envelope error. If the result cannot be serialized to
JSON, including a circular object, args is
{ "ok": true, "value": null, "value_note": "unserializable" }. If the debugger
channel cannot attach, the envelope error is CAPABILITY_UNAVAILABLE with details
{ tab_id, cause, reason }, never INTERNAL. Top-level await in expression is not
supported; use an async IIFE.

browser_wait accepts at least one of selector, text or url_contains. It polls
every 100 ms. selector is checked with document.querySelector, text is a
case-insensitive substring of the visible text, and url_contains is checked
against chrome.tabs. selector and text need the content script; url_contains
alone can run on a restricted page. timeout_ms is an integer from 100 through
120000 and defaults to 10000. Success is { matched: true, waited_ms, url }. If
the condition is not met by the deadline the command fails with TIMEOUT and
details.waited_ms.

browser_history runs back, forward or reload. Back and forward read the page's
navigation entries with CDP Page.getNavigationHistory and activate the adjacent
entry with Page.navigateToHistoryEntry, so they also work for entries created
before the tool touched the tab; reload uses chrome.tabs.reload. All three wait
for the tab to reach status complete and report the final url and loaded. A back
or forward with no history entry is NOT_FOUND; a closed tab is BROWSER_GONE.

### Network capture

net_capture controls one tab's capture ring.

- action start resolves the tab, attaches the managed debugger channel, enables
  the CDP Network domain and returns capturing true, the tab id, the epoch-ms
  started_at and capacity 2000. Starting an already capturing tab returns the
  current capture without clearing it. Starting a stopped capture starts a fresh
  ring.
- action stop disables Fetch and Network when the debugger is still attached,
  keeps the entries and returns the entry count.
- action status never contacts the debugger; it returns the in-memory capturing
  flag, tab id, started_at when known and the entry count.
- Each tab has its own ring of at most 2000 entries. When the ring is full the
  oldest entry is evicted and its request map entry is removed. A same-tab
  navigation keeps the capture. Closing the tab drops the capture. A debugger
  detach, including the thirty second idle detach and a reconnect, marks the
  capture stopped but leaves its entries for net_log.

net_log reads the rings. with tab_id it reads that tab; without tab_id it merges
every captured tab of this browser. Entries are sorted by seq ascending, filters
are applied first and limit is applied last. since_seq keeps entries with a
larger seq. url_contains is a case-insensitive substring; method is compared
upper-case; resource_type is compared lower-case; status_min and status_max
apply only when the entry has a numeric status. limit is an integer from 1
through 1000 and defaults to 100. next_seq is the last returned seq, or
since_seq when nothing matched. capturing is true when any selected ring is
active.

An entry is:

~~~json
{ "seq": 42, "ts": 1763100000123, "tab_id": "7", "request_id": "1234.5",
  "method": "GET", "url": "https://example.test/app.js", "status": 200,
  "mime": "text/javascript", "resource_type": "Script", "from_cache": false,
  "duration_ms": 18, "error": null, "intercepted": null }
~~~

status and mime are null until a response arrives. error is the CDP error text
for a failed request. intercepted is null or {action, rule_id} for a request
that a rule matched. request_headers and response_headers are present only when
include_headers is true. seq is a global monotonic counter for this extension
instance; net_clear does not reset it.

net_clear empties the selected rings (one tab, or every captured tab when tab_id
is omitted) and returns the number of removed entries. It does not stop a
capture and does not clear intercept rules.

net_intercept manages CDP Fetch rules.

- action set needs rules: an array of 1 through 50 objects. Each rule has a
  unique id of 1 through 64 characters, url_regex, optional method and
  resource_type, and action "block", "mock" or "continue". url_regex is compiled
  as a JavaScript RegExp; an invalid pattern is BAD_ARGS with details.rule_id.
  method is upper-cased and resource_type lower-cased for matching.
- A mock rule also accepts status (an integer from 100 through 599, default
  200), headers (a string-to-string object) and body_base64 (standard base64
  whose decoded size is at most 1 MiB). Those fields are only valid for mock.
  When a mock has a body and no content-type header, text/plain; charset=utf-8
  is added.
- A tab-scoped set (tab_id present) replaces that tab's rules and starts its
  capture if needed. A browser-wide set (tab_id omitted) replaces the browser
  rules and applies to every capturing tab and to every later capture.
- Tab rules are evaluated before browser rules, and the first match in array
  order wins. A request that matches no rule is continued unchanged.
- block uses Fetch.failRequest with BlockedByClient; mock uses
  Fetch.fulfillRequest; continue uses Fetch.continueRequest. If one of those
  calls fails, Corvus records the failure, attempts Fetch.continueRequest as a
  fallback, and leaves the rule set in place.
- action clear removes the tab rules when tab_id is present, or all browser
  rules otherwise, and returns rules [] and applied true. The other scope's
  rules still apply.

The rule list in the result contains only id, action, url_regex, method and
resource_type; mock configuration is stored but not echoed.

### Downloads

download_list reads chrome.downloads.search and returns the newest downloads
first, ordered by start time and then id. limit is an integer from 1 through 200
and defaults to 50. Each item has id (number), filename, url, state,
bytes_received, total_bytes, danger, exists, started_at in epoch milliseconds and
tab_id as a string when the browser recorded a source tab. A download started by
a page this extension never touched may have no tab_id.

download_wait waits for one download to reach complete or interrupted. With id
it waits for that download. Without id it records the ids that exist when the
call starts and waits for the newest download created afterwards. It polls every
100 ms. timeout_ms is an integer from 100 through 120000 and defaults to 30000;
on expiry the command fails with TIMEOUT and details {waited_ms, id?}. The
result is {id, state, filename, url, bytes_received, total_bytes}. Downloads are
profile-wide, not scoped to a tab, and the extension never pauses or prompts for
one.

### File uploads

upload_file attaches local files to an <input type="file"> resolved by target.
files is an array of 1 through 10 absolute paths (Windows drive or UNC paths, or
a path beginning with a slash); a relative path or NUL is BAD_ARGS with
details.path. The target may be an @eN ref from the tab's latest snapshot or a
CSS selector. The input may be hidden: Corvus resolves it in the content script,
finds the same node through CDP DOM.getDocument and DOM.querySelector, confirms
DOM.describeNode reports INPUT with type file, and calls DOM.setFileInputFiles.
Before attaching, Corvus probes every absolute path with a HEAD request to its
file URL, because the browser can attach a non-existent path as an empty File
entry without a CDP error. A missing or unreadable path is BAD_ARGS with
details.path; a target that is not a file input is BAD_ARGS with details.reason
"not_a_file_input".

After the files are attached, Corvus dispatches bubbling input and change events
in the page's main world so framework handlers see the new FileList. The result
is {uploaded: true, files, target: {ref?, tag, text}, input: {accept?,
multiple}}. accept appears only when the input has a non-empty accept attribute;
multiple is always a boolean.

### Tabs and browser selection

tab_list returns every tab, ordered by window id then tab index:
{ tab_id, index, active, url, title, status }. status is the chrome.tabs status
string ("loading", "complete" or "unloaded").

tab_new creates a tab. url is optional and must be http, https or file when
present; when it is omitted the browser new-tab page opens. active defaults to
true. previous_tab_id is the tab that was active before creation, or null. The
result reports loaded true only when the new document reached status complete
inside the command budget; a budget expiry is still a success with loaded false
and the current url.

tab_close requires tab_id. It returns { closed: true, tab_id } and adds
next_tab_id only when the closed tab was the active tab and a neighbouring tab
was activated in the same window.

tab_switch requires tab_id, activates the tab, focuses its window best effort
and returns { tab_id, url, title, active: true }.

browser_list is answered by the daemon, not by this extension. It returns every
connected browser with browser_id, profile, version, capabilities and
connected_at_ms, sorted by connected_at_ms ascending. The daemon uses the same
registry to resolve browser_id for the other tools.

## Error codes

| Code | When this extension emits it | Retryable |
| --- | --- | --- |
| BAD_ARGS | Missing tool, unknown tool, missing url, target, text, keys, values or files, invalid direction, invalid world, invalid action, invalid CSS selector, ref not shaped like @e12, number out of range, malformed regex, a non-file upload target, a relative or unreadable upload path, a malformed browser_id, or an omitted browser_id while several browsers are connected (daemon-side). | no |
| NOT_FOUND | No element matches the selector, ref is unknown or from an old document, the tab does not exist, the select option value does not exist, the upload target disappeared, or browser_history has no history entry. | yes, after a new snapshot, page change or different target |
| NOT_INTERACTABLE | Element hidden, disabled, read only, not editable, zero hit area, covered by another element (details.covering), or a target outside the viewport. | yes, after scrolling or a new snapshot |
| TIMEOUT | The content script or the tab did not answer inside meta.timeout_ms plus 1.5 s, browser_wait reached its deadline, or download_wait reached its deadline (details.waited_ms, details.id?). | yes |
| NAVIGATION_LOST | The page started navigating while a content-script command was running, so its document and refs are gone. | yes, after re-resolving on the new page |
| CAPABILITY_UNAVAILABLE | The managed debugger could not attach (for example another debugger or DevTools is attached), the page is a restricted scheme, or the browser lacks a permission for this operation. details carries tab_id, cause and reason when known. | no, until the blocking condition is removed |
| BROWSER_GONE | The tab or browser was closed during the command. | no |
| TRANSPORT_DOWN | The daemon socket is not connected, or the requested browser_id is not this browser. | yes, after the connection is re-established |
| INTERNAL | An unexpected browser or extension API failure; includes a trace id when the command carried one. | no |

## Loading unpacked

1. Start the Corvus daemon so ws://127.0.0.1:7330 is listening.
2. Open the browser extensions page, enable developer mode and choose "Load
   unpacked". Select this directory, the one containing manifest.json.
3. The extension has no options page and no side panel. Open the toolbar popup
   to see the read-only status card: connection dot and state, browser id,
   extension version, advertised capabilities, and the automatic debugger
   channel note.
4. If the daemon was not running when the extension loaded, start it and wait
   for the watchdog (at most 30 s), or reopen the popup and use the automatic
   reconnect. The status card turns green when hello_ack has been processed.
5. The extension id differs between browsers unless the manifest pins it with a
   key. This directory does not ship key material, so the daemon must not depend
   on a fixed extension id.

## Verifying with the daemon

A full probe loads this directory unpacked, connects to a running daemon and
sends command envelopes through the daemon's MCP surface. A minimal check:

1. Navigate to a test page and call page_snapshot. The result must have nodes
   and refs such as @e1; a second snapshot must keep the same refs.
2. Call net_capture with action start and the returned tab_id. Reload the page
   with browser_history, then call net_log with include_headers true. The
   entries must include the document and its subresources, seq must increase, and
   next_seq must equal the last seq.
3. Call net_intercept with action set, a tab_id and one mock rule such as
   {"id":"m1","url_regex":"/api/","action":"mock","status":201,"body_base64":"b2s="}.
   Request that URL in the page. The response must be 201 with the mocked body,
   and net_log must tag the entry with {action:"mock",rule_id:"m1"}. Clear the
   rule, then repeat with a block rule and confirm the request fails.
4. Call download_list and check the newest item. Trigger a small download, then
   call download_wait and confirm it resolves to complete or interrupted. A
   timeout is a TIMEOUT error whose details include waited_ms.
5. Create a small file in the system temporary directory, navigate to a page
   with an <input type="file">, and call upload_file with its absolute path.
   Confirm the result reports uploaded true and that browser_eval reads
   document.querySelector('input[type=file]').files[0].name as the same file
   name.
6. Negative checks: tab_close with tab_id "tab-999" must be NOT_FOUND with
   details.tab_id; net_intercept with an invalid url_regex must be BAD_ARGS with
   details.rule_id; upload_file on a text input must be BAD_ARGS with
   details.reason "not_a_file_input"; upload_file with a relative path must be
   BAD_ARGS with details.path; a browser_id that is not this browser's id must
   be TRANSPORT_DOWN with details.browser_id.

## Known gaps

- No manifest key is shipped, so the extension id can differ between browsers
  and between unpacked loads. This must be resolved before any daemon-side
  allowlist depends on the id.
- Network capture covers only tabs where net_capture was started. It records
  request and response metadata and optional headers, not response bodies.
  Starting a capture attaches the debugger, so DevTools or another debugger on
  the same tab makes start fail with CAPABILITY_UNAVAILABLE.
- A debugger detach stops a capture. The entries remain readable by net_log and
  are dropped when the tab closes or net_clear runs.
- The webRequest and unlimitedStorage permissions are declared for the browser
  surface but no command uses them yet; network capture and interception are
  implemented through the debugger channel.
- Downloads are profile-wide. download_list exposes a tab id only when the
  browser recorded one. download_wait without an id treats a download as a
  candidate when it is new or started within the five seconds before the call,
  prefers the newest candidate, and never returns one that started more than
  five seconds before the call.
- upload_file validates absolute paths with a HEAD request to the file URL
  before DOM.setFileInputFiles, which can otherwise accept a non-existent path.
  If the browser disables extension access to file URLs, valid paths are
  reported unreadable.
- The extension has no automated browser end-to-end test in this repository.
  Verification is the static syntax and protocol checks plus a live daemon probe
  against an unpacked load.
