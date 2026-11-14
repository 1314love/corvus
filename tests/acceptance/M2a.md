[English](M2a.md) | [中文](M2a.zh.md)

# Corvus M2a acceptance playbook

## 1. Purpose

M2 wave 1 adds fourteen tools to the four M1 tools. This playbook is the
manual, real-browser acceptance path for that 18-tool surface. It complements
the automated conformance suite: the suite proves protocol and argument
handling against a stateful mock, while this playbook proves that a real
Chromium browser with the Corvus extension produces the documented results.

Run the automated suite first:

```bash
bash tests/conformance/run.sh
```

Then run this playbook on a desktop session. Record every request, response,
latency and screenshot in the evidence directory.

## 2. Scope and frozen tool names

The four M1 tools are unchanged: `browser_navigate`, `page_snapshot`,
`browser_click`, `browser_type`.

M2 wave 1 adds:

| family | tools |
|---|---|
| page content | `page_text`, `page_html`, `page_find` |
| pointer and keyboard | `browser_hover`, `browser_press`, `browser_scroll`, `browser_select` |
| page execution | `browser_eval`, `browser_wait` |
| navigation | `browser_history` |
| tabs | `tab_list`, `tab_new`, `tab_close`, `tab_switch` |

`tab_id` is optional on page and action tools and means the active tab when it
is omitted. `tab_close` and `tab_switch` require it. A `tab_id` is a JSON
string; a numeric JSON value such as `999` is BAD_ARGS. An unknown string such
as `"tab-999"` is forwarded unchanged and returns NOT_FOUND with
`details.tab_id` in the error text, never BAD_ARGS. `target` is either an `@eN` ref from
`page_snapshot` or `page_find`, or a CSS selector.
Do not invent tool names, argument keys or error codes.

Current timeout policy: 60000 ms default, 125000 ms ceiling.
`browser_eval` uses 30000 ms. `browser_wait` uses
`args.timeout_ms + 5000`, capped at 125000 ms, and its own default
`timeout_ms` is 10000 ms.

## 3. Prerequisites

- Windows 10 desktop session with Microsoft Edge installed. The reference run
  used branded Edge 157.
- The repository at `D:/DSH/browser_bridege/corvus`.
- Rust toolchain available at `C:/Users/Administrator/.cargo/bin` and the
  local offline registry. Always pass `--offline`.
- Node.js for the fixture server, and curl for MCP calls.
- A free MCP port pair. The defaults are WebSocket 7330 and HTTP 7331; this
  playbook uses them but any free pair works when passed to both the daemon and
  the client.
- The extension directory `extension/` with `manifest.json` at its root.

## 4. Build and start the daemon

From a MSYS bash shell in the repository root:

```bash
export PATH="/c/Users/Administrator/.cargo/bin:$PATH"
mkdir -p /tmp/corvus-m2a
export EVID=/tmp/corvus-m2a/$(date +%Y%m%d-%H%M%S)
mkdir -p "$EVID"

cargo build --offline -p corvusd --bin corvusd 2>&1 | tee "$EVID/build.log"
./target/debug/corvusd.exe serve > "$EVID/daemon.log" 2>&1 &
echo $! > "$EVID/daemon.pid"
```

If the default ports are busy, add flags on both sides, for example
`serve --ws-port 17330 --http-port 17331`, and use the matching MCP URL.

Wait for the MCP endpoint and store the health document:

```bash
for i in $(seq 1 50); do
  curl -fsS http://127.0.0.1:7331/health -o "$EVID/health-before.json" && break
  sleep 0.2
done
cat "$EVID/health-before.json"
```

`/health` must report `version`, `protocol` `2025-06-18`, `ws_addr`,
`http_addr` and a `browsers` array.

Start the fixture server in a second shell:

```bash
node tests/acceptance/fixtures/serve.mjs > "$EVID/fixture.log" 2>&1 &
echo $! > "$EVID/fixture.pid"
curl -fsS -o /dev/null -w 'fixture status=%{http_code}\n' http://127.0.0.1:8808/m2
```

