[English](M2b.md) | [中文](M2b.zh.md)

# Corvus M2b acceptance playbook

## 1. Purpose

M2b adds eight network, download and upload tools to the 18-tool M2 wave 1
surface, for a frozen surface of 26 tools. This playbook is the manual,
real-browser acceptance path for those tools. The automated conformance suite
proves protocol, argument and routing behavior against stateful mocks; this
playbook proves that a real Chromium browser with the Corvus extension
captures real requests, blocks and mocks them, downloads a real file and
attaches a real file to a file input.

Run the automated suite on an isolated port pair first:

```bash
CARGO=/c/Users/Administrator/.cargo/bin/cargo.exe \
CORVUSD_BIN=./target/debug/corvusd.exe \
CORVUSD_ARGS="serve --ws-port 29330 --http-port 29331" \
CORVUS_MCP_URL=http://127.0.0.1:29331/mcp \
CORVUS_WS_URL=ws://127.0.0.1:29330 \
bash tests/conformance/run.sh
```

The expected result is 108 passed, 0 failed, 0 skipped. Then perform this
playbook on a desktop session and record every request, response, latency and
screenshot in the evidence directory.

## 2. Scope, routing and frozen tool names

The 18 M2 wave 1 tools are unchanged:

| family | tools |
|---|---|
| page content | `page_text`, `page_html`, `page_find` |
| pointer and keyboard | `browser_hover`, `browser_press`, `browser_scroll`, `browser_select` |
| page execution | `browser_eval`, `browser_wait` |
| navigation and tabs | `browser_navigate`, `page_snapshot`, `browser_click`, `browser_type`, `browser_history`, `tab_list`, `tab_new`, `tab_close`, `tab_switch` |

M2b adds eight tools:

| family | tools |
|---|---|
| browser registry | `browser_list` |
| network | `net_capture`, `net_log`, `net_clear`, `net_intercept` |
| downloads | `download_list`, `download_wait` |
| uploads | `upload_file` |

Every tool input schema has an optional string `browser_id`. When it is
omitted and exactly one browser is connected, that browser is used. When it is
omitted and no browser is connected the call is TRANSPORT_DOWN; when two or
more browsers are connected it is BAD_ARGS with
`details.reason="multiple_browsers"` and `details.browser_ids`. An explicit
but unknown id is TRANSPORT_DOWN with `details.browser_id`. `browser_list`
is answered by the daemon registry and never needs an extension round trip.

`tab_id` is a JSON string; a numeric JSON value such as `999` is BAD_ARGS.
An unknown string such as `"tab-999"` is forwarded unchanged and returns
NOT_FOUND with `details.tab_id` in the error text. `target` is an `@eN` ref
from `page_snapshot` or `page_find`, or a CSS selector.

Timeouts: all M2b tools use 60000 ms. `download_wait` uses
`min(args.timeout_ms + 5000, 125000)`; its own default `timeout_ms` is
30000 ms, so the default budget is 35000 ms. Do not invent tool names,
argument keys or error codes.

## 3. Prerequisites

- Windows 10 desktop session with Microsoft Edge installed. The reference run
  used branded Edge 157.
- The repository at `D:/DSH/browser_bridege/corvus`.
- Rust toolchain at `C:/Users/Administrator/.cargo/bin` and the local offline
  registry. Always pass `--offline`.
- Node.js for the fixture server and JSON encoding, curl for MCP calls,
  `base64` and `cygpath` from the MSYS/Git for Windows shell.
- A free MCP port pair. The defaults are WebSocket 7330 and HTTP 7331; this
  playbook uses them, but any free pair works when passed to both sides.
- The extension directory `extension/` with `manifest.json` at its root.
- A writable temporary directory outside the repository for the upload file.

## 4. Build and start the daemon

From an MSYS bash shell in the repository root:

```bash
export PATH="/c/Users/Administrator/.cargo/bin:$PATH"
mkdir -p /tmp/corvus-m2b
export EVID=/tmp/corvus-m2b/$(date +%Y%m%d-%H%M%S)
mkdir -p "$EVID"

cargo build --offline -p corvusd --bin corvusd 2>&1 | tee "$EVID/build.log"
./target/debug/corvusd.exe serve > "$EVID/daemon.log" 2>&1 &
echo $! > "$EVID/daemon.pid"
```

