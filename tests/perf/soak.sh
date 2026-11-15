#!/usr/bin/env bash
# Corvus NFR-3 soak launcher. QA test code only.
# Mock mode starts an isolated daemon + mock extension on 29430/29431 with a temp DB.
# Live mode only drives the already-running service through an owned tab; it starts no daemon.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
cd "$ROOT"

CARGO=${CARGO:-}
if [[ -z "$CARGO" ]]; then
  if command -v cargo >/dev/null 2>&1; then
    CARGO=$(command -v cargo)
  elif [[ -x "$HOME/.cargo/bin/cargo.exe" ]]; then
    CARGO="$HOME/.cargo/bin/cargo.exe"
  else
    CARGO="$HOME/.cargo/bin/cargo"
  fi
fi

to_win() {
  if command -v cygpath >/dev/null 2>&1; then
    cygpath -m "$1"
  else
    printf '%s' "$1"
  fi
}

# shellcheck source=../lib/tmp.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/lib/tmp.sh"
TMPBASE=$(corvus_tmp_root) || exit 1
LIVE=${SOAK_LIVE:-0}
WS_PORT=${SOAK_WS_PORT:-29430}
HTTP_PORT=${SOAK_HTTP_PORT:-$((WS_PORT + 1))}
FIXTURE_URL=${SOAK_FIXTURE_URL:-}
if [[ "$LIVE" == "1" ]]; then
  EVID=${SOAK_EVID:-"$TMPBASE/corvus-nfr3-live"}
  MCP_URL=${SOAK_MCP_URL:-"http://127.0.0.1:7331/mcp"}
  HEALTH_URL=${SOAK_HEALTH_URL:-"http://127.0.0.1:7331/health"}
  BROWSER_ID=${SOAK_BROWSER_ID:-"chromium:7abc8aafb8098216"}
  FIXTURE_URL=${FIXTURE_URL:-"http://127.0.0.1:8811/blank"}
  DURATION=${SOAK_DURATION_SECONDS:-600}
  CHECKPOINT=${SOAK_CHECKPOINT_SECONDS:-60}
  WORKERS=${SOAK_WORKERS:-2}
  RATE_MS=${SOAK_RATE_MS:-1000}
  SUMMARY=${SOAK_SUMMARY:-"$ROOT/tests/report/NFR-3-live-summary.txt"}
  EVIDENCE_LABEL="live soak"
else
  EVID=${SOAK_EVID:-"$TMPBASE/corvus-nfr3-soak"}
  MCP_URL="http://127.0.0.1:$HTTP_PORT/mcp"
  HEALTH_URL="http://127.0.0.1:$HTTP_PORT/health"
  BROWSER_ID=${SOAK_BROWSER_ID:-"mock:soak"}
  FIXTURE_URL=${FIXTURE_URL:-"http://127.0.0.1:8811/perf-10k.html"}
  DURATION=${SOAK_DURATION_SECONDS:-259200}
  CHECKPOINT=${SOAK_CHECKPOINT_SECONDS:-3600}
  WORKERS=${SOAK_WORKERS:-2}
  RATE_MS=${SOAK_RATE_MS:-2000}
  SUMMARY=${SOAK_SUMMARY:-"$ROOT/tests/report/NFR-3-soak-summary.txt"}
  EVIDENCE_LABEL="mock soak"
fi

mkdir -p "$EVID"
EVID_WIN=$(to_win "$EVID")
SUMMARY_WIN=$(to_win "$SUMMARY")
MOCK_BIN=${SOAK_MOCK_BIN:-"$ROOT/tests/mock-extension/target/debug/mock-extension.exe"}
DAEMON_BIN=${CORVUSD_BIN:-""}
DAEMON_PID=""
MOCK_PID=""
WIN_PID=""
DB="$EVID/corvus.db"
DB_WIN=$(to_win "$DB")

cleanup() {
  local status=$?
  trap - EXIT INT TERM
  if [[ -n "$MOCK_PID" ]]; then
    kill "$MOCK_PID" >/dev/null 2>&1 || true
  fi
  if [[ -n "$DAEMON_PID" ]]; then
    kill "$DAEMON_PID" >/dev/null 2>&1 || true
  fi
  if [[ -n "$WIN_PID" ]]; then
    taskkill //F //PID "$WIN_PID" >/dev/null 2>&1 || true
  fi
  wait >/dev/null 2>&1 || true
  echo "EVIDENCE $EVID"
  exit "$status"
}
trap cleanup EXIT INT TERM

echo "SOAK launch mode=$EVIDENCE_LABEL evidence=$EVID summary=$SUMMARY"
echo "SOAK ports ws=$WS_PORT http=$HTTP_PORT duration=${DURATION}s checkpoint=${CHECKPOINT}s workers=$WORKERS rate=${RATE_MS}ms"

