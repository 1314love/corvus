#!/usr/bin/env bash
#
# Remove the Corvus systemd service.
#
# The unit and installed programs are removed. Data, logs and config are kept
# unless --purge is given; --purge removes the whole CORVUS_HOME root and the
# service user. The script is idempotent. Unsafe roots such as /home/alice are
# refused before any change; --self-test checks that guard without changing
# anything.
#
# Usage: sudo scripts/uninstall-linux.sh [--purge]
# Environment overrides: CORVUS_HOME, CORVUS_USER.

set -euo pipefail

fail() {
    echo "uninstall-linux: $*" >&2
    exit 1
}

usage() {
    cat <<'EOF'
Usage: sudo scripts/uninstall-linux.sh [--purge|--self-test]

  --purge      also remove CORVUS_HOME and the service user
  --self-test  verify the CORVUS_HOME safety guard without changing anything

Environment:
  CORVUS_HOME  installation root (default /opt/corvus)
  CORVUS_USER  service user and group (default corvus)
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
        --purge) purge=1 ;;
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

corvus_home="${CORVUS_HOME:-/opt/corvus}"
corvus_user="${CORVUS_USER:-corvus}"
unit_file="/etc/systemd/system/corvus.service"

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

if command -v systemctl >/dev/null 2>&1; then
    systemctl stop corvus.service 2>/dev/null || true
    systemctl disable corvus.service 2>/dev/null || true
fi

rm -f "$unit_file"

if command -v systemctl >/dev/null 2>&1; then
    systemctl daemon-reload 2>/dev/null || true
    systemctl reset-failed corvus.service 2>/dev/null || true
fi

if [ "$purge" -eq 1 ]; then
    if [ -e "$corvus_home" ]; then
        rm -rf "$corvus_home"
        echo "Removed $corvus_home"
    else
        echo "$corvus_home does not exist."
    fi
    if id -u "$corvus_user" >/dev/null 2>&1; then
        userdel "$corvus_user" 2>/dev/null || echo "Warning: could not delete user $corvus_user" >&2
    fi
    if getent group "$corvus_user" >/dev/null 2>&1; then
        groupdel "$corvus_user" 2>/dev/null || echo "Warning: could not delete group $corvus_user" >&2
    fi
    echo "Purged $corvus_home and the Corvus service user."
else
    rm -f "$corvus_home/bin/corvusd" "$corvus_home/bin/corvus"
    rm -rf "$corvus_home/extension"
    rmdir "$corvus_home/bin" 2>/dev/null || true
    echo "Removed the Corvus programs and extension from $corvus_home."
    echo "Data and logs are kept under $corvus_home (use --purge to remove them)."
fi

echo "Uninstall complete."