If the default ports are busy, add flags on both sides, for example
`serve --ws-port 29330 --http-port 29331`, and use the matching MCP URL.

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
for path in /m2b /api/json /api/slow /download/report.txt; do
  curl -sS -o /dev/null -w "$path status=%{http_code}\n" "http://127.0.0.1:8808$path"
done
```

The M2b fixture is `http://127.0.0.1:8808/m2b` (file
`tests/acceptance/fixtures/m2b-page.html`). It has title `Corvus M2b Page`,
marker `M2b fixture marker 5b21`, buttons `#fetch-slow` and `#fetch-json`,
output `#net-out`, download link `#download`, and a hidden file input
`#file` accepting `.txt,text/plain`. The fixture also serves `/api/json`
(immediate JSON `{"source":"json","value":7}`) and `/api/slow` (same shape
after 600 ms with `source:"slow"` and `value:42`), plus
`/download/report.txt` as an attachment named `corvus-report.txt`. The M2a
routes `/m2`, `/m2-b`, `/m2-c` and `/m2-csp` remain available.

## 5. Load the extension

1. Open `edge://extensions`.
2. Enable **Developer mode**.
3. Click **Load unpacked** and select the repository `extension/` directory,
   the directory containing `manifest.json`.
4. Confirm the Corvus card is present and has no error badge. Record the actual
   extension id. The reference environment produced
   `pdgemoghkehbjpklbmkmchgfmpfghbl`.
5. Open the service worker console and confirm hello / hello_ack lines and no
   uncaught exception. The popup is status-only; its dot must show connected.
   The debugger and network channels are automatic.
6. Re-read `/health` as `health-after.json` and confirm the browser appears
   with `capabilities` including `dom` and `scripting`.

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
  --data '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"m2b-acceptance","version":"1.0"}}}'

SESSION=$(awk -F': ' 'tolower($1)=="mcp-session-id"{gsub("\r","",$2); print $2}' "$EVID/init.headers")
echo "session=$SESSION"
```

Send `notifications/initialized` and define the helper used by every step:

```bash
curl -sS -o /dev/null -w 'initialized status=%{http_code}\n' \
  -X POST "$MCP" \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -H "MCP-Protocol-Version: $PROTO" \
  ${SESSION:+-H "Mcp-Session-Id: $SESSION"} \
  --data '{"jsonrpc":"2.0","method":"notifications/initialized"}'

call() {
  local name="$1" body="$2"
  curl -sS -o "$EVID/$name.json" -w "$name %{time_total}\n" \
    -X POST "$MCP" \
    -H 'content-type: application/json' \
    -H 'accept: application/json, text/event-stream' \
    -H "MCP-Protocol-Version: $PROTO" \
    ${SESSION:+-H "Mcp-Session-Id: $SESSION"} \
    --data "$body"
}
```

## 7. Baseline: 26 tools and the M2b page

```bash
call tools-list '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'
call browser-list-one '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"browser_list","arguments":{}}}'
call m2b-navigate '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"browser_navigate","arguments":{"url":"http://127.0.0.1:8808/m2b"}}}'
call m2b-snapshot '{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"page_snapshot","arguments":{"mode":"interactive"}}}'
```

Pass criteria: `tools-list` contains exactly the 26 frozen names and every
input schema has an optional string `browser_id`. `browser-list-one` returns
a `browsers` array with one entry containing `browser_id`, `profile`,
`version`, `capabilities` and `connected_at_ms`, sorted by
`connected_at_ms` ascending. The navigate result has `navigated:true`,
`loaded:true` and the `/m2b` URL. The snapshot title is `Corvus M2b Page`
and contains refs for `#fetch-json`, `#fetch-slow` and `#download`; hidden
`#file` may be absent from interactive mode, which is expected.

## 8. Network capture and log

```bash
call capture-start '{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"net_capture","arguments":{"action":"start"}}}'
call fetch-json '{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"page_find","arguments":{"query":"Fetch JSON"}}}'
```

