# Corvus browser extension

A zero-UI Manifest V3 extension that bridges the local Corvus daemon and this
browser. Every file in this directory is shipped as is: there is no bundler, no
npm package, no build step, no remote code and no inline script.

The only visible surface is one popup: a read-only status card with a connection
dot, connection details, and one line noting that the debugger channel is managed
automatically. It has no controls.

## Files

| File | Role |
| --- | --- |
| manifest.json | MV3 manifest: permissions, content script, service worker, popup. |
| background.js | Service worker: protocol envelopes, command routing, tab resolution, content-script injection, the managed debugger channel, watchdog alarm. |
| offscreen.html / offscreen.js | Offscreen document: owns the only WebSocket, reconnects with backoff, relays frames to the service worker. |
| content.js | Content script: page-scoped tools inside the page (snapshot, text, HTML, find, click, type, hover, press, scroll, select, wait probe). |
| popup.html / popup.js | Read-only status card: connection dot and state, browser id, version, capabilities. |
| README.md | This document. |

## Message flow

1. The service worker starts on install, on browser start, and on every worker
   wake-up. It loads the stored state, creates the thirty second alarm
   corvus.watchdog and ensures the offscreen document exists.
2. The offscreen document owns the WebSocket to ws://127.0.0.1:7330. When the
   socket opens it asks the service worker for a hello envelope and sends it.
3. The daemon answers with hello_ack. The service worker stores the assigned
   browser id in chrome.storage.local under corvus.browserId, stores the daemon
   version, and marks the connection as connected. The popup dot turns green.
4. The daemon sends command envelopes. The offscreen document relays them to
   the service worker with chrome.runtime.sendMessage. The service worker
   validates the envelope, resolves the target tab, chooses the execution path,
   and answers with a response envelope that carries the same id.
5. There are four execution paths:
   - Tab tools (tab_list, tab_new, tab_close, tab_switch) use chrome.tabs only
     and never need a content script.
   - browser_eval uses the managed debugger channel and CDP Runtime.evaluate.
   - browser_history back and forward use the debugger channel; reload uses
     chrome.tabs.reload.
   - The other tools are forwarded to content.js. The service worker pings the
     tab first and injects content.js when no listener is present yet.
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

There are eighteen frozen tool names. The service worker rejects any other name
with BAD_ARGS.

Common rules:

- tab_id is an optional string on every tool. When omitted, the active tab of
  the last focused window is used. tab_close and tab_switch require it. A numeric
  string is resolved as a tab id; any other non-empty string is an unknown tab
  and answers NOT_FOUND with details.tab_id.
- Every tab id in a result is a decimal string, matching the tab_id argument.
- target is an element reference ("@e12") or a CSS selector. The legacy aliases
  ref and selector are accepted for target; target wins when both are present.
- Every command resolves. A tool-level failure is a response envelope whose
  args is null and whose top-level error carries one of the nine codes below.
- Page commands run in the top frame only.

### Payload table