The M2 fixture is `http://127.0.0.1:8808/m2` (file
`tests/acceptance/fixtures/m2-page.html`). Three extra documents are used by
the tab, history and CSP checks: `/m2-b`, `/m2-c` and `/m2-csp`. The
`/m2-csp` response carries
`Content-Security-Policy: script-src 'self'; object-src 'none'; base-uri 'none'`,
has title `Corvus CSP Page` and text marker `CSP fixture marker 7f3c`.

## 5. Load the extension

1. Open `edge://extensions`.
2. Enable **Developer mode**.
3. Click **Load unpacked** and select the repository `extension/` directory,
   the directory that contains `manifest.json`.
4. Confirm the Corvus card is present and has no error badge. Record the actual
   extension id. The reference environment produced
   `pdgemoghkehbjpklbmkmchgfmpfghbl`; a different unpacked path normally
   produces a different id.
5. Open the service worker console and confirm hello / hello_ack lines and no
   uncaught exception. The toolbar popup is status-only; its status dot must
   show connected. The debugger channel is automatic, so there is no switch to
   flip.
6. Re-read `/health` and save it as `health-after.json`:

```bash
curl -fsS http://127.0.0.1:7331/health -o "$EVID/health-after.json"
cat "$EVID/health-after.json"
```

The `browsers` array must contain the browser with `capabilities` including
`dom` and `scripting`, for example `["dom","scripting"]`.

## 6. MCP session and helper

Initialize once and keep the raw response:

```bash
MCP=http://127.0.0.1:7331/mcp
PROTO=2025-06-18

curl -sS -D "$EVID/init.headers" -o "$EVID/init.json" \
  -X POST "$MCP" \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -H 'MCP-Protocol-Version: 2025-06-18' \
  --data '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"m2a-acceptance","version":"1.0"}}}'

SESSION=$(awk -F': ' 'tolower($1)=="mcp-session-id"{gsub("\r","",$2); print $2}' "$EVID/init.headers")
echo "session=$SESSION"
```

The response must carry `protocolVersion` `2025-06-18`. If the daemon returns
a session id, keep it for later calls; the current daemon is stateless and may
omit it.

Define the call helper. It saves the response body and prints the client-side
latency in seconds:

```bash
call() {
  local name="$1"
  local body="$2"
  curl -sS -o "$EVID/$name.json" -w "$name %{time_total}\n" \
    -X POST "$MCP" \
    -H 'content-type: application/json' \
    -H 'accept: application/json, text/event-stream' \
    -H "MCP-Protocol-Version: $PROTO" \
    ${SESSION:+-H "Mcp-Session-Id: $SESSION"} \
    --data "$body"
}
```

Send the initialized notification:

```bash
curl -sS -o /dev/null -w 'initialized status=%{http_code}\n' \
  -X POST "$MCP" \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -H 'MCP-Protocol-Version: 2025-06-18' \
  --data '{"jsonrpc":"2.0","method":"notifications/initialized"}'
```

## 7. Baseline: 18 tools, navigation, snapshot

List tools and assert the exact count and names:

```bash
call tools-list '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'
node -e 'const r=JSON.parse(require("fs").readFileSync(process.argv[1],"utf8")); const names=r.result.tools.map(t=>t.name); console.log("count="+names.length); console.log(names.sort().join(","));' "$EVID/tools-list.json"
```

Expected count is 18. Every tool must carry `inputSchema` and all four boolean
annotations. Missing or extra names are a contract defect.

Navigate to the fixture and snapshot:

```bash
call navigate-m2 '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"browser_navigate","arguments":{"url":"http://127.0.0.1:8808/m2"}}}'
call snapshot-m2 '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"page_snapshot","arguments":{"mode":"interactive"}}}'
node -e 'const r=JSON.parse(require("fs").readFileSync(process.argv[1],"utf8")); const s=JSON.parse(r.result.content[0].text); console.log(s.url, s.title, s.node_count); for (const n of s.nodes) if (n.ref && n.tag) console.log(n.ref, n.tag, n.name || "");' "$EVID/snapshot-m2.json"
```

Expected: final URL `http://127.0.0.1:8808/m2`, title
`Corvus M2 Page`, positive `node_count`, and refs for the fixture controls.
Use the actual refs printed by the command in the steps below. The placeholders
`Q_REF`, `GO_REF`, `HOVER_REF`, `KEY_REF`, `PICK_REF`, `SINGLE_REF` and
`FIND_REF` stand for those actual refs.