if [[ "$LIVE" == "1" ]]; then
  if ! curl -fsS --max-time 10 "$HEALTH_URL" >/dev/null 2>&1; then
    echo "health endpoint is not reachable: $HEALTH_URL" >&2
    exit 1
  fi
  if ! curl -fsS --max-time 10 "$FIXTURE_URL" >/dev/null 2>&1; then
    echo "fixture URL is not reachable: $FIXTURE_URL" >&2
    exit 1
  fi
  node "$ROOT/tests/perf/soak.mjs" --live --mcp-url "$MCP_URL" --health-url "$HEALTH_URL" --browser-id "$BROWSER_ID" --fixture-url "$FIXTURE_URL" --evidence-dir "$EVID_WIN" --summary-file "$SUMMARY_WIN" --duration-seconds "$DURATION" --checkpoint-seconds "$CHECKPOINT" --workers "$WORKERS" --rate-ms "$RATE_MS"
  exit 0
fi

if /c/Windows/System32/netstat.exe -ano 2>/dev/null | grep -E "[:.]$HTTP_PORT[[:space:]]" | grep -q LISTENING; then
  echo "port $HTTP_PORT is already listening; refusing to disturb it" >&2
  exit 1
fi

if [[ -z "$DAEMON_BIN" ]]; then
  if [[ ! -x "$ROOT/target/qa-soak-build/release/corvusd.exe" ]]; then
    echo "SOAK building release daemon"
    CARGO_TARGET_DIR="target/qa-soak-build" "$CARGO" build --release --offline -p corvusd --bin corvusd >"$EVID/build-daemon.log" 2>&1
  fi
  DAEMON_BIN="$ROOT/target/qa-soak-build/release/corvusd.exe"
fi
if [[ ! -x "$DAEMON_BIN" ]]; then
  echo "daemon binary not executable: $DAEMON_BIN" >&2
  exit 1
fi

if [[ ! -x "$MOCK_BIN" ]]; then
  echo "SOAK building mock extension"
  "$CARGO" build --offline --manifest-path "$ROOT/tests/mock-extension/Cargo.toml" >"$EVID/build-mock.log" 2>&1
fi
if [[ ! -x "$MOCK_BIN" ]]; then
  echo "mock binary not executable: $MOCK_BIN" >&2
  exit 1
fi

if [[ "${SOAK_FRESH:-0}" == "1" || ! -f "$EVID/state.json" ]]; then
  rm -f "$DB" "$DB-wal" "$DB-shm"
fi
echo "SOAK starting daemon db=$DB_WIN"
"$DAEMON_BIN" serve --db "$DB_WIN" --ws-port "$WS_PORT" --http-port "$HTTP_PORT" >"$EVID/daemon.log" 2>&1 &
DAEMON_PID=$!

READY=0
for _ in $(seq 1 120); do
  if curl -fsS --max-time 2 "$HEALTH_URL" >"$EVID/health-start.json" 2>/dev/null; then
    READY=1
    break
  fi
  sleep 0.5
done
if [[ "$READY" != "1" ]]; then
  echo "daemon did not become healthy on $HEALTH_URL" >&2
  tail -40 "$EVID/daemon.log" >&2 || true
  exit 1
fi

echo "SOAK starting mock browser=$BROWSER_ID"
"$MOCK_BIN" --url "ws://127.0.0.1:$WS_PORT" --browser-id "$BROWSER_ID" --profile soak --quiet --idle-timeout-ms 0 >"$EVID/mock.log" 2>&1 &
MOCK_PID=$!

MOCK_READY=0
for _ in $(seq 1 120); do
  if curl -fsS --max-time 2 "$HEALTH_URL" 2>/dev/null | grep -q "$BROWSER_ID"; then
    MOCK_READY=1
    break
  fi
  sleep 0.5
done
if [[ "$MOCK_READY" != "1" ]]; then
  echo "mock browser did not connect" >&2
  tail -40 "$EVID/mock.log" >&2 || true
  exit 1
fi

WIN_PID=$(/c/Windows/System32/netstat.exe -ano 2>/dev/null | awk -v p=":$HTTP_PORT" '$2 ~ p && $4 == "LISTENING" { print $5; exit }')
if [[ -z "$WIN_PID" ]]; then
  echo "could not determine daemon Windows PID on port $HTTP_PORT" >&2
  exit 1
fi
echo "SOAK daemon Windows pid=$WIN_PID"

node "$ROOT/tests/perf/soak.mjs" --mcp-url "$MCP_URL" --health-url "$HEALTH_URL" --browser-id "$BROWSER_ID" --fixture-url "$FIXTURE_URL" --rss-pid "$WIN_PID" --evidence-dir "$EVID_WIN" --summary-file "$SUMMARY_WIN" --duration-seconds "$DURATION" --checkpoint-seconds "$CHECKPOINT" --workers "$WORKERS" --rate-ms "$RATE_MS"
