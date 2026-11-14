#!/usr/bin/env bash
# Corvus M2 MCP conformance checks.
#
# Starts corvusd, waits for the MCP HTTP endpoint on 127.0.0.1:7331, drives
# initialize / notifications/initialized / tools/list / tools/call with curl,
# and asserts the frozen M2 contract from docs/02-architecture.md section 5.2.
# A stateful mock extension is started when available so positive tool calls
# exercise the complete daemon -> extension -> daemon path.
#
# Coverage: all 18 tools with at least one happy call and one negative per
# family, navigation loaded/url and immediate-snapshot checks, default argument
# normalization in recorded envelopes, and a loaded=false navigate that must
# still be a successful tool call.
#
# Usage:
#   tests/conformance/run.sh [--no-mock] [--keep-tmp]
#
# Environment overrides:
#   CORVUSD_BIN        daemon binary to run (default: target/debug/corvusd[.exe])
#   CORVUSD_ARGS       daemon arguments (default: "serve"; an empty string is retried)
#   CORVUS_MCP_URL     default http://127.0.0.1:7331/mcp
#   CORVUS_WS_URL      default ws://127.0.0.1:7330
#   CARGO              cargo binary (default: cargo, then the known Windows path)
#   EXPECTED_PROTOCOL_VERSION  default 2025-06-18
#   EXPECTED_TOOLS     comma separated, default the 18 M2 tools
#   KEEP_TMP=1         keep logs even on success
#
# Exit codes: 0 all checks passed (skips allowed), 1 one or more checks failed.

set -u -o pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
MCP_URL=${CORVUS_MCP_URL:-http://127.0.0.1:7331/mcp}
WS_URL=${CORVUS_WS_URL:-ws://127.0.0.1:7330}
HEALTH_URL=${CORVUS_HEALTH_URL:-${MCP_URL%/mcp}/health}
EXPECTED_PROTOCOL_VERSION=${EXPECTED_PROTOCOL_VERSION:-2025-06-18}
EXPECTED_TOOLS=${EXPECTED_TOOLS:-browser_navigate,page_snapshot,browser_click,browser_type,page_text,page_html,page_find,browser_hover,browser_press,browser_scroll,browser_select,browser_eval,browser_wait,browser_history,tab_list,tab_new,tab_close,tab_switch}
export EXPECTED_PROTOCOL_VERSION EXPECTED_TOOLS

USE_MOCK=1
KEEP_TMP=${KEEP_TMP:-0}
for arg in "$@"; do
  case "$arg" in
    --no-mock) USE_MOCK=0 ;;
    --keep-tmp) KEEP_TMP=1 ;;
    -h|--help)
      sed -n '2,30p' "$0" | sed 's/^# {0,1}//'
      exit 0
      ;;
    *)
      echo "unknown argument: $arg" >&2
      exit 2
      ;;
  esac
done

if [ -z "${CARGO:-}" ]; then
  if command -v cargo >/dev/null 2>&1; then
    CARGO=$(command -v cargo)
  elif [ -x "C:/Users/Administrator/.cargo/bin/cargo.exe" ]; then
    CARGO="C:/Users/Administrator/.cargo/bin/cargo.exe"
  else
    CARGO=""
  fi
fi
CARGO_BIN=${CARGO:-cargo}

TMP=$(mktemp -d "${TMPDIR:-/tmp}/corvus-conformance.XXXXXX") || {
  echo "cannot create a temporary directory" >&2
  exit 1
}
DAEMON_PID=""
MOCK_PID=""
FAILED=0
PASSED=0
SKIPPED=0
declare -a ROWS=()

record() { # status name detail
  ROWS+=("$1|$2|$3")
  case "$1" in
    PASS) PASSED=$((PASSED + 1)) ;;
    FAIL) FAILED=$((FAILED + 1)) ;;
    SKIP) SKIPPED=$((SKIPPED + 1)) ;;
  esac
}

stop_pid() {
  local pid="${1:-}"
  [ -n "$pid" ] || return 0
  kill "$pid" 2>/dev/null || true
  local i
  for i in 1 2 3 4 5 6 7 8 9 10; do
    kill -0 "$pid" 2>/dev/null || return 0
    sleep 0.1
  done
  if command -v taskkill >/dev/null 2>&1; then
    taskkill //F //T //PID "$pid" >/dev/null 2>&1 || true
  fi
}