Timing pitfall: `browser_navigate` normally returns `loaded=true` for this
local page. If an immediate `page_snapshot` still shows the previous document,
wait 500 ms and retry once. A persistent wrong page is a defect; record the
navigate JSON with its `loaded` value and both snapshot timestamps.

## 8. Page content tools

### page_text

```bash
call page-text '{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"page_text","arguments":{}}}'
```

Pass criteria: `url` is the fixture URL, `title` is `Corvus M2 Page`,
`text` contains `Corvus M2` and `M2 acceptance fixture`, and `truncated` is
false. Repeat with `{"max_chars":20}`: the result must be non-empty,
`truncated=true`, and the call must still be a success.

### page_html

```bash
call page-html '{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"page_html","arguments":{"target":"#main"}}}'
```

Pass criteria: `html` starts with `<main id="main">` (or contains it at the
start of the returned fragment), contains `Corvus M2`, and `truncated` is
false. A second call with `{"max_chars":50}` must set `truncated=true`.
A call with no target returns the whole `documentElement`; it is acceptable
for that call to be large.

### page_find and shared refs

```bash
call page-find '{"jsonrpc":"2.0","id":12,"method":"tools/call","params":{"name":"page_find","arguments":{"query":"Find target","limit":5}}}'
```

Pass criteria: `count` is at least 1 and is at most `limit`; each match has
`ref`, `tag`, `role` and `text`; at least one match is the `Find target`
button. Copy its `ref` into `FIND_REF` and click it:

```bash
call find-click '{"jsonrpc":"2.0","id":13,"method":"tools/call","params":{"name":"browser_click","arguments":{"target":"FIND_REF"}}}'
call find-result '{"jsonrpc":"2.0","id":14,"method":"tools/call","params":{"name":"page_text","arguments":{}}}'
```

The click must return `clicked=true`, and the page text must contain
`find target clicked`. This proves that `page_find` refs live in the same
document ref space as `page_snapshot` and are accepted by `browser_click`.

Negative checks: `page_find` with no `query`, `limit` 0 or 101, or an
invalid `regex:true` pattern must return a structured BAD_ARGS error. A query
with no match is a success with `count=0`, not an error.

## 9. Interaction tools

### browser_hover

```bash
call hover '{"jsonrpc":"2.0","id":20,"method":"tools/call","params":{"name":"browser_hover","arguments":{"target":"#hover"}}}'
call hover-text '{"jsonrpc":"2.0","id":21,"method":"tools/call","params":{"name":"page_text","arguments":{}}}'
```

Pass criteria: `hovered=true`, `url` is the fixture URL, `target` contains
`tag:"button"` and preferably `text:"Hover me"`. The follow-up page text must
contain `hover event received`, proving the mouseover handler ran. A missing
`target` is BAD_ARGS.

### browser_press

```bash
call press '{"jsonrpc":"2.0","id":22,"method":"tools/call","params":{"name":"browser_press","arguments":{"keys":["Enter","Tab"],"target":"#keyinput"}}}'
call press-text '{"jsonrpc":"2.0","id":23,"method":"tools/call","params":{"name":"page_text","arguments":{}}}'
```

Pass criteria: `pressed=true`; `keys` echoes `["Enter","Tab"]`; `url`
is the fixture URL. The fixture records keydown events, so page text must
contain `pressed:Enter,Tab`. Also try a chord such as
`{"keys":"Control+A","target":"#keyinput"}`: the result must report the
normalized chord. Empty `keys` is BAD_ARGS.

### browser_scroll

```bash
call scroll-down '{"jsonrpc":"2.0","id":24,"method":"tools/call","params":{"name":"browser_scroll","arguments":{"direction":"down","pages":1}}}'
call scroll-up '{"jsonrpc":"2.0","id":25,"method":"tools/call","params":{"name":"browser_scroll","arguments":{"direction":"up","pages":1}}}'
```

Pass criteria: `scrolled=true`; `x` and `y` are numbers; after scrolling
down on this tall fixture `y` is greater than zero; scrolling up reduces
`y`. `pages:0` reports the current offsets without moving. A direction other
than up, down, left or right is BAD_ARGS.

