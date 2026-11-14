# Corvus browser extension

A zero-UI Manifest V3 extension that bridges the local Corvus daemon and this
browser. It loads no remote code, uses no bundler and has no build step: every
file in this directory is shipped as is.

The only visible surface is one popup with a single debugger switch and one
connection status dot.

## Files

| File | Role |
| --- | --- |
| manifest.json | MV3 manifest: permissions, content script, service worker, popup. |
| background.js | Service worker: protocol envelopes, command routing, tab resolution, content script injection, debugger channel, watchdog alarm. |
| offscreen.html / offscreen.js | Offscreen document: owns the only WebSocket, reconnects with backoff, relays frames to the service worker. |
| content.js | Content script: navigate, snapshot, click and type inside the page. |
| popup.html / popup.js | Single switch for the debugger channel and one status dot. |

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
   validates the envelope, resolves the target tab, injects content.js when the
   tab has no listener yet, forwards the command, and answers with a response
   envelope that carries the same id.
5. The content script answers exactly once per command with either
   {ok:true, result} or {ok:false, error}. The service worker wraps that answer
   in the response envelope.
6. The offscreen document relays the response to the daemon over the socket.

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
object sits in the top level error field; it is never a bare string. This
matches crates/corvus-proto/src/envelope.rs, where Envelope::response_ok puts the
result in args and Envelope::response_err sets error.

Hello and hello_ack:

~~~json
{ "v": 1, "id": "01J8Z9K3V7Q2M4N6P8R0T2W4X6", "kind": "hello",
  "browser": "edge:7f3a91c0d2b4e6f8",
  "args": { "profile": "7f3a91c0d2b4e6f8", "version": "0.1.0",
            "capabilities": ["dom", "scripting"] } }
~~~

~~~json
{ "v": 1, "id": "01J8Z9K3V7Q2M4N6P8R0T2W4X6", "kind": "hello_ack",
  "browser": "edge:default",
  "args": { "daemon": "corvus", "version": "0.1.0", "protocol": 1,
            "capabilities": ["dom", "scripting"],
            "limits": { "max_timeout_ms": 120000, "default_timeout_ms": 15000,
                        "max_inflight": 64, "max_snapshot_nodes": 20000 } } }
~~~

The browser token is browser name plus a random profile token persisted in
chrome.storage.local. Extension APIs cannot read the real profile name.

## Tool payloads

The frozen M1 tool names and argument shapes are:

| Tool | Arguments | Success result |
| --- | --- | --- |
| browser_navigate | { "url": string, "tab_id"?: string } | { navigated, loaded, url, previous, same_document, started_at } |
| page_snapshot | { "tab_id"?: string, "mode"?: "interactive" \| "full", "max_nodes"?: 40..3000 } | { url, title, viewport, mode, nodes, ... } |
| browser_click | { "target": string, "tab_id"?: string } | { clicked, ref, selector, tag, role, name, bbox, point, events, url } |
| browser_type | { "target": string, "text": string, "clear"?: boolean, "blur"?: boolean, "tab_id"?: string } | { typed, ref, selector, tag, role, cleared, events, value, ... } |

target is an element reference ("@e12") or a CSS selector. The extension also
accepts the legacy aliases "ref" and "selector" for target; target takes
precedence when more than one is present. mode defaults to "interactive" for
page_snapshot, and clear defaults to true for browser_type. tab_id is optional
for all four tools; when it is absent the active tab of the last focused window
is used.

### browser_navigate

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

### page_snapshot

~~~json
{ "mode": "interactive", "max_nodes": 700 }
~~~

Result: url, title, viewport, mode, node_cap, node_count, element_count, text_count,
ref_count, truncated, and nodes. Each element node is
{kind:"element", ref:"@e12", tag:"button", role:"button", name:"Submit",
bbox:{x,y,w,h}, depth, states, attributes?}. Each text node is
{kind:"text", depth, text}.

mode "interactive" (the default) keeps only actionable elements: buttons, links,
form controls, and elements with an interactive role, tabindex or
contenteditable. Text nodes that surround those elements are kept as context;
the visible text of the actionable element itself is folded into its name. mode
"full" returns the complete compact tree.

