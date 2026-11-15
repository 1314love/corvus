#!/usr/bin/env bash
#
# Install or upgrade Corvus on a systemd Linux host.
#
# The installer creates one root tree, installs/updates the systemd unit, and
# starts the service. It is idempotent. The only file outside the root is the
# unit at /etc/systemd/system/corvus.service.
#
# Permissions: the home, bin and extension are 0755 root:root so a desktop user
# can run bin/corvus and load extension/; data, logs and config stay 0750 owned
# by the service user.
# Usage: sudo scripts/install-linux.sh
# Environment overrides: CORVUS_HOME, CORVUS_USER, CORVUS_WS_PORT, CORVUS_HTTP_PORT.

set -euo pipefail

fail() {
    echo "install-linux: $*" >&2
    exit 1
}

usage() {
    cat <<'EOF'
Usage: sudo scripts/install-linux.sh [--self-test]

  --self-test  verify the CORVUS_HOME safety guard without installing

Environment:
  CORVUS_HOME       installation root (default /opt/corvus)
  CORVUS_USER       service user and group (default corvus)
  CORVUS_WS_PORT    WebSocket port (default 7330)
  CORVUS_HTTP_PORT  MCP HTTP port (default 7331)

The script must run from the extracted release package, which contains bin/,
extension/ and systemd/corvus.service.
EOF
}