### browser_select

```bash
call select-multi '{"jsonrpc":"2.0","id":26,"method":"tools/call","params":{"name":"browser_select","arguments":{"target":"#pick","values":["alpha","beta"]}}}'
call select-single '{"jsonrpc":"2.0","id":27,"method":"tools/call","params":{"name":"browser_select","arguments":{"target":"#single","values":["two"]}}}'
```

Pass criteria: the multiple select result has `selected` containing
`alpha` and `beta` and `value` `alpha` (the first selected option); the
single select result has `selected:["two"]` and `value:"two"`. Both return
the fixture `url`. An empty `values` array is BAD_ARGS; a value that no
option has is NOT_FOUND. A single-select element with two values is BAD_ARGS.

## 10. browser_eval, browser_wait and browser_history

### browser_eval

```bash
call eval-title '{"jsonrpc":"2.0","id":30,"method":"tools/call","params":{"name":"browser_eval","arguments":{"expression":"document.title"}}}'
call eval-sum '{"jsonrpc":"2.0","id":31,"method":"tools/call","params":{"name":"browser_eval","arguments":{"expression":"1+1"}}}'
call eval-throw '{"jsonrpc":"2.0","id":32,"method":"tools/call","params":{"name":"browser_eval","arguments":{"expression":"(() => { throw new Error(\"m2 boom\") })()"}}}'
call eval-cyclic '{"jsonrpc":"2.0","id":33,"method":"tools/call","params":{"name":"browser_eval","arguments":{"expression":"(() => { const x = {}; x.self = x; return x; })()"}}}'
```

Pass criteria:

- title result is `{"ok":true,"value":"Corvus M2 Page"}`.
- sum result is `{"ok":true,"value":2}`; `value` keeps its numeric type.
- thrown page error is a successful tool call whose result is
  `{"ok":false,"error":{"name":"Error","message":"m2 boom"}}`; `isError` must
  be false and no envelope `error` field is present.
- cyclic object returns `{"ok":true,"value":null,"value_note":"unserializable"}`.
- `world` defaults to `isolated`; `world:"main"` also returns the title.
  `world:"server"` is BAD_ARGS. The tool timeout is 30000 ms.
- The strict-CSP regression in the next subsection is mandatory for both
  `world` values; an `EvalError` on `/m2-csp` is a hard failure.

### browser_wait

```bash
call wait-reload '{"jsonrpc":"2.0","id":34,"method":"tools/call","params":{"name":"browser_history","arguments":{"action":"reload"}}}'
call wait-late '{"jsonrpc":"2.0","id":35,"method":"tools/call","params":{"name":"browser_wait","arguments":{"selector":"#late","timeout_ms":5000}}}'
call wait-selector '{"jsonrpc":"2.0","id":36,"method":"tools/call","params":{"name":"browser_wait","arguments":{"selector":"#h","timeout_ms":5000}}}'
call wait-text '{"jsonrpc":"2.0","id":37,"method":"tools/call","params":{"name":"browser_wait","arguments":{"text":"M2 acceptance fixture","timeout_ms":5000}}}'
call wait-url '{"jsonrpc":"2.0","id":38,"method":"tools/call","params":{"name":"browser_wait","arguments":{"url_contains":"/m2","timeout_ms":5000}}}'
call wait-timeout '{"jsonrpc":"2.0","id":39,"method":"tools/call","params":{"name":"browser_wait","arguments":{"selector":"#never","timeout_ms":100}}}'
```

Pass criteria: `wait-late` runs immediately after a reload, while `#late` is
still hidden; it must return `matched=true` with `waited_ms > 0` (the fixture
reveals it after about 1200 ms). A zero wait means the reload or the polling
loop is suspect; record it and retry once. Each other happy call returns
`matched=true`, a non-negative `waited_ms`, and the fixture `url`;
`waited_ms` is below the requested timeout. The timeout call returns a structured TIMEOUT error and completes in
roughly 100 ms plus transport overhead. MCP error text starts with
`CODE: message` and appends `ErrorBody.details` as trailing JSON, so record
`details.waited_ms` from that object.
A call with no condition is BAD_ARGS. The effective daemon timeout is
`args.timeout_ms + 5000` capped at 125000 ms, so a 5000 ms wait is bounded by
10000 ms.