The refs come from a per-document element map and are stable while the document
lives: two snapshots in a row return the same @eN for the same element. A ref
from a previous document is stale and answers NOT_FOUND. The node budget is
clamped to 40 through 3000 nodes; the walk stops at the cap and sets
truncated:true. Hidden subtrees are skipped. Roles and names are computed from
ARIA attributes, labels, alt text and visible text, so the node list is the
hybrid accessibility tree plus trimmed DOM.

### browser_click

~~~json
{ "target": "@e12" }
~~~

or a CSS selector:

~~~json
{ "target": "button[type=submit]" }
~~~

The legacy aliases { "ref": "@e12" } and { "selector": "button[type=submit]" }
are also accepted; target wins when both are present.

Result: clicked, ref, selector, tag, role, name, bbox, point, events, url. The handler
scrolls the element into view, checks that the center point hit-tests to the
element or one of its descendants, focuses it, and then dispatches pointerover,
pointerenter, pointermove, pointerdown, mousedown, pointerup, mouseup and a real
element.click(), so native default actions and framework handlers run.

### browser_type

~~~json
{ "target": "@e5", "text": "hello world", "clear": true, "blur": false }

The legacy aliases { "ref": "@e5" } and { "selector": "input[name=q]" } are also
accepted; target wins when both are present.
~~~

Result: typed, ref, selector, tag, role, cleared, events, value (masked for password
fields), value_length, url. The handler focuses the field, writes through the
native HTMLInputElement or HTMLTextAreaElement value setter, then dispatches
beforeinput, an InputEvent with data for input and a change event, so
framework-controlled inputs such as React, Vue and Angular forms update instead
of rolling back. Editable regions use execCommand insertText with a textContent
fallback. clear defaults to true; blur defaults to false.

## Error codes

| Code | When this extension emits it | Retryable |
| --- | --- | --- |
| BAD_ARGS | Missing tool, unknown tool, missing url, target or text, invalid CSS selector, ref not shaped like @e12, oversized payload. | no |
| NOT_FOUND | No element matches the selector, ref is unknown or from an old document, the tab does not exist. | yes, after a new snapshot or page change |
| NOT_INTERACTABLE | Element hidden, disabled, read only, not editable, zero hit area, or covered by another element (details.covering names it). | yes, after scrolling or a new snapshot |
| TIMEOUT | The content script or the tab did not answer inside meta.timeout_ms plus 1.5 s. | yes |
| NAVIGATION_LOST | The page navigated or unloaded before the command answered. | yes, re-resolve on the new page |
| CAPABILITY_UNAVAILABLE | The page cannot be scripted, for example a chrome, edge, about or extension page, or the debugger could not attach. | no until the page or the switch changes |
| BROWSER_GONE | The tab was closed while the command was running. | no |
| TRANSPORT_DOWN | The daemon reports this when the extension socket is gone; the extension cannot send it over a closed socket. | yes, after reconnect |
| INTERNAL | Unexpected bug, native setter failure, click threw. | no |

## Loading unpacked

1. Open chrome://extensions (or edge://extensions, brave://extensions).
2. Enable Developer mode.
3. Choose Load unpacked and select this extension directory.
4. The Corvus icon opens the popup. The dot is gray while disconnected and
   green while the daemon connection is up. The switch controls the debugger
   channel; it is off by default and attach failures are shown as a red dot and
   a message in the popup.

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

Negative checks:

- Click or type an @eN that is not in the current snapshot: NOT_FOUND.
- Click an element hidden behind an overlay or with display none:
  NOT_INTERACTABLE.
- Type into a div that is not editable: NOT_INTERACTABLE.
- Send a command with tool "browser_fly": BAD_ARGS.
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
- The profile token is synthetic because extensions cannot read the profile
  name.
- The debugger channel is attached and detached by the popup switch and is
  advertised in capabilities. All four M1 tools currently run on the DOM
  channel, which is the specified automatic fallback.
- webRequest, downloads and unlimitedStorage are declared as required by the
  architecture but no M1 command uses them yet.
- Commands are not buffered while the socket is down; nothing can be sent over
  a closed socket.