cleanup() {
  local code=$?
  trap - EXIT INT TERM
  stop_pid "$MOCK_PID"
  stop_pid "$DAEMON_PID"
  if [ "$FAILED" -gt 0 ] || [ "$KEEP_TMP" = "1" ]; then
    echo ""
    echo "logs kept in: $TMP"
    [ -f "$TMP/daemon.log" ] && { echo "--- daemon.log (tail) ---"; tail -n 20 "$TMP/daemon.log"; }
    [ -f "$TMP/mock.log" ] && { echo "--- mock.log (tail) ---"; tail -n 10 "$TMP/mock.log"; }
  else
    rm -rf "$TMP"
  fi
  exit "$code"
}
trap cleanup EXIT INT TERM

print_table() {
  echo ""
  printf '%-44s %-6s %s\n' "CHECK" "RESULT" "DETAIL"
  printf '%-44s %-6s %s\n' "--------------------------------------------" "------" "------------------------------------------"
  local row status name detail
  for row in "${ROWS[@]}"; do
    IFS='|' read -r status name detail <<< "$row"
    printf '%-44s %-6s %s\n' "$name" "$status" "$detail"
  done
}

finish() {
  print_table
  echo ""
  echo "RESULT: $PASSED passed, $FAILED failed, $SKIPPED skipped"
  if [ "$FAILED" -gt 0 ]; then
    exit 1
  fi
  exit 0
}

need_tool() {
  if ! command -v "$1" >/dev/null 2>&1; then
    record FAIL "prerequisites" "missing required command: $1"
    finish
  fi
}

need_tool curl
need_tool node

if [ "$USE_MOCK" = "1" ] && [ ! -x "$ROOT/tests/mock-extension/target/debug/mock-extension.exe" ] \
   && [ ! -x "$ROOT/tests/mock-extension/target/debug/mock-extension" ]; then
  echo "building mock extension ..."
  if ! "$CARGO_BIN" build --offline --manifest-path "$ROOT/tests/mock-extension/Cargo.toml" >"$TMP/mock-build.log" 2>&1; then
    record FAIL "mock extension builds" "see $TMP/mock-build.log"
    USE_MOCK=0
  else
    record PASS "mock extension builds" "cargo build --offline"
  fi
fi

MOCK_BIN=""
if [ -x "$ROOT/tests/mock-extension/target/debug/mock-extension.exe" ]; then
  MOCK_BIN="$ROOT/tests/mock-extension/target/debug/mock-extension.exe"
elif [ -x "$ROOT/tests/mock-extension/target/debug/mock-extension" ]; then
  MOCK_BIN="$ROOT/tests/mock-extension/target/debug/mock-extension"
fi

# Refuse to test against a daemon we did not start: the result would be ambiguous.
PREEXISTING=$(curl -s -o /dev/null -w '%{http_code}' --max-time 2 "$HEALTH_URL" 2>/dev/null || true)
if [ -n "$PREEXISTING" ] && [ "$PREEXISTING" != "000" ]; then
  record FAIL "MCP port free" "something already answers on $HEALTH_URL (HTTP $PREEXISTING); stop it first"
  finish
fi

find_daemon_bin() {
  local candidate
  for candidate in \
    "$ROOT/target/debug/corvusd.exe" \
    "$ROOT/target/debug/corvusd" \
    "$ROOT/target/release/corvusd.exe" \
    "$ROOT/target/release/corvusd"; do
    if [ -x "$candidate" ]; then
      echo "$candidate"
      return 0
    fi
  done
  return 1
}

DAEMON_BIN="${CORVUSD_BIN:-}"
if [ -z "$DAEMON_BIN" ]; then
  DAEMON_BIN=$(find_daemon_bin || true)
fi
if [ -z "$DAEMON_BIN" ]; then
  if [ -z "${CARGO:-}" ]; then
    record FAIL "daemon builds" "no corvusd binary and no cargo on this machine"
    finish
  fi
  echo "building corvusd ..."
  if ! "$CARGO_BIN" build --offline -p corvusd --bin corvusd >"$TMP/daemon-build.log" 2>&1; then
    record FAIL "daemon builds" "cargo build --offline -p corvusd failed; see $TMP/daemon-build.log"
    finish
  fi
  DAEMON_BIN=$(find_daemon_bin || true)
  if [ -z "$DAEMON_BIN" ]; then
    record FAIL "daemon builds" "cargo build succeeded but no corvusd binary was found"
    finish
  fi
  record PASS "daemon builds" "cargo build --offline -p corvusd"