| Tool | Arguments | Success result |
| --- | --- | --- |
| browser_navigate | { "url": string, "tab_id"?: string } | { navigated, loaded, url, previous, same_document, started_at } |
| page_snapshot | { "tab_id"?: string, "mode"?: "interactive" \| "full", "max_nodes"?: 40..3000 } | { url, title, viewport, mode, node_cap, node_count, element_count, text_count, ref_count, truncated, nodes } |
| browser_click | { "target": string, "tab_id"?: string } | { clicked, ref, selector, tag, role, name, bbox, point, events, url } |
| browser_type | { "target": string, "text": string, "clear"?: boolean, "blur"?: boolean, "tab_id"?: string } | { typed, ref, selector, tag, role, cleared, events, value, value_length, url } |
| page_text | { "tab_id"?: string, "max_chars"?: 1..200000 } | { url, title, text, truncated } |
| page_html | { "tab_id"?: string, "target"?: string, "max_chars"?: 1..500000 } | { url, html, truncated } |
| page_find | { "query": string, "tab_id"?: string, "role"?: string, "limit"?: 1..100, "regex"?: boolean } | { url, count, matches: [{ ref, tag, role, text }] } |
| browser_hover | { "target": string, "tab_id"?: string } | { hovered: true, url, target: { ref, tag, text? } } |
| browser_press | { "keys": string \| string[], "target"?: string, "tab_id"?: string } | { pressed: true, keys, url } |
| browser_scroll | { "direction": "up" \| "down" \| "left" \| "right", "pages"?: 0..50, "target"?: string, "tab_id"?: string } | { scrolled: true, x, y, url } |
| browser_select | { "target": string, "values": string[], "tab_id"?: string } | { selected, value, url } |
| browser_eval | { "expression": string, "world"?: "main" \| "isolated", "tab_id"?: string } | { ok: true, value } or { ok: false, error: { name, message } } |
| browser_wait | { "tab_id"?: string, "selector"?: string, "text"?: string, "url_contains"?: string, "timeout_ms"?: 100..120000 } | { matched: true, waited_ms, url } |
| browser_history | { "action": "back" \| "forward" \| "reload", "tab_id"?: string } | { action, url, loaded } |
| tab_list | {} | { tabs: [{ tab_id, index, active, url, title, status }] } |
| tab_new | { "url"?: string, "active"?: boolean } | { tab_id, url, loaded, previous_tab_id } |
| tab_close | { "tab_id": string } | { closed: true, tab_id, next_tab_id? } |
| tab_switch | { "tab_id": string } | { tab_id, url, title, active: true } |

Defaults: page_snapshot mode is "interactive" and max_nodes is 700 (clamped to
40 through 3000); browser_type clear is true and blur is false; page_text
max_chars is 100000; page_html max_chars is 200000; page_find limit is 20 and
regex is false; browser_press keys is one chord when a string is given;
browser_scroll pages is 1; browser_eval world is "isolated"; browser_wait
timeout_ms is 10000; tab_new active is true.

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
error page (chrome-error://), the service worker navigates it with
chrome.tabs.update and retries content-script injection once after the load
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

### Tabs

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

## Error codes

| Code | When this extension emits it | Retryable |
| --- | --- | --- |
| BAD_ARGS | Missing tool, unknown tool, missing url, target, text, keys or values, invalid direction, invalid world, invalid action, invalid CSS selector, ref not shaped like @e12, number out of range, malformed regex. | no |
| NOT_FOUND | No element matches the selector, ref is unknown or from an old document, the tab does not exist, the select option value does not exist, or browser_history has no history entry. | yes, after a new snapshot, page change or different target |
| NOT_INTERACTABLE | Element hidden, disabled, read only, not editable, zero hit area, covered by another element (details.covering), or a target outside the viewport. | yes, after scrolling or a new snapshot |
| TIMEOUT | The content script or the tab did not answer inside meta.timeout_ms plus 1.5 s, or browser_wait reached its deadline (details.waited_ms). | yes |
| NAVIGATION_LOST | The page navigated or unloaded before the command answered. | yes, re-resolve on the new page |
| CAPABILITY_UNAVAILABLE | The page cannot be scripted, for example chrome, edge, about or extension pages, or the debugger channel could not attach because DevTools is open. | no until the page changes or DevTools closes |
| BROWSER_GONE | The tab was closed while the command was running or before it could report. | no |
| TRANSPORT_DOWN | The daemon reports this when the extension socket is gone; the extension cannot send it over a closed socket. | yes, after reconnect |
| INTERNAL | Unexpected bug, native setter failure, click threw, or chrome.tabs.create returned no tab. | no |

## Loading unpacked

1. Open chrome://extensions (or edge://extensions, brave://extensions).
2. Enable Developer mode.
3. Choose Load unpacked and select this extension directory.
4. The Corvus icon opens a read-only status card. The dot is gray while
   disconnected and green while the daemon connection is up. The card shows the
   assigned browser id, the extension version, the advertised capabilities and a
   note that the debugger channel is managed automatically; it has no controls.

The extension id is derived from the unpacked directory, so it differs between
machines and browsers. A stable id requires a manifest key; no key material is
shipped in this repository yet.

## Verifying with the daemon

1. Start the Corvus daemon so it listens on ws://127.0.0.1:7330. The port and
   path are fixed in offscreen.js.
2. Reload the extension, open the popup and confirm the dot turns green.
3. Open the service worker console from the extensions page (Inspect views:
   service worker) and look for the boot line, the hello_ack line with the
   assigned browser id, and one command line per request.
4. Open the offscreen console (Inspect views: offscreen.html) and confirm the
   socket state messages.
5. Send browser_navigate, then page_snapshot, then browser_click or
   browser_type through the daemon or its MCP tools. A successful response
   carries the tool result directly in args and has no error field.

Payload checks:

- page_text returns url, title, text and truncated; raise max_chars above the
  document length and confirm truncated false.
- page_html with no target returns the whole documentElement; with a target it
  returns that element's outerHTML.
- page_find returns refs that page_snapshot also knows about; pass one to
  browser_click.
- browser_hover returns hovered true; browser_press "Enter" returns pressed
  true; browser_scroll returns the new x and y; browser_select returns the
  selected option values.
- browser_eval "1+1" returns { ok: true, value: 2 }; browser_eval "throw new
  Error('boom')" returns { ok: false, error: { name: "Error", message: "boom" }
  } without an envelope error.