### browser_history

```bash
call history-base '{"jsonrpc":"2.0","id":40,"method":"tools/call","params":{"name":"browser_navigate","arguments":{"url":"http://127.0.0.1:8808/m2"}}}'
call navigate-b '{"jsonrpc":"2.0","id":41,"method":"tools/call","params":{"name":"browser_navigate","arguments":{"url":"http://127.0.0.1:8808/m2-b"}}}'
call history-back '{"jsonrpc":"2.0","id":42,"method":"tools/call","params":{"name":"browser_history","arguments":{"action":"back"}}}'
call history-forward '{"jsonrpc":"2.0","id":43,"method":"tools/call","params":{"name":"browser_history","arguments":{"action":"forward"}}}'
call history-reload '{"jsonrpc":"2.0","id":44,"method":"tools/call","params":{"name":"browser_history","arguments":{"action":"reload"}}}'
```

Pass criteria: this sequence must step exactly one history entry at a time.
`history-back` returns `action:"back"`, URL `/m2`, `loaded:true`; it must
not skip to an older page. `history-forward` returns URL `/m2-b`,
`loaded:true`; reload keeps `/m2-b` and returns `loaded:true`. An action
outside back, forward and reload is BAD_ARGS. Back on a fresh tab that has only
one history entry returns a structured NOT_FOUND, not BAD_ARGS; verify this
with `NEW_TAB` in section 11 before switching away.

### bfcache and immediate DOM calls

This sequence is a regression check for back-forward cache restores. The bug
symptom: `browser_history back` restored `/m2` from the bfcache, but the content
script kept its unloading flag set, so `page_text` and `page_snapshot` returned
NAVIGATION_LOST even though `document.readyState` was `complete`. Run every step
back to back; do not insert a sleep or retry the DOM call.

```bash
call bfcache-tab-new '{"jsonrpc":"2.0","id":80,"method":"tools/call","params":{"name":"tab_new","arguments":{"url":"http://127.0.0.1:8808/m2","active":true}}}'
call bfcache-navigate-b '{"jsonrpc":"2.0","id":81,"method":"tools/call","params":{"name":"browser_navigate","arguments":{"url":"http://127.0.0.1:8808/m2-b","tab_id":"BF_TAB"}}}'
call bfcache-back '{"jsonrpc":"2.0","id":82,"method":"tools/call","params":{"name":"browser_history","arguments":{"action":"back","tab_id":"BF_TAB"}}}'
call bfcache-back-text '{"jsonrpc":"2.0","id":83,"method":"tools/call","params":{"name":"page_text","arguments":{"tab_id":"BF_TAB"}}}'
call bfcache-back-snapshot '{"jsonrpc":"2.0","id":84,"method":"tools/call","params":{"name":"page_snapshot","arguments":{"tab_id":"BF_TAB","mode":"interactive"}}}'
call bfcache-forward '{"jsonrpc":"2.0","id":85,"method":"tools/call","params":{"name":"browser_history","arguments":{"action":"forward","tab_id":"BF_TAB"}}}'
call bfcache-forward-text '{"jsonrpc":"2.0","id":86,"method":"tools/call","params":{"name":"page_text","arguments":{"tab_id":"BF_TAB"}}}'
call bfcache-forward-snapshot '{"jsonrpc":"2.0","id":87,"method":"tools/call","params":{"name":"page_snapshot","arguments":{"tab_id":"BF_TAB","mode":"interactive"}}}'
call bfcache-reload '{"jsonrpc":"2.0","id":88,"method":"tools/call","params":{"name":"browser_history","arguments":{"action":"reload","tab_id":"BF_TAB"}}}'
call bfcache-reload-text '{"jsonrpc":"2.0","id":89,"method":"tools/call","params":{"name":"page_text","arguments":{"tab_id":"BF_TAB"}}}'
call bfcache-reload-snapshot '{"jsonrpc":"2.0","id":90,"method":"tools/call","params":{"name":"page_snapshot","arguments":{"tab_id":"BF_TAB","mode":"interactive"}}}'
call bfcache-close '{"jsonrpc":"2.0","id":91,"method":"tools/call","params":{"name":"tab_close","arguments":{"tab_id":"BF_TAB"}}}'
```