fi

start_daemon() { # extra args...
  "$DAEMON_BIN" "$@" >"$TMP/daemon.log" 2>&1 &
  DAEMON_PID=$!
}

DAEMON_ARGS=${CORVUSD_ARGS-serve}
# shellcheck disable=SC2086
start_daemon $DAEMON_ARGS
STARTED_AT=$SECONDS
READY=0
while [ $((SECONDS - STARTED_AT)) -lt 30 ]; do
  if ! kill -0 "$DAEMON_PID" 2>/dev/null; then
    break
  fi
  code=$(curl -s -o "$TMP/health.json" -w '%{http_code}' --max-time 2 "$HEALTH_URL" 2>/dev/null || true)
  if [ -n "$code" ] && [ "$code" != "000" ]; then
    READY=1
    break
  fi
  sleep 0.25
done

# The daemon CLI shape is still settling; retry once without arguments when
# the "serve" form exits immediately with a usage error.
if [ "$READY" = "0" ] && ! kill -0 "$DAEMON_PID" 2>/dev/null && [ "$DAEMON_ARGS" != "" ]; then
  echo "daemon exited with 'serve'; retrying without arguments ..."
  start_daemon
  STARTED_AT=$SECONDS
  while [ $((SECONDS - STARTED_AT)) -lt 30 ]; do
    if ! kill -0 "$DAEMON_PID" 2>/dev/null; then
      break
    fi
    code=$(curl -s -o "$TMP/health.json" -w '%{http_code}' --max-time 2 "$HEALTH_URL" 2>/dev/null || true)
    if [ -n "$code" ] && [ "$code" != "000" ]; then
      READY=1
      break
    fi
    sleep 0.25
  done
fi

if [ "$READY" != "1" ]; then
  record FAIL "daemon starts and binds MCP port" "see $TMP/daemon.log"
  finish
fi
record PASS "daemon starts and binds MCP port" "$(basename "$DAEMON_BIN") started in $((SECONDS - STARTED_AT))s"

HEALTH_CODE=$(curl -s -o "$TMP/health.json" -w '%{http_code}' --max-time 5 "$HEALTH_URL" 2>/dev/null || true)
if [ "$HEALTH_CODE" != "200" ]; then
  record FAIL "health endpoint" "GET $HEALTH_URL returned HTTP $HEALTH_CODE"
elif OUT=$(node "$ROOT/tests/conformance/mcp_assert.mjs" health "$TMP/health.json" 2>&1); then
  record PASS "health endpoint" "$OUT"
else
  record FAIL "health endpoint" "$OUT"
fi

start_mock() { # log out browser-id extra args...
  local log="$1" out="$2" browser="$3"
  shift 3
  "$MOCK_BIN" --url "$WS_URL" --browser-id "$browser" --out "$out" "$@" >"$log" 2>&1 &
  MOCK_PID=$!
  local i
  for i in $(seq 1 100); do
    if grep -q '^READY ' "$log" 2>/dev/null; then
      return 0
    fi
    if ! kill -0 "$MOCK_PID" 2>/dev/null; then
      return 1
    fi
    sleep 0.1
  done
  return 1
}

wait_for_no_browsers() {
  local i
  for i in $(seq 1 50); do
    curl -s -o "$TMP/health-wait.json" --max-time 2 "$HEALTH_URL" 2>/dev/null || true
    if node -e 'const fs=require("fs"); try { const h=JSON.parse(fs.readFileSync(process.argv[1],"utf8")); process.exit(Array.isArray(h.browsers)&&h.browsers.length===0?0:1); } catch { process.exit(1); }' "$TMP/health-wait.json" >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.1
  done
  return 1
}

PAGE_A='http://127.0.0.1:8808/a'
PAGE_B='http://127.0.0.1:8808/b'
MOCK_READY=0
if [ "$USE_MOCK" = "1" ] && [ -n "$MOCK_BIN" ]; then
  if start_mock "$TMP/mock.log" "$TMP/mock-envelopes.jsonl" "mock:conformance" \
    --exit-after 100; then
    MOCK_READY=1
    record PASS "mock extension handshake" "$(grep -m1 '^READY ' "$TMP/mock.log")"
  else
    record FAIL "mock extension handshake" "see $TMP/mock.log"
    USE_MOCK=0
  fi