Read a ref from `fetch-json.json` as `FETCH_REF`, replace the literal
`FETCH_REF` placeholder in the command with that value, click it to generate
requests, then inspect the capture:

```bash
call fetch-click '{"jsonrpc":"2.0","id":12,"method":"tools/call","params":{"name":"browser_click","arguments":{"target":"FETCH_REF"}}}'
call fetch-wait '{"jsonrpc":"2.0","id":13,"method":"tools/call","params":{"name":"browser_wait","arguments":{"text":"json:7","timeout_ms":5000}}}'
call capture-status '{"jsonrpc":"2.0","id":14,"method":"tools/call","params":{"name":"net_capture","arguments":{"action":"status"}}}'
call net-log-api '{"jsonrpc":"2.0","id":15,"method":"tools/call","params":{"name":"net_log","arguments":{"url_contains":"/api/","limit":20}}}'
call net-log-headers '{"jsonrpc":"2.0","id":16,"method":"tools/call","params":{"name":"net_log","arguments":{"url_contains":"/api/json","include_headers":true,"limit":5}}}'
```

Pass criteria: `capture-start` returns `capturing:true`, the active
`tab_id`, `started_at` and `capacity:2000`. `capture-status` reports
`capturing:true` and `entries>0` after the request. `net-log-api` returns
entries sorted by `seq` ascending with `seq`, `ts`, `tab_id`,
`request_id`, `method`, `url`, `status`, `mime`, `resource_type`,
`from_cache`, `duration_ms`, `error` and `intercepted`; the `/api/json`
entry has HTTP 200, method `GET`, and a numeric `duration_ms`. Headers appear
only in `net-log-headers` (request and response maps for a completed request),
and `next_seq` equals the last returned `seq`.

Then clear and stop:

```bash
call net-clear '{"jsonrpc":"2.0","id":17,"method":"tools/call","params":{"name":"net_clear","arguments":{}}}'
call net-log-empty '{"jsonrpc":"2.0","id":18,"method":"tools/call","params":{"name":"net_log","arguments":{}}}'
call capture-stop '{"jsonrpc":"2.0","id":19,"method":"tools/call","params":{"name":"net_capture","arguments":{"action":"stop"}}}'
```

`net-clear` returns `cleared` equal to the previous entry count. The next
`net_log` has `entries:[]` and `next_seq:0`. `capture-stop` returns
`capturing:false`, the tab id and the remaining entry count.

## 9. Request interception

Capture must be on for the tab. Set a tab-scoped block rule and click
`#fetch-json`:

```bash
call intercept-block '{"jsonrpc":"2.0","id":20,"method":"tools/call","params":{"name":"net_intercept","arguments":{"action":"set","rules":[{"id":"block-json","action":"block","url_regex":"/api/json"}]}}}'
call intercept-block-click '{"jsonrpc":"2.0","id":21,"method":"tools/call","params":{"name":"browser_click","arguments":{"target":"FETCH_REF"}}}'
call intercept-block-text '{"jsonrpc":"2.0","id":22,"method":"tools/call","params":{"name":"page_text","arguments":{}}}'
```

Pass criteria: `intercept-block` returns `ok:true`, `scope:"tab"`, the
echoed rules and `applied:true`; ids are strings. After the click the page
shows `network failed: ...` in `#net-out` and its `data-net` is
`failed`. `net_log` for that request has `status:null`,
`error:"BlockedByClient"` and `intercepted.action:"block"`; the page itself
stays responsive.

Now replace the rule with a mock. The body is base64 of
`{"ok":true,"source":"mock","value":99}`:

```bash
BODY=$(printf '%s' '{"ok":true,"source":"mock","value":99}' | base64 -w0)
MOCK_RULE=$(node -e 'console.log(JSON.stringify({action:"set",rules:[{id:"mock-json",action:"mock",url_regex:"/api/json",status:200,headers:{"content-type":"application/json"},body_base64:process.argv[1]}]}))' "$BODY")
call intercept-mock "{\"jsonrpc\":\"2.0\",\"id\":23,\"method\":\"tools/call\",\"params\":{\"name\":\"net_intercept\",\"arguments\":$MOCK_RULE}}"
call intercept-mock-click '{"jsonrpc":"2.0","id":24,"method":"tools/call","params":{"name":"browser_click","arguments":{"target":"FETCH_REF"}}}'
call intercept-mock-text '{"jsonrpc":"2.0","id":25,"method":"tools/call","params":{"name":"page_text","arguments":{}}}'
```