Read `BF_TAB` from `bfcache-tab-new.json` and pass it verbatim on every call.

Pass criteria: `bfcache-back` returns `action:"back"`, URL `/m2` and
`loaded:true`; the immediately following `bfcache-back-text` and
`bfcache-back-snapshot` both return `isError:false`, with `/m2` and title
`Corvus M2 Page`. `bfcache-forward` returns `/m2-b`; its two immediate DOM
calls return `isError:false` with `/m2-b` and title `Corvus M2 Page B`.
`bfcache-reload` keeps `/m2-b` with `loaded:true`; its two immediate DOM
calls are also `isError:false`. A NAVIGATION_LOST, any other `isError:true`, or
a snapshot of the previous page on any of the six immediate calls is a hard
failure. This is the stale unloading-flag regression, not load timing: a later
retry, a sleep, or `document.readyState === "complete"` does not make it pass.
Close the tab at the end to keep the section 11 tab state clean.

### browser_eval on a strict-CSP page

Confirm the header before calling the tool:

```bash
curl -sS -D - -o /dev/null http://127.0.0.1:8808/m2-csp | grep -i '^content-security-policy:'
```

It must print
`content-security-policy: script-src 'self'; object-src 'none'; base-uri 'none'`.
The page title is `Corvus CSP Page`; its text marker is `CSP fixture marker 7f3c`.

```bash
call csp-navigate '{"jsonrpc":"2.0","id":60,"method":"tools/call","params":{"name":"browser_navigate","arguments":{"url":"http://127.0.0.1:8808/m2-csp"}}}'
call csp-text '{"jsonrpc":"2.0","id":61,"method":"tools/call","params":{"name":"page_text","arguments":{}}}'
call csp-eval-isolated '{"jsonrpc":"2.0","id":62,"method":"tools/call","params":{"name":"browser_eval","arguments":{"expression":"document.title","world":"isolated"}}}'
call csp-eval-main '{"jsonrpc":"2.0","id":63,"method":"tools/call","params":{"name":"browser_eval","arguments":{"expression":"document.title","world":"main"}}}'
call csp-back '{"jsonrpc":"2.0","id":64,"method":"tools/call","params":{"name":"browser_navigate","arguments":{"url":"http://127.0.0.1:8808/m2"}}}'
```

Pass criteria: `csp-navigate` returns `loaded:true` with the `/m2-csp`
URL and `csp-text` contains `Corvus CSP Page` and `CSP fixture marker 7f3c`.
Both eval calls return `{"ok":true,"value":"Corvus CSP Page"}`. `EvalError`,
`ok:false` or `isError:true` on either call is a hard failure: the automatic
debugger channel must evaluate despite the page's CSP, without disabling CSP
or injecting page script. `csp-back` returns to `/m2` for the tabs section.

## 11. Tabs

```bash
call tabs-before '{"jsonrpc":"2.0","id":50,"method":"tools/call","params":{"name":"tab_list","arguments":{}}}'
call tab-new '{"jsonrpc":"2.0","id":51,"method":"tools/call","params":{"name":"tab_new","arguments":{"url":"http://127.0.0.1:8808/m2-c"}}}'
```

Read `tab_id` from `tab-new.json` as `NEW_TAB`, and read a previous tab id
from `tabs-before.json` as `OLD_TAB`. The ids are JSON strings; copy them
verbatim. Before switching away, call `browser_history` with `action:"back"`
and `tab_id:"NEW_TAB"`; the brand-new tab has only one history entry, so this
must return a structured NOT_FOUND. Save it as `history-fresh-tab.json`.

```bash
call tabs-after-new '{"jsonrpc":"2.0","id":52,"method":"tools/call","params":{"name":"tab_list","arguments":{}}}'
call tab-fresh-history '{"jsonrpc":"2.0","id":56,"method":"tools/call","params":{"name":"browser_history","arguments":{"action":"back","tab_id":"NEW_TAB"}}}'
call tab-switch '{"jsonrpc":"2.0","id":53,"method":"tools/call","params":{"name":"tab_switch","arguments":{"tab_id":"NEW_TAB"}}}'
call tab-close '{"jsonrpc":"2.0","id":54,"method":"tools/call","params":{"name":"tab_close","arguments":{"tab_id":"NEW_TAB"}}}'
call tabs-after-close '{"jsonrpc":"2.0","id":55,"method":"tools/call","params":{"name":"tab_list","arguments":{}}}'
```