elif [ "$USE_MOCK" = "1" ]; then
  record FAIL "mock extension available" "binary not found after build"
  USE_MOCK=0
else
  record SKIP "mock extension handshake" "--no-mock"
fi

# --- MCP over streamable HTTP -------------------------------------------------

mcp_post() { # payload body headers session with_protocol
  local payload="$1" body="$2" headers="$3" session="${4:-}" with_protocol="${5:-1}"
  local args=(
    -sS --max-time 30 -X POST "$MCP_URL"
    -H "Content-Type: application/json"
    -H "Accept: application/json, text/event-stream"
    -D "$headers" -o "$body" -w "%{http_code}"
  )
  if [ -n "$session" ]; then
    args+=(-H "Mcp-Session-Id: $session")
  fi
  if [ "$with_protocol" = "1" ]; then
    args+=(-H "MCP-Protocol-Version: $EXPECTED_PROTOCOL_VERSION")
  fi
  curl "${args[@]}" --data-raw "$payload" 2>"$TMP/curl.err" || true
}

expect_bad_args() { # label payload prefix
  local label="$1" payload="$2" prefix="$3"
  local status detail=""
  status=$(mcp_post "$payload" "$TMP/$prefix.json" "$TMP/$prefix.headers" "$SESSION")
  if [ "$status" = "200" ] && detail=$(node "$ROOT/tests/conformance/mcp_assert.mjs" call "$TMP/$prefix.json" error BAD_ARGS 2>&1); then
    record PASS "$label" "$detail"
  else
    record FAIL "$label" "HTTP $status ${detail:-$(head -c 200 "$TMP/$prefix.json" 2>/dev/null)}"
  fi
}

expect_navigate() { # label url expected-loaded prefix
  local label="$1" url="$2" loaded="$3" prefix="$4"
  local payload status detail=""
  payload='{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"browser_navigate","arguments":{"url":"'"$url"'"}}}'
  status=$(mcp_post "$payload" "$TMP/$prefix.json" "$TMP/$prefix.headers" "$SESSION")
  if [ "$status" = "200" ] && detail=$(node "$ROOT/tests/conformance/mcp_assert.mjs" navigate "$TMP/$prefix.json" "$loaded" "$url" 2>&1); then
    record PASS "$label" "$detail"
  else
    record FAIL "$label" "HTTP $status ${detail:-$(head -c 200 "$TMP/$prefix.json" 2>/dev/null)}"
  fi
}

expect_snapshot() { # label expected-url needle prefix
  local label="$1" url="$2" needle="$3" prefix="$4"
  local payload status detail=""
  payload='{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"page_snapshot","arguments":{}}}'
  status=$(mcp_post "$payload" "$TMP/$prefix.json" "$TMP/$prefix.headers" "$SESSION")
  if [ "$status" = "200" ] && detail=$(node "$ROOT/tests/conformance/mcp_assert.mjs" snapshot "$TMP/$prefix.json" "$url" "$needle" 2>&1); then
    record PASS "$label" "$detail"
  else
    record FAIL "$label" "HTTP $status ${detail:-$(head -c 200 "$TMP/$prefix.json" 2>/dev/null)}"
  fi
}

CALL_ID=20

expect_result() { # label tool arguments-json expected-json prefix [needle]
  local label="$1" tool="$2" arguments="$3" expected="$4" prefix="$5" text_needle="${6:-}"
  CALL_ID=$((CALL_ID + 1))
  local payload status detail
  payload=$(printf '{"jsonrpc":"2.0","id":%s,"method":"tools/call","params":{"name":"%s","arguments":%s}}' \
    "$CALL_ID" "$tool" "$arguments")
  status=$(mcp_post "$payload" "$TMP/$prefix.json" "$TMP/$prefix.headers" "$SESSION")
  if [ "$status" = "200" ] && detail=$(node "$ROOT/tests/conformance/mcp_assert.mjs" result "$TMP/$prefix.json" "$expected" "$text_needle" 2>&1); then
    record PASS "$label" "$detail"
  else
    record FAIL "$label" "HTTP $status ${detail:-$(head -c 200 "$TMP/$prefix.json" 2>/dev/null)}"
  fi
}