assert_safe_home() {
    local path="$1"
    case "$path" in
        /home|/home/*|/root|/root/*|/usr|/usr/*|/etc|/etc/*|/bin|/bin/*|/sbin|/sbin/*|/lib|/lib/*|/lib64|/lib64/*|/boot|/boot/*|/dev|/dev/*|/proc|/proc/*|/sys|/sys/*)
            fail "CORVUS_HOME must not be under a system or user directory: $path" ;;
    esac
    local current="$path"
    while [ -n "$current" ]; do
        if [ -L "$current" ]; then
            fail "CORVUS_HOME must not contain a symlinked ancestor: $current"
        fi
        if [ "$current" = "/" ]; then
            break
        fi
        current="$(dirname "$current")"
    done
}

self_test=0
while [ "$#" -gt 0 ]; do
    case "$1" in
        --self-test) self_test=1 ;;
        -h|--help) usage; exit 0 ;;
        *) fail "unknown argument: $1" ;;
    esac
    shift
done

if [ "$self_test" -eq 1 ]; then
    if ( assert_safe_home "/home/alice" >/dev/null 2>&1 ); then
        fail "self-test: /home/alice was accepted"
    fi
    ( assert_safe_home "/opt/corvus" >/dev/null 2>&1 ) || fail "self-test: /opt/corvus was rejected"
    ( assert_safe_home "/var/lib/corvus" >/dev/null 2>&1 ) || fail "self-test: /var/lib/corvus was rejected"
    echo "self-test passed: /home/alice is refused; /opt/corvus and /var/lib/corvus are allowed"
    exit 0
fi

[ "$(id -u)" -eq 0 ] || fail "run this script as root (EUID 0)"
command -v systemctl >/dev/null 2>&1 || fail "systemctl is required"
[ -d /run/systemd/system ] || fail "systemd is not running"

source_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
unit_file="/etc/systemd/system/corvus.service"

corvus_home="${CORVUS_HOME:-/opt/corvus}"
corvus_user="${CORVUS_USER:-corvus}"
corvus_ws_port="${CORVUS_WS_PORT:-7330}"
corvus_http_port="${CORVUS_HTTP_PORT:-7331}"

case "$corvus_home" in
    /*) ;;
    *) fail "CORVUS_HOME must be an absolute path" ;;
esac
corvus_home="${corvus_home%/}"
[ -n "$corvus_home" ] || fail "CORVUS_HOME must not be empty"
case "$corvus_home" in
    /|/bin|/boot|/dev|/etc|/home|/lib|/lib64|/media|/mnt|/opt|/proc|/root|/run|/sbin|/srv|/sys|/tmp|/usr|/var)
        fail "CORVUS_HOME must be a dedicated directory, not $corvus_home" ;;
esac
case "$corvus_home" in
    *[!A-Za-z0-9/._-]*) fail "CORVUS_HOME may contain only letters, digits, slash, dot, underscore and hyphen" ;;
esac
case "$corvus_home" in
    *..*) fail "CORVUS_HOME must not contain '..'" ;;
esac
assert_safe_home "$corvus_home"

case "$corvus_user" in
    ''|*[!a-z0-9_-]*) fail "CORVUS_USER must match [a-z_][a-z0-9_-]*" ;;
    [a-z_]*) ;;
    *) fail "CORVUS_USER must match [a-z_][a-z0-9_-]*" ;;
esac

validate_port() {
    local value="$1"
    local label="$2"
    case "$value" in
        ''|*[!0-9]*) fail "$label must be a number" ;;
    esac
    if [ "$value" -lt 1 ] || [ "$value" -gt 65535 ]; then
        fail "$label must be between 1 and 65535"
    fi
}
validate_port "$corvus_ws_port" "CORVUS_WS_PORT"
validate_port "$corvus_http_port" "CORVUS_HTTP_PORT"

for required in "$source_dir/bin/corvusd" "$source_dir/bin/corvus" "$source_dir/extension/manifest.json" "$source_dir/systemd/corvus.service"; do
    [ -f "$required" ] || fail "missing $required; run the script from the extracted package"
done

if ! getent group "$corvus_user" >/dev/null 2>&1; then
    groupadd --system "$corvus_user"
fi
if ! id -u "$corvus_user" >/dev/null 2>&1; then
    useradd --system --gid "$corvus_user" --home-dir "$corvus_home" --shell /usr/sbin/nologin --no-create-home "$corvus_user"
fi
corvus_group="$(id -gn "$corvus_user")"

systemctl stop corvus.service 2>/dev/null || true

install -d -m 0755 -o root -g root "$corvus_home"
chown root:root "$corvus_home"
chmod 0755 "$corvus_home"
install -d -m 0755 -o root -g root "$corvus_home/bin"
chown root:root "$corvus_home/bin"
chmod 0755 "$corvus_home/bin"
for directory in data logs config; do
    install -d -m 0750 -o "$corvus_user" -g "$corvus_group" "$corvus_home/$directory"
    chown "$corvus_user:$corvus_group" "$corvus_home/$directory"
    chmod 0750 "$corvus_home/$directory"
done

install -m 0755 -o root -g root "$source_dir/bin/corvusd" "$corvus_home/bin/corvusd"
install -m 0755 -o root -g root "$source_dir/bin/corvus" "$corvus_home/bin/corvus"

rm -rf "$corvus_home/extension"
install -d -m 0755 -o root -g root "$corvus_home/extension"
cp -R "$source_dir/extension/." "$corvus_home/extension/"
find "$corvus_home/extension" -type d -exec chmod 0755 {} +
find "$corvus_home/extension" -type f -exec chmod 0644 {} +
chown -R root:root "$corvus_home/extension"
[ -f "$corvus_home/extension/manifest.json" ] || fail "the extension copy has no manifest.json"

tmp_unit="$(mktemp)"
trap 'rm -f "$tmp_unit"' EXIT
sed \
    -e "s|@CORVUS_HOME@|$corvus_home|g" \
    -e "s|@CORVUS_USER@|$corvus_user|g" \
    -e "s|@CORVUS_GROUP@|$corvus_group|g" \
    -e "s|@CORVUS_WS_PORT@|$corvus_ws_port|g" \
    -e "s|@CORVUS_HTTP_PORT@|$corvus_http_port|g" \
    "$source_dir/systemd/corvus.service" > "$tmp_unit"
install -m 0644 -o root -g root "$tmp_unit" "$unit_file"
rm -f "$tmp_unit"
trap - EXIT

chown -R "$corvus_user:$corvus_group" "$corvus_home/data" "$corvus_home/logs" "$corvus_home/config"

systemctl daemon-reload
systemctl enable corvus.service >/dev/null
if ! systemctl restart corvus.service; then
    systemctl status --no-pager --full corvus.service || true
    fail "the corvus.service unit failed to start"
fi

if ! systemctl is-active --quiet corvus.service; then
    systemctl status --no-pager --full corvus.service || true
    fail "the corvus.service unit is not active after restart"
fi

health_url="http://127.0.0.1:${corvus_http_port}/health"
if command -v curl >/dev/null 2>&1; then
    health_ok=0
    attempt=0
    while [ "$attempt" -lt 30 ]; do
        if curl -fsS --max-time 2 "$health_url" >/dev/null 2>&1; then
            health_ok=1
            break
        fi
        attempt=$((attempt + 1))
        sleep 1
    done
    if [ "$health_ok" -ne 1 ]; then
        systemctl status --no-pager --full corvus.service || true
        fail "health check did not pass at $health_url"
    fi
    echo "Health check passed: $health_url"
else
    echo "WARNING: curl not found; skipped HTTP health check for $health_url" >&2
fi

echo "Corvus is installed and running."
echo "  home:      $corvus_home"
echo "  service:   corvus.service"
echo "  binaries:  $corvus_home/bin"
echo "  database:  $corvus_home/data/corvus.db"
echo "  extension: $corvus_home/extension"
echo "  logs:      journalctl -u corvus.service"
echo "  websocket: ws://127.0.0.1:$corvus_ws_port"
echo "  MCP:       http://127.0.0.1:$corvus_http_port/mcp"
echo
echo "Next steps:"
echo "  1. Load $corvus_home/extension as an unpacked extension in Chrome or Edge."
echo "  2. Verify with: curl -fsS http://127.0.0.1:$corvus_http_port/health"
echo "  3. Inspect status with: systemctl status corvus.service"
echo
echo "The unit binds the daemon to 127.0.0.1. Corvus has no authentication;"
echo "do not expose the ports to an untrusted network."