Pass criteria: the set echoes rule `mock-json`; after the click `#net-out`
shows `mock:99` and `data-net="mock"`. `net_log` shows a 200 response with
`intercepted.action:"mock"` and `rule_id:"mock-json"`. Clear the rules:

```bash
call intercept-clear '{"jsonrpc":"2.0","id":26,"method":"tools/call","params":{"name":"net_intercept","arguments":{"action":"clear"}}}'
call intercept-final-click '{"jsonrpc":"2.0","id":27,"method":"tools/call","params":{"name":"browser_click","arguments":{"target":"FETCH_REF"}}}'
```

After clear, the click reaches the real endpoint again and `#net-out` returns
to `json:7`. Rules are first-match-wins; setting replaces the whole scope.

An `action:"set"` response confirms that the rules were installed; it cannot
report a Fetch action that fails later while a request is in flight. If a
request is not blocked or mocked as expected, check the daemon and extension
logs for `intercept action failed`; the extension falls back to continuing the
request. Record this as an S3 observability limitation, not an error-shape
defect.

## 10. Downloads

```bash
call download-list-before '{"jsonrpc":"2.0","id":30,"method":"tools/call","params":{"name":"download_list","arguments":{"limit":20}}}'
call download-click '{"jsonrpc":"2.0","id":31,"method":"tools/call","params":{"name":"browser_click","arguments":{"target":"DOWNLOAD_REF"}}}'
```

Read `DOWNLOAD_REF` from the baseline snapshot, replace the literal
placeholder in the command with that ref, and wait for the newest download:

```bash
call download-wait '{"jsonrpc":"2.0","id":32,"method":"tools/call","params":{"name":"download_wait","arguments":{"timeout_ms":20000}}}'
call download-list-after '{"jsonrpc":"2.0","id":33,"method":"tools/call","params":{"name":"download_list","arguments":{"limit":20}}}'
```

Pass criteria: `download_list` returns newest first with `id`,
`filename`, `url`, `state`, `bytes_received`, `total_bytes`, `danger`,
`exists` and `started_at`. `download_wait` returns the new entry with
`state:"complete"`, `filename:"corvus-report.txt"` (the link's download
attribute), `bytes_received>0` and `total_bytes>0`. The `id` is the real
download id from the list. The MCP schema requires `id` as a string, so pass
the numeric download id as its decimal string (for example `"42"`). Re-run
`download_wait` with that explicit id and confirm it resolves immediately.
A non-decimal value such as `"no-such-download"` is BAD_ARGS. An unknown
numeric id is not NOT_FOUND: it waits and returns TIMEOUT with
`details.waited_ms` and `details.id`. A download that does not finish within
`timeout_ms+5000` is TIMEOUT with `details.waited_ms`.

## 11. Uploads

Create a small text file outside the repository and read its Windows path:

```bash
printf 'corvus upload fixture\n' > "$EVID/upload.txt"
UPLOAD_WIN=$(cygpath -w "$EVID/upload.txt")
echo "$UPLOAD_WIN"
```

Build the JSON body with Node so the backslashes are escaped correctly, then
call the tool against the hidden input by CSS selector:

```bash
UPLOAD_BODY=$(node -e 'console.log(JSON.stringify({target:"#file",files:[process.argv[1]]}))' "$UPLOAD_WIN")
call upload-file "{\"jsonrpc\":\"2.0\",\"id\":40,\"method\":\"tools/call\",\"params\":{\"name\":\"upload_file\",\"arguments\":$UPLOAD_BODY}}"
call upload-files '{"jsonrpc":"2.0","id":41,"method":"tools/call","params":{"name":"browser_eval","arguments":{"expression":"Array.from(document.querySelector(\"#file\").files).map(f=>f.name)"}}}'
call upload-text '{"jsonrpc":"2.0","id":42,"method":"tools/call","params":{"name":"page_text","arguments":{}}}'
```

