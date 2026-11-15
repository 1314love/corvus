#!/usr/bin/env bash
#
# Build the Corvus Linux release archive under dist/.
#
# Builds corvusd and corvus for the native x86_64 Linux host, stages the two
# binaries, the browser extension, the Linux installers and the systemd unit,
# then writes VERSION and SHA256SUMS. The finished tar.gz is extracted again
# and every checksum is verified before the script succeeds.
#
# Usage: scripts/package-linux.sh
# Exit:  0 packaged, non-zero on any build, copy or checksum failure.

set -euo pipefail

fail() {
    echo "package-linux: $*" >&2
    exit 1
}

case "$(uname -s):$(uname -m)" in
    Linux:x86_64) ;;
    *) fail "this package must be built on x86_64 Linux" ;;
esac

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

dist_dir="$repo_root/dist"
cargo_toml="$repo_root/Cargo.toml"

[ -f "$cargo_toml" ] || fail "$cargo_toml is missing"
[ -f "$repo_root/Cargo.lock" ] || fail "$repo_root/Cargo.lock is missing"
[ -f "$repo_root/extension/manifest.json" ] || fail "extension/manifest.json is missing"
[ -f "$repo_root/packaging/systemd/corvus.service" ] || fail "packaging/systemd/corvus.service is missing"
[ -f "$repo_root/scripts/install-linux.sh" ] || fail "scripts/install-linux.sh is missing"
[ -f "$repo_root/scripts/uninstall-linux.sh" ] || fail "scripts/uninstall-linux.sh is missing"
command -v sha256sum >/dev/null 2>&1 || fail "sha256sum is required"
command -v tar >/dev/null 2>&1 || fail "tar is required"

if [ -n "${CARGO:-}" ]; then
    cargo_bin="${CARGO}"
else
    cargo_bin="$(command -v cargo || true)"
fi
[ -n "$cargo_bin" ] || fail "cargo was not found"

version="$(sed -n 's/^version = "\([^"]*\)"$/\1/p' "$cargo_toml" | head -n 1)"
case "$version" in
    ""|*[!0-9A-Za-z.+-]*) fail "cannot read a valid version from $cargo_toml" ;;
esac

target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"
stage="$dist_dir/linux-x86_64"
archive="$dist_dir/corvus-$version-linux-x86_64.tar.gz"

echo "Corvus Linux package"
echo "===================="
echo "version: $version"
echo "cargo:   $cargo_bin"
echo "stage:   $stage"

"$cargo_bin" build --release --locked -p corvusd -p corvus

[ -x "$target_dir/release/corvusd" ] || fail "the build did not produce $target_dir/release/corvusd"
[ -x "$target_dir/release/corvus" ] || fail "the build did not produce $target_dir/release/corvus"

rm -rf "$stage"
mkdir -p "$stage/bin" "$stage/extension" "$stage/systemd"

install -m 0755 "$target_dir/release/corvusd" "$stage/bin/corvusd"
install -m 0755 "$target_dir/release/corvus" "$stage/bin/corvus"
cp -R "$repo_root/extension/." "$stage/extension/"
install -m 0755 "$repo_root/scripts/install-linux.sh" "$stage/install-linux.sh"
install -m 0755 "$repo_root/scripts/uninstall-linux.sh" "$stage/uninstall-linux.sh"
install -m 0644 "$repo_root/packaging/systemd/corvus.service" "$stage/systemd/corvus.service"
printf '%s\n' "$version" > "$stage/VERSION"

[ -f "$stage/extension/manifest.json" ] || fail "the staged extension has no manifest.json"

files=(VERSION bin/corvusd bin/corvus install-linux.sh uninstall-linux.sh systemd/corvus.service)
while IFS= read -r rel; do
    files+=("$rel")
done < <(cd "$stage" && find extension -type f | LC_ALL=C sort)

: > "$stage/SHA256SUMS"
for rel in "${files[@]}"; do
    digest="$(sha256sum "$stage/$rel" | awk '{print $1}')"
    printf '%s  %s\n' "$digest" "$rel" >> "$stage/SHA256SUMS"
done

(
    cd "$stage"
    sha256sum -c SHA256SUMS
)

rm -f "$archive"
tar -czf "$archive" -C "$stage" VERSION SHA256SUMS bin extension install-linux.sh uninstall-linux.sh systemd
[ -s "$archive" ] || fail "$archive was not created or is empty"

archive_listing="$(mktemp)"
trap 'rm -f "$archive_listing"' EXIT
tar -tzf "$archive" > "$archive_listing"
for entry in VERSION SHA256SUMS bin/corvusd bin/corvus install-linux.sh uninstall-linux.sh systemd/corvus.service extension/manifest.json; do
    grep -qx "$entry" "$archive_listing" || fail "the archive is missing $entry"
done
rm -f "$archive_listing"
trap - EXIT

verify_dir="$(mktemp -d)"
trap 'rm -rf "$verify_dir" "$stage"' EXIT
tar -xzf "$archive" -C "$verify_dir"
(
    cd "$verify_dir"
    sha256sum -c SHA256SUMS
)
rm -rf "$verify_dir" "$stage"
trap - EXIT

archive_bytes="$(wc -c < "$archive" | tr -d ' ')"
echo
echo "artifacts:"
echo "  archive: $archive ($archive_bytes bytes)"
echo "  checksums: $(( ${#files[@]} )) entries, verified from the extracted archive"
echo "  version:   $version"
