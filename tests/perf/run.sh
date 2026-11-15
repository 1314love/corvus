#!/usr/bin/env bash
# Corvus M4 performance and memory evidence runner.
# Builds or uses a corvusd binary, starts an isolated daemon, mock extension and
# static fixture server, runs every benchmark mode, and prints a summary table.
# Scope: tests only. No product source is modified.
set -o pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
cd "$ROOT" || exit 1

if [ -z "$CARGO" ]; then
  if command -v cargo >/dev/null 2>&1; then
    CARGO=cargo
  elif [ -x "$HOME/.cargo/bin/cargo.exe" ]; then
    CARGO=$HOME/.cargo/bin/cargo.exe
  elif [ -x "$HOME/.cargo/bin/cargo" ]; then
    CARGO=$HOME/.cargo/bin/cargo
  else
    CARGO=cargo
  fi
fi

# shellcheck source=../lib/tmp.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/lib/tmp.sh"
TMPROOT=$(corvus_tmp_root) || exit 1
TMP=$(mktemp -d "$TMPROOT/corvus-perf.XXXXXX") || exit 1
EVID=$TMP/evidence
mkdir -p "$EVID" || exit 1

PERF_WS_PORT=$PERF_WS_PORT
[ -n "$PERF_WS_PORT" ] || PERF_WS_PORT=31330
PERF_HTTP_PORT=$PERF_HTTP_PORT
[ -n "$PERF_HTTP_PORT" ] || PERF_HTTP_PORT=$((PERF_WS_PORT + 1))
PERF_LATENCY_SAMPLES=$PERF_LATENCY_SAMPLES
[ -n "$PERF_LATENCY_SAMPLES" ] || PERF_LATENCY_SAMPLES=60
PERF_LIVE_SAMPLES=$PERF_LIVE_SAMPLES
[ -n "$PERF_LIVE_SAMPLES" ] || PERF_LIVE_SAMPLES=60
PERF_MEMORY_SECONDS=$PERF_MEMORY_SECONDS
[ -n "$PERF_MEMORY_SECONDS" ] || PERF_MEMORY_SECONDS=900
PERF_IDLE_SECONDS=$PERF_IDLE_SECONDS
[ -n "$PERF_IDLE_SECONDS" ] || PERF_IDLE_SECONDS=120
PERF_SAMPLE_SECONDS=$PERF_SAMPLE_SECONDS
[ -n "$PERF_SAMPLE_SECONDS" ] || PERF_SAMPLE_SECONDS=2
PERF_WORKERS=$PERF_WORKERS
[ -n "$PERF_WORKERS" ] || PERF_WORKERS=3
PERF_RATE_MS=$PERF_RATE_MS
[ -n "$PERF_RATE_MS" ] || PERF_RATE_MS=150
PERF_KEEP_TMP=$PERF_KEEP_TMP
[ -n "$PERF_KEEP_TMP" ] || PERF_KEEP_TMP=1
PERF_LIVE_MCP_URL=$PERF_LIVE_MCP_URL
[ -n "$PERF_LIVE_MCP_URL" ] || PERF_LIVE_MCP_URL=http://127.0.0.1:7331/mcp
PERF_LIVE_BROWSER_ID=$PERF_LIVE_BROWSER_ID
[ -n "$PERF_LIVE_BROWSER_ID" ] || PERF_LIVE_BROWSER_ID=edge:b5e09e58ecdf3caa

DB=$CORVUS_DB
[ -n "$DB" ] || DB=$TMP/corvus-perf.db
DAEMON_PID=
MOCK_PID=
FIXTURE_PID=
DAEMON_BIN=
MOCK_BIN=

cleanup() {
  set +e
  [ -n "$MOCK_PID" ] && kill "$MOCK_PID" 2>/dev/null
  [ -n "$FIXTURE_PID" ] && kill "$FIXTURE_PID" 2>/dev/null
  [ -n "$DAEMON_PID" ] && kill "$DAEMON_PID" 2>/dev/null
  sleep 0.2
  rm -f "$DB" "$DB-wal" "$DB-shm"
  if [ "$PERF_KEEP_TMP" != "1" ]; then
    rm -rf "$TMP"
  else
    echo "EVIDENCE $EVID"
  fi
}
trap cleanup EXIT INT TERM

echo "== build =="
if [ -n "$CORVUSD_BIN" ]; then
  DAEMON_BIN=$CORVUSD_BIN
  echo "using CORVUSD_BIN=$DAEMON_BIN"
else
  TARGET_DIR=$PERF_TARGET_DIR
  [ -n "$TARGET_DIR" ] || TARGET_DIR=target/perf-release
  echo "cargo build --release --offline -p corvusd --bin corvusd -> $TARGET_DIR"
  CARGO_TARGET_DIR=$TARGET_DIR "$CARGO" build --release --offline -p corvusd --bin corvusd >"$TMP/build-daemon.log" 2>&1 || {
    cat "$TMP/build-daemon.log"
    exit 1
  }
  DAEMON_BIN=$TARGET_DIR/release/corvusd.exe
fi
if [ ! -x "$DAEMON_BIN" ]; then
  echo "daemon binary not executable: $DAEMON_BIN"
  exit 1
fi
echo "daemon binary: $DAEMON_BIN"

"$CARGO" build --offline --manifest-path tests/mock-extension/Cargo.toml >"$TMP/build-mock.log" 2>&1 || {
  cat "$TMP/build-mock.log"
  exit 1
}
MOCK_BIN=$ROOT/tests/mock-extension/target/debug/mock-extension.exe
if [ ! -x "$MOCK_BIN" ]; then
  echo "mock binary not executable: $MOCK_BIN"
  exit 1
