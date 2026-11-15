#!/usr/bin/env bash
#
# Check the local Corvus development environment.
#
# Finds the Rust toolchain (PATH first, then the usual rustup locations, which
# are not on PATH in the MSYS shell used here), prints the versions the build
# uses, and proves that the workspace resolves from the local cargo cache with
# "cargo metadata --offline".
#
# Usage: scripts/dev-setup.sh
# Exit:  0 ready, 1 toolchain missing or incomplete, 2 offline resolution failed.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

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

component_version() {
    local component="$1"
    local subcommand="$1"
    local output
    case "$component" in
        rustfmt) subcommand="fmt" ;;
    esac
    if ! output="$("$cargo_bin" "$subcommand" --version 2>&1)"; then
        echo "error: the $component component is missing." >&2
        echo "Fix: rustup component add $component" >&2
        exit 1
    fi
    echo "$output"
}

echo "Corvus dev setup"
echo "================"

if ! cargo_bin="$(find_cargo)"; then
    echo "error: cargo was not found." >&2
    echo "Install the Rust toolchain (https://rustup.rs) or put cargo on PATH." >&2
    echo "The script also checks \$CARGO_HOME/bin and \$HOME/.cargo/bin." >&2
    exit 1
fi

cargo_version="$("$cargo_bin" --version)"
echo "cargo:   $cargo_bin"
echo "         $cargo_version"

toolchain_bin="$(dirname "$cargo_bin")"
rustc_bin=""
for candidate in "$toolchain_bin/rustc.exe" "$toolchain_bin/rustc"; do
    if [ -x "$candidate" ]; then
        rustc_bin="$candidate"
        break
    fi
done
if [ -z "$rustc_bin" ] && command -v rustc >/dev/null 2>&1; then
    rustc_bin="$(command -v rustc)"
fi
if [ -z "$rustc_bin" ]; then
    echo "error: rustc was not found next to cargo; the toolchain is incomplete." >&2
    exit 1
fi
rustc_version="$("$rustc_bin" --version)"
echo "rustc:   $rustc_bin"
echo "         $rustc_version"

rustfmt_version="$(component_version rustfmt)"
clippy_version="$(component_version clippy)"
echo "rustfmt: $rustfmt_version"
echo "clippy:  $clippy_version"

echo "workspace: $repo_root"
if ! metadata_error="$("$cargo_bin" metadata --offline --format-version 1 2>&1 >/dev/null)"; then
    echo >&2
    echo "error: the workspace does not resolve from the local cargo cache." >&2
    echo >&2
    echo "cargo metadata --offline failed. The usual causes are a missing Cargo.lock" >&2
    echo "entry or a dependency that has never been fetched on this machine. Fetch it" >&2
    echo "while the registry is reachable, then re-run this script." >&2
    echo >&2
    echo "cargo said:" >&2
    echo "$metadata_error" >&2
    exit 2
fi
echo "offline resolution: ok"
echo
echo "ready: run scripts/build.sh to produce release binaries."
