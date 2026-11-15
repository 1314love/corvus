#!/usr/bin/env bash
#
# Build the Corvus release binaries and checksum them.
#
# Writes dist/corvusd[.exe], dist/corvus[.exe] and dist/SHA256SUMS. The build is
# offline by default; set CORVUS_CARGO_OFFLINE=0 to let Cargo use the network on
# a fresh CI runner.
# Safe to re-run; exits non-zero if the build, a copy or a checksum fails.
#
# Usage: scripts/build.sh
# Environment: CORVUS_CARGO_OFFLINE=0 enables network access (default is offline).

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

dist_dir="$repo_root/dist"
target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"

find_cargo() {
    if command -v cargo >/dev/null 2>&1; then
        command -v cargo
        return 0
    fi

    local candidates=(
        "${CARGO_HOME:-}/bin/cargo.exe"
        "${HOME:-}/.cargo/bin/cargo.exe"
        "${CARGO_HOME:-}/bin/cargo"
        "${HOME:-}/.cargo/bin/cargo"
    )
    local candidate
    for candidate in "${candidates[@]}"; do
        case "$candidate" in
            /bin/*) continue ;;
        esac
        if [ -x "$candidate" ]; then
            echo "$candidate"
            return 0
        fi
    done
    return 1
}

if ! cargo_bin="$(find_cargo)"; then
    echo "error: cargo was not found; run scripts/dev-setup.sh for details." >&2
    exit 1
fi

exe_suffix=""
case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) exe_suffix=".exe" ;;
esac

echo "cargo:  $cargo_bin"
echo "target: $target_dir"

offline_args=()
if [ "${CORVUS_CARGO_OFFLINE:-1}" != "0" ]; then
    offline_args=(--offline)
fi

"$cargo_bin" build --release "${offline_args[@]}" -p corvusd -p corvus

mkdir -p "$dist_dir"

binaries=()
for name in corvusd corvus; do
    built_bin="$target_dir/release/$name$exe_suffix"
    if [ ! -f "$built_bin" ]; then
        echo "error: the build did not produce $built_bin" >&2
        exit 1
    fi
    cp -f "$built_bin" "$dist_dir/$name$exe_suffix"
    binaries+=("$name$exe_suffix")
    echo "binary: dist/$name$exe_suffix"
done

sums_file="$dist_dir/SHA256SUMS"
: > "$sums_file"

for file_name in "${binaries[@]}"; do
    file_path="$dist_dir/$file_name"
    if command -v sha256sum >/dev/null 2>&1; then
        hash_line="$(sha256sum "$file_path")"
        hash="${hash_line%% *}"
    elif command -v certutil >/dev/null 2>&1; then
        hash="$(certutil -hashfile "$file_path" SHA256 2>/dev/null | sed -n "2p" | tr -d " \r")"
    else
        echo "error: neither sha256sum nor certutil is available for checksums" >&2
        exit 1
    fi
    case "$hash" in
        ""|*[!0-9a-fA-F]*)
            echo "error: could not compute a SHA-256 for $file_name" >&2
            exit 1
            ;;
    esac
    if [ "${#hash}" -ne 64 ]; then
        echo "error: unexpected SHA-256 length for $file_name: $hash" >&2
        exit 1
    fi
    echo "$hash  $file_name" >> "$sums_file"
done

echo "checksums: dist/SHA256SUMS"
echo "verify:    (cd dist && sha256sum -c SHA256SUMS)"