Pass criteria: `upload-file` returns `uploaded:true`, the absolute path in
`files`, and `target.tag:"input"` with `input.accept` containing
`.txt` and `input.multiple:false`. `upload-files` returns
`{"ok":true,"value":["upload.txt"]}`, proving `input.files` is populated.
`page_text` contains `files:upload.txt`. A target that is not a file input
returns BAD_ARGS with `details.reason:"not_a_file_input"`; a missing path
returns BAD_ARGS with `details.path`. A relative path is BAD_ARGS; the API
requires absolute paths.

## 12. Multi-browser routing

`browser_list` proves the registry. With one browser the single-entry result
is enough; to exercise routing, load the same unpacked extension in a second
browser profile or window so `/health` lists two browsers. Record both
`browser_id` values.

- Omit `browser_id`: BAD_ARGS, `details.reason:"multiple_browsers"`, and
  `details.browser_ids` lists both ids.
- Pass the first `browser_id`: the call succeeds against that browser.
- Pass a made-up id: TRANSPORT_DOWN with `details.browser_id`.
- `browser_list` still answers locally and lists both browsers sorted by
  `connected_at_ms` ascending.

Do not test with two browsers unless the environment really has two; record
the single-browser case as PASS for the registry and SKIP for ambiguity.

## 13. Negative and error-path matrix

Every failure must be HTTP 200 with `result.isError=true` and
`content[0].text` beginning `CODE: message`; details are appended as a
trailing JSON object. A JSON-RPC `error` for a tool-level failure is a defect.

| tool | arguments | expected |
|---|---|---|
| unknown tool `corvus_missing_tool` | any | BAD_ARGS |
| `net_capture` | `{"action":"pause"}` | BAD_ARGS |
| `net_log` | `{"limit":0}` | BAD_ARGS |
| `net_log` | `{"limit":1001}` | BAD_ARGS |
| `net_intercept` | `{"action":"set"}` | BAD_ARGS |
| `net_intercept` | rule with numeric `id`: `1` | BAD_ARGS |
| `net_intercept` | two rules with the same id | BAD_ARGS, `details.rule_id` |
| `net_intercept` | rule `url_regex:"["` | BAD_ARGS, `details.rule_id` |
| `download_list` | `{"limit":201}` | BAD_ARGS |
| `download_wait` | `{"id":"no-such-download"}` | BAD_ARGS (id must be a decimal string) |
| `download_wait` | `{"id":"999999999","timeout_ms":100}` | TIMEOUT, `details.waited_ms` and `details.id` |
| `download_wait` | `{"timeout_ms":100}` with no download starting | TIMEOUT, `details.waited_ms` |
| `upload_file` | `{"target":"#file","files":[]}` | BAD_ARGS |
| `upload_file` | 11 paths | BAD_ARGS |
| `upload_file` | `{"target":"#net-out","files":["C:/Windows/win.ini"]}` | BAD_ARGS, `not_a_file_input` |
| `upload_file` | a path that does not exist | BAD_ARGS, `details.path` |
| `upload_file` | relative path | BAD_ARGS |
| `page_text` | unknown `browser_id` | TRANSPORT_DOWN, `details.browser_id` |
| `page_text` | omitted `browser_id` with two browsers | BAD_ARGS, `multiple_browsers` |
| `tab_switch` | `{"tab_id":"tab-999"}` | NOT_FOUND, `details.tab_id` |
| `tab_switch` | `{"tab_id":999}` | BAD_ARGS (must be a string) |
| extension disabled | any page tool | TRANSPORT_DOWN |

Stopping the extension must not crash the daemon. Re-enable it, confirm
`/health` lists the browser again, and continue.

## 14. Evidence to record

- `build.log`, `daemon.log`, `fixture.log`
- `health-before.json`, `health-after.json`, `init.headers`, `init.json`,
  `tools-list.json`
- `browser-list-one.json` and, when two browsers exist,
  `browser-list-two.json`, `route-ambiguous.json`, `route-unknown.json`
