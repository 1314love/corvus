#!/usr/bin/env bash
#
# Build a Corvus release package under dist/.
#
# Runs the release build, then copies the daemon and CLI binaries, the browser
# extension, a VERSION file and a SHA256SUMS manifest into dist/. The build is
# offline: the local cargo cache is the only dependency source.
#
# Usage: scripts/package.sh
# Exit:  0 packaged, non-zero on any build, copy or checksum failure.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

dist_dir="$repo_root/dist"
cargo_toml="$repo_root/Cargo.toml"

echo "Corvus package"
echo "=============="

if [ ! -f "$cargo_toml" ]; then
    echo "error: $cargo_toml is missing" >&2
    exit 1
fi

version="$(sed -n 's/^version = "\([^"]*\)"$/\1/p' "$cargo_toml" | head -n 1)"
case "$version" in
    ""|*[!0-9A-Za-z.+-]*)
        echo "error: cannot read a valid version from $cargo_toml" >&2
        exit 1
        ;;
esac

echo "version: $version"
echo "dist:    $dist_dir"

bash "$repo_root/scripts/build.sh"

exe_suffix=""
case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) exe_suffix=".exe" ;;
esac

for name in corvusd corvus; do
    if [ ! -f "$dist_dir/$name$exe_suffix" ]; then
        echo "error: dist/$name$exe_suffix is missing after the build" >&2
        exit 1
    fi
done

if [ ! -f "$repo_root/extension/manifest.json" ]; then
    echo "error: extension/manifest.json is missing" >&2
    exit 1
fi

rm -rf "$dist_dir/extension"
cp -R "$repo_root/extension" "$dist_dir/extension"

printf '%s\n' "$version" > "$dist_dir/VERSION"

hash_file() {
    local file="$1"
    local digest=""

    if command -v sha256sum >/dev/null 2>&1; then
        digest="$(sha256sum "$file" | awk '{print $1}')"
    elif command -v certutil >/dev/null 2>&1; then
        digest="$(certutil -hashfile "$file" SHA256 2>/dev/null | sed -n '2p' | tr -d ' \r')"
    else
        echo "error: neither sha256sum nor certutil is available for checksums" >&2
        return 1
    fi

    case "$digest" in
        ""|*[!0-9a-fA-F]*)
            echo "error: could not compute a SHA-256 for $file" >&2
            return 1
            ;;
    esac

    if [ "${#digest}" -ne 64 ]; then
        echo "error: unexpected SHA-256 length for $file: $digest" >&2
        return 1
    fi

    printf '%s\n' "$digest" | tr 'A-F' 'a-f'
}

sums_file="$dist_dir/SHA256SUMS"
: > "$sums_file"

files=("corvusd$exe_suffix" "corvus$exe_suffix" "VERSION")
while IFS= read -r rel; do
    files+=("$rel")
done < <(cd "$dist_dir" && find extension -type f | LC_ALL=C sort)

for rel in "${files[@]}"; do
    digest="$(hash_file "$dist_dir/$rel")"
    printf '%s  %s\n' "$digest" "$rel" >> "$sums_file"
done

zip_name="corvus-$version-windows-x64.zip"
zip_path="$dist_dir/$zip_name"
rm -f "$zip_path"

to_win_path() {
    local input="$1"

    if command -v cygpath >/dev/null 2>&1; then
        cygpath -w "$input"
    else
        local parent
        parent="$(cd "$(dirname "$input")" && pwd -W)"
        printf '%s/%s\n' "$parent" "$(basename "$input")"
    fi
}

if command -v pwsh >/dev/null 2>&1; then
    ps_command="pwsh"
elif command -v powershell.exe >/dev/null 2>&1; then
    ps_command="powershell.exe"
else
    echo "error: PowerShell is required to create $zip_name" >&2
    exit 1
fi

dist_win="$(to_win_path "$dist_dir")"
zip_win="$(to_win_path "$zip_path")"

CORVUS_DIST_DIR="$dist_win" CORVUS_ZIP_PATH="$zip_win" "$ps_command" -NoProfile -NonInteractive -Command "
  \$ErrorActionPreference = 'Stop';
  \$dist = \$env:CORVUS_DIST_DIR;
  \$zipPath = \$env:CORVUS_ZIP_PATH;
  Remove-Item -LiteralPath \$zipPath -Force -ErrorAction SilentlyContinue;
  \$items = @(
    (Join-Path \$dist 'corvusd.exe'),
    (Join-Path \$dist 'corvus.exe'),
    (Join-Path \$dist 'VERSION'),
    (Join-Path \$dist 'SHA256SUMS'),
    (Join-Path \$dist 'extension')
  );
  Compress-Archive -LiteralPath \$items -DestinationPath \$zipPath -CompressionLevel Optimal -Force
"

if [ ! -f "$zip_path" ] || [ ! -s "$zip_path" ]; then
    echo "error: $zip_name was not created or is empty" >&2
    exit 1
fi

zip_bytes="$(wc -c < "$zip_path" | tr -d ' ')"

echo
echo "artifacts:"
for rel in "${files[@]}"; do
    bytes="$(wc -c < "$dist_dir/$rel" | tr -d ' ')"
    printf '  %-32s %10s bytes\n' "$rel" "$bytes"
done
echo "checksums: dist/SHA256SUMS"
echo "version:   dist/VERSION"
echo "archive:   dist/$zip_name ($zip_bytes bytes)"
echo
echo "verify: (cd dist && sha256sum -c SHA256SUMS)"