Pass criteria: `tab_list` returns `tabs` with string `tab_id`, numeric
`index`, boolean `active`, `url`, `title` and `status`; exactly one tab is
active. `tab_new` returns string `tab_id`, `url`, `loaded` and
`previous_tab_id`; the new tab appears in `tab_list` and becomes active when
`active` is true. Copy ids verbatim, never coerce them to numbers.
`tab_switch` returns the requested `tab_id`, its `url`, `title` and
`active:true`. `tab_close` returns `closed:true` and the `tab_id`;
`next_tab_id` is present only when the active tab was closed and a neighbour
was activated. After closing, `tab_list` no longer contains the tab.
`tab-fresh-history` on the newly opened tab must return structured NOT_FOUND,
proving one-entry history is handled.

Negative checks: missing or non-string `tab_id` on `tab_close` or
`tab_switch` is BAD_ARGS; a numeric JSON `tab_id` such as `999` is BAD_ARGS.
An unknown string id such as `"tab-999"` is forwarded unchanged and returns
NOT_FOUND with `details.tab_id` in `content[0].text`, never BAD_ARGS;
`tab_new` with `active` not a boolean is BAD_ARGS. A closed tab id used later
is NOT_FOUND or BROWSER_GONE, not a bare string.

## 12. Negative and error-path matrix

Every failure must be a structured MCP tool result: HTTP 200 with
`result.isError=true` and `content[0].text` beginning with
`CODE: message`, or the MCP error shape documented for transport failures.
The JSON-RPC layer must not turn tool failures into `error` objects.

| tool | arguments | expected |
|---|---|---|
| unknown tool `corvus_missing_tool` | any | BAD_ARGS |
| `page_text` | `{"max_chars":0}` | BAD_ARGS |
| `page_text` | `{"max_chars":200001}` | BAD_ARGS |
| `page_html` | `{"max_chars":500001}` | BAD_ARGS |
| `page_find` | `{}` | BAD_ARGS |
| `page_find` | `{"query":"x","limit":101}` | BAD_ARGS |
| `page_find` | `{"query":"[","regex":true}` | BAD_ARGS |
| `browser_hover` | `{}` | BAD_ARGS |
| `browser_press` | `{"keys":[]}` | BAD_ARGS |
| `browser_press` | `{"keys":"Hyper+Q"}` | BAD_ARGS |
| `browser_scroll` | `{"direction":"diagonal"}` | BAD_ARGS |
| `browser_select` | `{"target":"#pick","values":[]}` | BAD_ARGS |
| `browser_select` | `{"target":"#pick","values":["missing"]}` | NOT_FOUND |
| `browser_eval` | `{"expression":"1","world":"server"}` | BAD_ARGS |
| `browser_wait` | `{}` | BAD_ARGS |
| `browser_wait` | `{"selector":"#never","timeout_ms":100}` | TIMEOUT; record elapsed and `details.waited_ms` from the trailing JSON |
| `browser_history` | `{"action":"refresh"}` | BAD_ARGS |
| `tab_new` | `{"active":"yes"}` | BAD_ARGS |
| `tab_close` | `{}` | BAD_ARGS |
| `tab_close` | `{"tab_id":"tab-999"}` | NOT_FOUND; error text carries `details.tab_id` `"tab-999"` |
| `tab_switch` | `{}` | BAD_ARGS |
| `tab_switch` | `{"tab_id":"tab-999"}` | NOT_FOUND; error text carries `details.tab_id` `"tab-999"` |
| `tab_switch` | `{"tab_id":999}` | BAD_ARGS (tab_id must be a string) |
| `browser_history` | `{"action":"back"}` on a fresh one-entry tab | NOT_FOUND |
| disabled extension | any page tool | TRANSPORT_DOWN |

Stopping the extension, then issuing `page_text`, must return TRANSPORT_DOWN
and must not crash the daemon. Re-enable the extension and confirm /health
lists the browser again before continuing.