expect_tool_error() { # label tool arguments-json error-code prefix
  local label="$1" tool="$2" arguments="$3" code="$4" prefix="$5"
  CALL_ID=$((CALL_ID + 1))
  local payload status detail
  payload=$(printf '{"jsonrpc":"2.0","id":%s,"method":"tools/call","params":{"name":"%s","arguments":%s}}' \
    "$CALL_ID" "$tool" "$arguments")
  status=$(mcp_post "$payload" "$TMP/$prefix.json" "$TMP/$prefix.headers" "$SESSION")
  if [ "$status" = "200" ] && detail=$(node "$ROOT/tests/conformance/mcp_assert.mjs" call "$TMP/$prefix.json" error "$code" 2>&1); then
    record PASS "$label" "$detail"
  else
    record FAIL "$label" "HTTP $status ${detail:-$(head -c 200 "$TMP/$prefix.json" 2>/dev/null)}"
  fi
}

INIT_PAYLOAD='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"'"$EXPECTED_PROTOCOL_VERSION"'","capabilities":{},"clientInfo":{"name":"corvus-conformance","version":"0.1.0"}}}'
STATUS=$(mcp_post "$INIT_PAYLOAD" "$TMP/init.json" "$TMP/init.headers" "" 0)
SESSION=$(tr -d '\r' < "$TMP/init.headers" 2>/dev/null | sed -n 's/^[Mm][Cc][Pp]-[Ss][Ee][Ss][Ss][Ii][Oo][Nn]-[Ii][Dd]:[[:space:]]*//p' | head -n 1 || true)

if [ "$STATUS" != "200" ]; then
  record FAIL "initialize HTTP 200" "HTTP $STATUS ($(head -c 200 "$TMP/init.json" 2>/dev/null))"
else
  if OUT=$(node "$ROOT/tests/conformance/mcp_assert.mjs" protocol "$TMP/init.json" 2>&1); then
    record PASS "initialize protocolVersion" "$OUT"
  else
    record FAIL "initialize protocolVersion" "$OUT"
  fi
fi

if [ "$STATUS" = "200" ]; then
  NOTIFY_STATUS=$(mcp_post '{"jsonrpc":"2.0","method":"notifications/initialized"}' "$TMP/notify.json" "$TMP/notify.headers" "$SESSION")
  case "$NOTIFY_STATUS" in
    200|202|204) record PASS "notifications/initialized" "HTTP $NOTIFY_STATUS" ;;
    *) record FAIL "notifications/initialized" "HTTP $NOTIFY_STATUS" ;;
  esac
fi

LIST_STATUS=$(mcp_post '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' "$TMP/tools.json" "$TMP/tools.headers" "$SESSION")
if [ "$LIST_STATUS" != "200" ]; then
  record FAIL "tools/list HTTP 200" "HTTP $LIST_STATUS ($(head -c 200 "$TMP/tools.json" 2>/dev/null))"
else
  if OUT=$(node "$ROOT/tests/conformance/mcp_assert.mjs" tools "$TMP/tools.json" 2>&1); then
    NAME_LINE=$(printf '%s\n' "$OUT" | sed -n '1p')
    record PASS "tools/list exactly 18 M2 tools" "$NAME_LINE"
    record PASS "tools/list annotation hints" "$(printf '%s\n' "$OUT" | sed -n '2p')"
    record PASS "tools/list input schema shapes" "$(printf '%s\n' "$OUT" | sed -n '3p')"
  else
    record FAIL "tools/list contract" "$OUT"
  fi
fi

CALL_PAYLOAD='{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"page_snapshot","arguments":{}}}'
CALL_STATUS=$(mcp_post "$CALL_PAYLOAD" "$TMP/call.json" "$TMP/call.headers" "$SESSION")
if [ "$USE_MOCK" = "1" ]; then
  if [ "$CALL_STATUS" != "200" ]; then
    record FAIL "tools/call page_snapshot" "HTTP $CALL_STATUS ($(head -c 200 "$TMP/call.json" 2>/dev/null))"
  elif OUT=$(node "$ROOT/tests/conformance/mcp_assert.mjs" call "$TMP/call.json" success "Run" 2>&1); then
    record PASS "tools/call page_snapshot (round trip)" "$OUT"
  else
    record FAIL "tools/call page_snapshot (round trip)" "$OUT"
  fi
else
  if OUT=$(node "$ROOT/tests/conformance/mcp_assert.mjs" call "$TMP/call.json" error 2>&1); then
    record SKIP "tools/call page_snapshot (round trip)" "no mock extension; daemon answered structured error: $OUT"
  else
    record FAIL "tools/call page_snapshot (no extension path)" "$OUT"
  fi