- browser_wait selector "h1" returns matched true; browser_wait with a missing
  selector and a short timeout_ms returns TIMEOUT with details.waited_ms.
- tab_list lists the open tabs with string tab_id values; tab_new opens one and
  returns its tab_id and previous_tab_id; tab_switch activates it; tab_close
  closes it and reports next_tab_id when it was active.
- browser_history back after a normal navigation returns the previous url with
  loaded true; forward returns the later url.
- browser_eval with a circular object returns value null and value_note
  "unserializable"; tab_close with tab_id "tab-999" returns NOT_FOUND.

Negative checks:

- Click or type an @eN that is not in the current snapshot: NOT_FOUND.
- Click an element hidden behind an overlay or with display none:
  NOT_INTERACTABLE.
- Type into a div that is not editable: NOT_INTERACTABLE.
- Send a command with tool "browser_fly": BAD_ARGS.
- browser_select with a value that no option has: NOT_FOUND.
- browser_scroll with direction "sideways": BAD_ARGS.
- browser_history back on a fresh tab: NOT_FOUND.
- Stop the daemon: the dot turns gray, the watchdog retries every 30 s, and the
  reconnect backoff caps at 8 s. Commands sent while the socket is down never
  reach the extension; the daemon is responsible for answering TRANSPORT_DOWN
  or for replaying unacknowledged idempotency keys after reconnect.
- Close the tab while a command runs: BROWSER_GONE.
- Navigate the page while a command runs: NAVIGATION_LOST or TIMEOUT depending
  on when the tab reports the loss.

## Known gaps

- Only the top frame is handled. Content scripts are injected with
  allFrames:false and iframes are reported as nodes but not inspected.
- Shadow roots are not pierced; nodes inside open shadow roots are invisible to
  the snapshot.
- Refs are document scoped. Any navigation invalidates them.
- page_find reports at most one match per element, not every occurrence of the
  text inside one element.
- Interactive snapshots may skip non-actionable ancestors, so depth values can
  jump where the tree was filtered.
- browser_eval does not support top-level await in expression; wrap asynchronous
  work in an async IIFE.
- The profile token is synthetic because extensions cannot read the profile
  name.
- The debugger channel is managed automatically: it is always advertised in
  capabilities, attached lazily by the first command that needs CDP and detached
  after thirty seconds idle, on tab close and on reconnect. browser_eval and
  browser_history back and forward use it; the other tools run on the tabs,
  scripting and content-script channels. A tab owned by DevTools answers
  CAPABILITY_UNAVAILABLE with a reason.
- webRequest, downloads and unlimitedStorage are declared as required by the
  architecture but no command uses them yet.
- Commands are not buffered while the socket is down; nothing can be sent over
  a closed socket.
- This repository has no DOM test harness and the available branded browser
  ignores command-line extension loading, so automated checks here are syntax
  checks plus protocol tests; live checks use a manually loaded unpacked
  extension.