- `capture-start.json`, `capture-status.json`, `capture-stop.json`
- `net-log-api.json`, `net-log-headers.json`, `net-log-empty.json`,
  `net-clear.json`
- `intercept-block.json`, `intercept-block-click.json`,
  `intercept-block-text.json`, `intercept-mock.json`,
  `intercept-mock-text.json`, `intercept-clear.json`
- `download-list-before.json`, `download-click.json`,
  `download-wait.json`, `download-list-after.json`
- `upload-file.json`, `upload-files.json`, `upload-text.json`
- one JSON for every negative call
- screenshots: extension card, popup connected, `/m2b` before interaction,
  `network failed` state, `mock:99` state, upload `files:` state, download
  entry in the browser downloads page
- client-side `curl %{time_total}` lines for every call
- actual extension id, browser id, daemon version and protocol version

A result JSON alone is not enough: it must be traceable to the tool name,
arguments, latency and a screenshot or page text proving the effect.

## 15. Pass criteria per tool

Latency is client-side `curl %{time_total}` on a local page, single run.
Hard fail above 3000 ms for any M2b call on this fixture, except
`download_wait` and `net_capture` which are bounded by their own budget.

| tool | pass criteria | budget |
|---|---|---|
| `browser_list` | array shape, sorted by `connected_at_ms`; zero, one and two browsers correct | <= 1000 ms |
| `net_capture` | start/status/stop shapes; entry count grows; capacity 2000 | <= 1000 ms |
| `net_log` | filters, sort, limit, headers gating, `next_seq` correct | <= 1000 ms |
| `net_clear` | returns cleared count; subsequent log is empty | <= 1000 ms |
| `net_intercept` | set/clear shapes; block produces page-level failure; mock produces page text change; invalid rule BAD_ARGS with rule id | <= 1000 ms |
| `download_list` | newest first, shape, limit, numeric ids | <= 2000 ms |
| `download_wait` | resolves complete download; decimal-string id; non-decimal id BAD_ARGS; unknown decimal id TIMEOUT with waited_ms and id | <= timeout + 5000 ms |
| `upload_file` | `input.files` populated via eval; returned accept/multiple; non-file and missing path BAD_ARGS | <= 2000 ms |
| `browser_id` routing | ambiguous BAD_ARGS; unknown TRANSPORT_DOWN; explicit routes correctly | <= 1000 ms |
| error paths | structured code and message; daemon healthy | <= 60000 ms default |

Hard fail for any of: a tool that returns success while the page, capture or
downloads do not change; an unstructured error; a code outside the nine-code
taxonomy; a missing or extra tool; a wrong result type; or a daemon crash.

## 16. Cleanup and troubleshooting

```bash
call intercept-final-clear '{"jsonrpc":"2.0","id":99,"method":"tools/call","params":{"name":"net_intercept","arguments":{"action":"clear"}}}'
call capture-final-stop '{"jsonrpc":"2.0","id":100,"method":"tools/call","params":{"name":"net_capture","arguments":{"action":"stop"}}}'
```

Then stop the daemon and fixture server. Leave the evidence directory in place
and attach it to the report.

| symptom | check |
|---|---|
| MCP connection refused | daemon log; correct `--http-port`; `/health` |
| `browsers` empty | extension loaded, service worker running, hello in console |
| `browser_list` has two entries | use explicit `browser_id` for the rest of the run |
| `net_capture` start fails | browser connected with `dom`; reload the extension |
| `net_log` empty | start capture before navigating or clicking; check `tab_id` |
| block has no effect | rule regex must match the full URL; first match wins; capture on |
| mock response not used | `status` 100..599, valid base64 body, content-type header |
| `download_wait` times out | check browser downloads page and `download_list`; filename may be uniquified |
| `upload_file` BAD_ARGS | absolute Windows path; target is a file input; 1..10 files |
| `upload_file` succeeds but files empty | page may have reset the input; re-read `input.files` immediately |
| tab or browser id errors | ids are JSON strings; never coerce; unknown string is NOT_FOUND |

A failure that survives these checks is a defect. Record the exact request and
response body, tool, arguments, timestamp, latency and screenshot; do not patch
product code during acceptance.