fi

if [ "$USE_MOCK" = "1" ] && [ "$MOCK_READY" = "1" ]; then
  expect_navigate "browser_navigate loaded and final url" "$PAGE_A" true "navigate-a"
  expect_navigate "browser_navigate switches to page B" "$PAGE_B" true "navigate-b"
  expect_snapshot "snapshot immediately after navigate sees page B" "$PAGE_B" "Run" "snapshot-b"
else
  record SKIP "browser_navigate loaded and final url" "no mock extension"
  record SKIP "browser_navigate switches to page B" "no mock extension"
  record SKIP "snapshot immediately after navigate sees page B" "no mock extension"
fi

# --- M2 tool matrix ---------------------------------------------------------

if [ "$USE_MOCK" = "1" ] && [ "$MOCK_READY" = "1" ]; then
  expect_result "page_text returns page B text" page_text '{}' \
    '{"url":"'$PAGE_B'","truncated":false}' "page-text" "Mock page text"
  expect_result "page_html returns document html" page_html '{}' \
    '{"url":"'$PAGE_B'","truncated":false}' "page-html" "<h1>Mock page</h1>"
  expect_result "page_find finds Run" page_find '{"query":"Run"}' \
    '{"url":"'$PAGE_B'","count":1}' "page-find" "@e1"
  expect_result "browser_hover resolves target" browser_hover '{"target":"@e1"}' \
    '{"hovered":true,"url":"'$PAGE_B'","target":{"ref":"@e1","tag":"button"}}' "hover"
  expect_result "browser_press sends keys" browser_press '{"keys":["Enter","Tab"]}' \
    '{"pressed":true,"keys":["Enter","Tab"],"url":"'$PAGE_B'"}' "press"
  expect_result "browser_scroll moves page" browser_scroll '{"direction":"down","pages":2}' \
    '{"scrolled":true,"x":0,"y":200,"url":"'$PAGE_B'"}' "scroll"
  expect_result "browser_select chooses values" browser_select '{"target":"#sel","values":["alpha","beta"]}' \
    '{"selected":["alpha","beta"],"value":"alpha","url":"'$PAGE_B'"}' "select"
  expect_result "browser_eval returns a value" browser_eval '{"expression":"1 + 1"}' \
    '{"ok":true,"value":2}' "eval-value"
  expect_result "browser_eval page exception is ok=false" browser_eval '{"expression":"throw new Error(\"boom\")"}' \
    '{"ok":false,"error":{"name":"Error"}}' "eval-throw" "mock page error"
  expect_result "browser_eval unserializable value" browser_eval '{"expression":"window"}' \
    '{"ok":true,"value":null,"value_note":"unserializable"}' "eval-unserializable"
  expect_result "browser_wait matches selector" browser_wait '{"selector":"#ready","timeout_ms":1000}' \
    '{"matched":true,"url":"'$PAGE_B'"}' "wait-match"
  expect_result "browser_history goes back" browser_history '{"action":"back"}' \
    '{"action":"back","url":"'$PAGE_A'","loaded":true}' "history-back"
  expect_tool_error "browser_wait timeout is TIMEOUT" browser_wait '{"selector":"#never","timeout_ms":100}' \
    TIMEOUT "wait-timeout"
  expect_result "tab_list shows the active tab" tab_list '{}' \
    '{"tabs":[{"tab_id":"tab-1","index":0,"active":true,"url":"'$PAGE_A'"}]}' "tab-list-1"
  expect_result "tab_new opens tab-2" tab_new '{"url":"http://127.0.0.1:8808/c","active":true}' \
    '{"tab_id":"tab-2","url":"http://127.0.0.1:8808/c","loaded":true,"previous_tab_id":"tab-1"}' "tab-new"
  expect_result "page_text honours tab_id" page_text '{"tab_id":"tab-1"}' \
    '{"url":"'$PAGE_A'","truncated":false}' "page-text-tab"
  expect_result "tab_list shows both tabs" tab_list '{}' \
    '{"tabs":[{"tab_id":"tab-1","active":false,"url":"'$PAGE_A'"},{"tab_id":"tab-2","active":true,"url":"http://127.0.0.1:8808/c"}]}' "tab-list-2"
  expect_result "tab_switch activates tab-1" tab_switch '{"tab_id":"tab-1"}' \
    '{"tab_id":"tab-1","url":"'$PAGE_A'","title":"Mock page","active":true}' "tab-switch"
  expect_tool_error "tab_switch unknown tab is NOT_FOUND" tab_switch '{"tab_id":"tab-999"}' \
    NOT_FOUND "tab-not-found"
  expect_result "tab_close closes tab-2" tab_close '{"tab_id":"tab-2"}' \
    '{"closed":true,"tab_id":"tab-2"}' "tab-close"
  expect_result "tab_list returns to one tab" tab_list '{}' \
    '{"tabs":[{"tab_id":"tab-1","active":true,"url":"'$PAGE_A'"}]}' "tab-list-3"
  expect_result "browser_type captures text" browser_type '{"target":"@e1","text":"hello corvus"}' \
    '{"typed":true,"ref":"@e1","value":"hello corvus","value_length":12,"url":"'$PAGE_A'"}' "type-m2"
  expect_result "browser_click dispatches pointer events" browser_click '{"target":"@e2"}' \
    '{"clicked":true,"ref":"@e2","url":"'$PAGE_A'"}' "click-m2" "pointerdown"
  expect_result "page_text truncates at max_chars" page_text '{"max_chars":20}' \
    '{"url":"'$PAGE_A'","truncated":true}' "page-text-truncated"
  expect_result "page_html truncates at max_chars" page_html '{"max_chars":10}' \
    '{"url":"'$PAGE_A'","truncated":true}' "page-html-truncated"
  expect_result "page_find reports no matches" page_find '{"query":"nothing"}' \
    '{"url":"'$PAGE_A'","count":0}' "page-find-none"
  expect_result "page_html default max_chars after overrides" page_html '{}' \
    '{"url":"'$PAGE_A'","truncated":false}' "page-html-default"
