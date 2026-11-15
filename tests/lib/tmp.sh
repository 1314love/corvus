# Test artifact location policy (source this file).
#
# Every test artifact goes below <tmp>/corvus-tests, never inside the repository
# and never into the directory a command was started in. <tmp> is
# CORVUS_TEST_TMP, then TMPDIR, then TEMP/TMP (%TEMP%/%TMP% on Windows), then
# /tmp.
#
#   . "$(dirname "$0")/../lib/tmp.sh"
#   TMPROOT=$(corvus_tmp_root) || exit 1
#   TMP=$(mktemp -d "$TMPROOT/corvus-perf.XXXXXX") || exit 1

corvus_tmp_base() {
  if [ -n "${CORVUS_TEST_TMP:-}" ]; then
    printf '%s\n' "$CORVUS_TEST_TMP"
    return 0
  fi
  if [ -n "${TMPDIR:-}" ]; then
    printf '%s\n' "$TMPDIR"
    return 0
  fi
  _corvus_tmp=${TEMP:-${TMP:-}}
  if [ -n "$_corvus_tmp" ]; then
    if command -v cygpath >/dev/null 2>&1; then
      cygpath -u "$_corvus_tmp"
    else
      printf '%s\n' "$_corvus_tmp"
    fi
    return 0
  fi
  printf '%s\n' /tmp
}

corvus_tmp_root() {
  _corvus_base=$(corvus_tmp_base) || return 1
  _corvus_root="$_corvus_base/corvus-tests"
  mkdir -p "$_corvus_root" || return 1
  ( cd "$_corvus_root" && pwd -P )
}