## 13. Evidence to record

Create one directory per run. Keep raw bodies, not only summaries.

- `build.log`, `daemon.log`, `fixture.log`
- `health-before.json`, `health-after.json`
- `init.headers`, `init.json`, `tools-list.json`
- one `<tool>.json` for every happy call and every negative call
- `csp-headers.txt`, `csp-eval-isolated.json`, `csp-eval-main.json`
- `history-step-back.json`, `history-step-forward.json`, `history-fresh-tab.json`
- `bfcache-*.json`: tab-new, navigate-b, back, back-text, back-snapshot,
  forward, forward-text, forward-snapshot, reload, reload-text,
  reload-snapshot, close
- `curl` `time_total` lines from the helper, or a timestamped latency file
- screenshots: extension page, popup connected, fixture before interaction,
  after type/click, after scroll, after tab switch
- the actual extension id, browser id, browser version, daemon version and
  protocol version

A result JSON alone is not enough: it must be traceable to the tool name,
arguments, latency and screenshot or page text that proves the effect.

## 14. Pass criteria per tool

Latency is client-side `curl %{time_total}` on a local page, single run,
browser in the foreground. Hard fail above 3000 ms for any interactive tool on
this fixture; record the distribution for repeated runs.

| tool | pass criteria | budget |
|---|---|---|
| `page_text` | URL/title/text correct; truncation honest | <= 1000 ms |
| `page_html` | target fragment correct; truncation honest | <= 1000 ms |
| `page_find` | count/matches correct; returned ref clicks the element | <= 1000 ms |
| `browser_hover` | `hovered=true`; target tag/text correct; mouseover effect | <= 1000 ms |
| `browser_press` | `pressed=true`; keys echoed; keydown effect | <= 1000 ms |
| `browser_scroll` | `scrolled=true`; offsets change in the requested direction | <= 1000 ms |
| `browser_select` | selected values and value correct | <= 1000 ms |
| `browser_eval` | value type correct; strict-CSP title passes in both worlds; thrown error is `ok=false`; cyclic is unserializable | <= 1000 ms |
| `browser_wait` | matched condition; timeout is structured TIMEOUT (record elapsed) | <= waited + 500 ms |
| `browser_history` | exact one-step back/forward; fresh-tab back is NOT_FOUND; bfcache back/forward/reload each followed immediately by page_text and page_snapshot with `isError:false`; action, URL and loaded correct | <= 2000 ms |
| `tab_list` | all tabs, one active, correct shape | <= 2000 ms |
| `tab_new` | new tab id, URL, previous id, loaded | <= 2000 ms |
| `tab_switch` | requested tab active with URL/title | <= 2000 ms |
| `tab_close` | closed true, id, list reflects removal | <= 2000 ms |
| error paths | structured code and message; daemon healthy | <= 60000 ms default |

Hard fail for any of: a tool that returns success while the page does not
change, an unstructured or bare-string error, a code outside the nine-code
taxonomy, a missing or extra tool name, a wrong result type, or a daemon crash.

## 15. Cleanup and troubleshooting

```bash
kill "$(cat "$EVID/daemon.pid")" 2>/dev/null || true
kill "$(cat "$EVID/fixture.pid")" 2>/dev/null || true
```

Leave the evidence directory in place and attach it to the report.

| symptom | check |
|---|---|
| MCP connection refused | daemon log; correct `--http-port`; `/health` |
| `browsers` empty | extension loaded, service worker running, hello in console |
| all tools CAPABILITY_UNAVAILABLE | /health capabilities missing `dom`; reload extension |
| page_snapshot refs change | stale document or reload; take a fresh snapshot |
| `page_find` returns no ref | query case or visibility; try a simpler substring |
| `browser_press` no effect | focus target; check key grammar and modifier order |
| `browser_wait` times out early | fixture served on port 8808; selector/text is visible |
| `browser_eval` throws TypeError | use a complete expression or an async IIFE |
| tab tools fail | extension tab permission; active window; browser id still connected |

A failure that survives these checks is a defect. Record the exact request
body, response body, tool name, arguments, timestamp, latency and screenshot;
do not patch product code during acceptance.