else
  record SKIP "M2 happy tool matrix" "no mock extension"
  record SKIP "M2 extension error paths" "no mock extension"
fi

UNKNOWN_PAYLOAD='{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"corvus_tool_that_does_not_exist","arguments":{}}}'
UNKNOWN_STATUS=$(mcp_post "$UNKNOWN_PAYLOAD" "$TMP/unknown.json" "$TMP/unknown.headers" "$SESSION")
case "$UNKNOWN_STATUS" in
  200|400|404)
    if OUT=$(node "$ROOT/tests/conformance/mcp_assert.mjs" call "$TMP/unknown.json" error 2>&1); then
      record PASS "tools/call unknown tool" "$OUT (HTTP $UNKNOWN_STATUS)"
    else
      record FAIL "tools/call unknown tool" "$OUT (HTTP $UNKNOWN_STATUS)"
    fi
    ;;
  *)
    record FAIL "tools/call unknown tool" "unexpected HTTP $UNKNOWN_STATUS: $(head -c 200 "$TMP/unknown.json" 2>/dev/null)"
    ;;
esac

if [ "$USE_MOCK" = "1" ] && [ -f "$TMP/mock-envelopes.jsonl" ]; then
  if OUT=$(node "$ROOT/tests/conformance/mcp_assert.mjs" command-mode "$TMP/mock-envelopes.jsonl" page_snapshot interactive 2>&1); then
    record PASS "page_snapshot default mode" "$OUT"
  else
    record FAIL "page_snapshot default mode" "$OUT"
  fi
fi