fi

echo "== fixture server =="
PORT=0 HOST=127.0.0.1 node tests/perf/serve-fixture.mjs >"$TMP/fixture.log" 2>&1 &
FIXTURE_PID=$!
FIXTURE_URL=
i=1
while [ "$i" -le 100 ]; do
  FIXTURE_URL=$(awk '/^READY /{print $2; exit}' "$TMP/fixture.log" 2>/dev/null)
  [ -n "$FIXTURE_URL" ] && break
  if ! kill -0 "$FIXTURE_PID" 2>/dev/null; then
    break
  fi
  sleep 0.1
  i=$((i + 1))
done
if [ -z "$FIXTURE_URL" ]; then
  cat "$TMP/fixture.log"
  echo "fixture server did not start"
  exit 1
fi
echo "fixture: $FIXTURE_URL"

echo "== isolated daemon on $PERF_WS_PORT/$PERF_HTTP_PORT with temp DB =="
"$DAEMON_BIN" serve --db "$DB" --ws-port "$PERF_WS_PORT" --http-port "$PERF_HTTP_PORT" >"$TMP/daemon.log" 2>&1 &
DAEMON_PID=$!
MCP_URL=http://127.0.0.1:$PERF_HTTP_PORT/mcp
HEALTH_URL=http://127.0.0.1:$PERF_HTTP_PORT/health
HEALTH_OK=0
i=1
while [ "$i" -le 100 ]; do
  if curl -fsS --max-time 2 "$HEALTH_URL" >/dev/null 2>&1; then
    HEALTH_OK=1
    break
  fi
  if ! kill -0 "$DAEMON_PID" 2>/dev/null; then
    break
  fi
  sleep 0.1
  i=$((i + 1))
done
if [ "$HEALTH_OK" != "1" ]; then
  cat "$TMP/daemon.log"
  echo "daemon did not become healthy"
  exit 1
fi
echo "MCP: $MCP_URL"

echo "== mock extension =="
"$MOCK_BIN" --url "ws://127.0.0.1:$PERF_WS_PORT" --browser-id mock:perf --profile perf --out "$TMP/mock.jsonl" --wait-delay-ms 250 >"$TMP/mock.log" 2>&1 &
MOCK_PID=$!
MOCK_OK=0
i=1
while [ "$i" -le 100 ]; do
  HEALTH_JSON=$(curl -fsS --max-time 2 "$HEALTH_URL" 2>/dev/null)
  case "$HEALTH_JSON" in
    *mock:perf*) MOCK_OK=1; break ;;
  esac
  if ! kill -0 "$MOCK_PID" 2>/dev/null; then
    break
  fi
  sleep 0.1
  i=$((i + 1))
done
if [ "$MOCK_OK" != "1" ]; then
  cat "$TMP/mock.log"
  echo "mock extension did not register"
  exit 1
fi

NETSTAT=/c/Windows/System32/netstat.exe
[ -x "$NETSTAT" ] || NETSTAT=netstat
WINPID=$("$NETSTAT" -ano 2>/dev/null | awk -v p="127.0.0.1:$PERF_HTTP_PORT" '$1=="TCP" && $2==p && $4=="LISTENING" {print $5; exit}')
if [ -z "$WINPID" ]; then
  WINPID=$("$NETSTAT" -ano 2>/dev/null | awk -v p=":$PERF_HTTP_PORT" '$1=="TCP" && index($2,p)>0 && $4=="LISTENING" {print $5; exit}')
fi
if [ -z "$WINPID" ]; then
  echo "cannot determine daemon PID for RSS sampling"
  exit 1
fi
echo "daemon pid: $WINPID"

PAGE_URL=$FIXTURE_URL/perf-10k.html
LOAD_URL=$FIXTURE_URL/load

echo "== latency =="
node tests/perf/bench.mjs latency --url "$MCP_URL" --browser-id mock:perf --fixture-url "$PAGE_URL" --samples "$PERF_LATENCY_SAMPLES" --out "$EVID/latency.json" || exit 1

echo "== concurrency =="
node tests/perf/bench.mjs concurrency --url "$MCP_URL" --browser-id mock:perf --fixture-url "$PAGE_URL" --tabs 10 --clients 10 --out "$EVID/concurrency.json" || exit 1

echo "== memory ($PERF_MEMORY_SECONDS s load + $PERF_IDLE_SECONDS s idle) =="
node tests/perf/bench.mjs memory --url "$MCP_URL" --browser-id mock:perf --rss-pid "$WINPID" --load-url "$LOAD_URL" --duration "$PERF_MEMORY_SECONDS" --idle "$PERF_IDLE_SECONDS" --sample "$PERF_SAMPLE_SECONDS" --workers "$PERF_WORKERS" --rate-ms "$PERF_RATE_MS" --out "$EVID/memory.json" || exit 1

echo "== live headless Edge smoke =="
node tests/perf/bench.mjs live --url "$PERF_LIVE_MCP_URL" --browser-id "$PERF_LIVE_BROWSER_ID" --fixture-url "$PAGE_URL" --samples "$PERF_LIVE_SAMPLES" --out "$EVID/live.json" || exit 1

echo "== sizes =="
node tests/perf/bench.mjs size --binary "$DAEMON_BIN" --extension "$ROOT/extension" --out "$EVID/size.json" || exit 1

echo "== summary =="
node tests/perf/bench.mjs summary --dir "$EVID" >"$EVID/summary.txt"
SUMMARY_EXIT=$?
cat "$EVID/summary.txt"
if [ "$SUMMARY_EXIT" != "0" ]; then
  exit 1
fi
exit 0