expect_bad_args "page_snapshot rejects mode everything" '{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"page_snapshot","arguments":{"mode":"everything"}}}' "bad-mode"
expect_bad_args "page_snapshot rejects max_nodes 10" '{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"page_snapshot","arguments":{"max_nodes":10}}}' "bad-max-nodes"
expect_bad_args "browser_click rejects missing target" '{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"browser_click","arguments":{}}}' "bad-click"
expect_bad_args "browser_type rejects missing text" '{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"browser_type","arguments":{"target":"@e1"}}}' "bad-type"
expect_bad_args "browser_navigate rejects missing url" '{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"browser_navigate","arguments":{}}}' "bad-navigate"
expect_bad_args "page_text rejects max_chars 0" '{"jsonrpc":"2.0","id":12,"method":"tools/call","params":{"name":"page_text","arguments":{"max_chars":0}}}' "bad-page-text"
expect_bad_args "page_html rejects max_chars 500001" '{"jsonrpc":"2.0","id":13,"method":"tools/call","params":{"name":"page_html","arguments":{"max_chars":500001}}}' "bad-page-html"
expect_bad_args "page_find rejects missing query" '{"jsonrpc":"2.0","id":14,"method":"tools/call","params":{"name":"page_find","arguments":{}}}' "bad-page-find"
expect_bad_args "browser_hover rejects missing target" '{"jsonrpc":"2.0","id":15,"method":"tools/call","params":{"name":"browser_hover","arguments":{}}}' "bad-hover"
expect_bad_args "browser_press rejects empty keys" '{"jsonrpc":"2.0","id":16,"method":"tools/call","params":{"name":"browser_press","arguments":{"keys":[]}}}' "bad-press"
expect_bad_args "browser_scroll rejects diagonal" '{"jsonrpc":"2.0","id":17,"method":"tools/call","params":{"name":"browser_scroll","arguments":{"direction":"diagonal"}}}' "bad-scroll"
expect_bad_args "browser_select rejects empty values" '{"jsonrpc":"2.0","id":18,"method":"tools/call","params":{"name":"browser_select","arguments":{"target":"#sel","values":[]}}}' "bad-select"
expect_bad_args "browser_eval rejects world server" '{"jsonrpc":"2.0","id":19,"method":"tools/call","params":{"name":"browser_eval","arguments":{"expression":"1 + 1","world":"server"}}}' "bad-eval"
expect_bad_args "browser_wait rejects no condition" '{"jsonrpc":"2.0","id":20,"method":"tools/call","params":{"name":"browser_wait","arguments":{}}}' "bad-wait"
expect_bad_args "browser_history rejects refresh" '{"jsonrpc":"2.0","id":21,"method":"tools/call","params":{"name":"browser_history","arguments":{"action":"refresh"}}}' "bad-history"
expect_bad_args "tab_new rejects active yes" '{"jsonrpc":"2.0","id":22,"method":"tools/call","params":{"name":"tab_new","arguments":{"active":"yes"}}}' "bad-tab-new"
expect_bad_args "tab_close rejects missing tab_id" '{"jsonrpc":"2.0","id":23,"method":"tools/call","params":{"name":"tab_close","arguments":{}}}' "bad-tab-close"
expect_bad_args "tab_switch rejects missing tab_id" '{"jsonrpc":"2.0","id":24,"method":"tools/call","params":{"name":"tab_switch","arguments":{}}}' "bad-tab-switch"

if [ -f "$TMP/mock-envelopes.jsonl" ]; then
  RECORDED=$(wc -l < "$TMP/mock-envelopes.jsonl" | tr -d ' ')
  if [ "$RECORDED" -ge 18 ]; then
    record PASS "envelope recording" "$RECORDED received envelopes in mock-envelopes.jsonl"
  else
    record FAIL "envelope recording" "only $RECORDED envelopes in mock-envelopes.jsonl"
  fi
  if OUT=$(node "$ROOT/tests/conformance/mcp_assert.mjs" command "$TMP/mock-envelopes.jsonl" page_html '{"max_chars":200000}' 2>&1); then
    record PASS "recorded page_html default max_chars" "$OUT"
  else
    record FAIL "recorded page_html default max_chars" "$OUT"
  fi
  if OUT=$(node "$ROOT/tests/conformance/mcp_assert.mjs" command "$TMP/mock-envelopes.jsonl" browser_eval '{"world":"isolated"}' 2>&1); then
    record PASS "recorded browser_eval default world" "$OUT"
  else
    record FAIL "recorded browser_eval default world" "$OUT"
  fi
fi

if [ "$USE_MOCK" = "1" ]; then
  stop_pid "$MOCK_PID"
  MOCK_PID=""
  if ! wait_for_no_browsers; then
    record FAIL "mock extension unregisters before reload" "health still lists a browser; cannot isolate loaded=false"
  elif start_mock "$TMP/mock-negative.log" "$TMP/mock-negative-envelopes.jsonl" "mock:negative" \
    --answer-tool browser_navigate \
    --answer-json '{"loaded":false,"url":"http://127.0.0.1:8808/slow"}' \
    --exit-after 100; then
    record PASS "mock extension reload (loaded=false)" "$(grep -m1 '^READY ' "$TMP/mock-negative.log")"
    expect_navigate "browser_navigate loaded=false is not an error" "http://127.0.0.1:8808/slow" false "navigate-slow"
  else
    record FAIL "mock extension reload (loaded=false)" "see $TMP/mock-negative.log"
  fi
else
  record SKIP "browser_navigate loaded=false is not an error" "no mock extension"
fi

finish
