#!/usr/bin/env bash
set -euo pipefail

readonly repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
readonly delivery="$repo_root/scripts/dse-delivery.sh"
readonly release_tool="$repo_root/scripts/dse-release.py"
readonly test_root="$(mktemp -d "${TMPDIR:-/tmp}/dse-installer-test.XXXXXX")"

cleanup() {
  chmod -R u+w "$test_root" 2>/dev/null || true
  rm -rf "$test_root"
}
trap cleanup EXIT

fail() {
  printf 'installer self-test: %s\n' "$*" >&2
  exit 1
}

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

assert_rejected() {
  local label="$1"
  shift
  if "$@" >"$test_root/rejected.stdout" 2>"$test_root/rejected.stderr"; then
    fail "$label unexpectedly succeeded"
  fi
}

make_fixture_binaries() {
  local directory="$1"
  local version="$2"
  mkdir -p "$directory"
  cat >"$directory/dse" <<EOF
#!/bin/sh
case "\${1:-}" in
  --version) printf '%s\n' 'dse $version (fixture)' ;;
  doctor)
    [ "\${2:-}" = "--json" ] || exit 2
    [ "\${DSE_FIXTURE_DOCTOR_FAIL:-0}" = "0" ] || exit 9
    printf '%s\n' '{"status":"fixture-ok","version":"$version"}'
    ;;
  *) printf '%s\n' 'dse fixture $version' ;;
esac
EOF
  cat >"$directory/dse-tui" <<EOF
#!/bin/sh
if [ "\${1:-}" = "--version" ]; then
  printf '%s\n' 'dse-tui $version (fixture)'
else
  printf '%s\n' 'dse-tui fixture $version'
fi
EOF
  chmod 0755 "$directory/dse" "$directory/dse-tui"
}

make_release() {
  local version="$1"
  local revision="$2"
  local tree="$3"
  local output="$4"
  local binaries="$test_root/bin-$version"
  mkdir -p "$output"
  make_fixture_binaries "$binaries" "$version"
  for target in \
    aarch64-apple-darwin \
    x86_64-apple-darwin \
    aarch64-unknown-linux-gnu \
    x86_64-unknown-linux-gnu; do
    "$delivery" package \
      --output-dir "$output" \
      --binary-dir "$binaries" \
      --version "$version" \
      --revision "$revision" \
      --source-tree "$tree" \
      --target "$target" \
      --release-asset >/dev/null
  done
  "$release_tool" assemble \
    --dist-dir "$output" \
    --version "$version" \
    --revision "$revision" \
    --tree "$tree" \
    --created 2026-07-29T00:00:00Z >/dev/null
}

make_tool_path() {
  local directory="$1"
  mkdir -p "$directory"
  for tool in \
    awk bash basename cat chmod cp dd dirname grep gzip ln mkdir mktemp readlink rm rmdir \
    sed shasum tar tr wc; do
    tool_path="$(command -v "$tool")"
    ln -s "$tool_path" "$directory/$tool"
  done

  cat >"$directory/uname" <<'EOF'
#!/bin/sh
case "${1:-}" in
  -s) printf '%s\n' "${DSE_FAKE_UNAME_S:-Darwin}" ;;
  -m) printf '%s\n' "${DSE_FAKE_UNAME_M:-arm64}" ;;
  *) exec /usr/bin/uname "$@" ;;
esac
EOF
  chmod 0755 "$directory/uname"

cat >"$directory/mv" <<'EOF'
#!/bin/sh
# Map simulated target flags onto the real fixture host without changing the
# production delivery owner's atomic replacement semantics.
case "${1:-}" in
  -Tf)
    shift
    if [ "$(/usr/bin/uname -s)" = "Darwin" ]; then
      exec /bin/mv -f "$@"
    fi
    exec /bin/mv -Tf "$@"
    ;;
  -fh)
    shift
    if [ "$(/usr/bin/uname -s)" = "Darwin" ]; then
      exec /bin/mv -fh "$@"
    fi
    exec /bin/mv -Tf "$@"
    ;;
  *) exec /bin/mv "$@" ;;
esac
EOF
  chmod 0755 "$directory/mv"

  cat >"$directory/curl" <<'EOF'
#!/bin/sh
set -eu
output=""
write_format=""
url=""
retry_all_errors=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o)
      output="$2"
      shift 2
      ;;
    -w)
      write_format="$2"
      shift 2
      ;;
    --proto | --proto-redir | --connect-timeout | --max-time | --retry | --retry-delay | --retry-max-time)
      shift 2
      ;;
    --retry-all-errors)
      retry_all_errors=1
      shift
      ;;
    --tlsv1.2 | --retry-connrefused | -fsSL)
      shift
      ;;
    *)
      url="$1"
      shift
      ;;
  esac
done
[ -n "$url" ] || exit 2
[ "$retry_all_errors" = "1" ] || exit 91
if [ -n "$write_format" ]; then
  case "${DSE_FAKE_CURL_MODE:-ok}" in
    latest-none)
      printf '%s' 'https://github.com/gelin66/deepseek-agent/releases'
      exit 0
      ;;
  esac
  printf '%s' "https://github.com/gelin66/deepseek-agent/releases/tag/${DSE_FAKE_RELEASE_TAG}"
  exit 0
fi
asset="${url##*/}"
case "${DSE_FAKE_CURL_MODE:-ok}:$asset" in
  404-manifest:dist-manifest.json) exit 22 ;;
  timeout-archive:*.tar.gz) exit 28 ;;
  reset-archive:*.tar.gz) exit 56 ;;
  partial-archive:*.tar.gz)
    dd if="$DSE_FAKE_RELEASE_DIR/$asset" of="$output" bs=1 count=128 2>/dev/null
    exit 18
    ;;
esac
[ -f "$DSE_FAKE_RELEASE_DIR/$asset" ] || exit 22
cp "$DSE_FAKE_RELEASE_DIR/$asset" "$output"
EOF
  chmod 0755 "$directory/curl"
}

run_installer() {
  local installer="$1"
  shift
  env \
    PATH="$tool_path_root" \
    DSE_FAKE_RELEASE_DIR="$active_release" \
    DSE_FAKE_RELEASE_TAG="$active_tag" \
    DSE_RELEASE_TAG="$active_tag" \
    DSE_RELEASE_MANIFEST="$active_release/dist-manifest.json" \
    /bin/sh "$installer" "$@"
}

rev1="1111111111111111111111111111111111111111"
tree1="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
rev2="2222222222222222222222222222222222222222"
tree2="bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
release1="$test_root/release-1"
release2="$test_root/release-2"
make_release 1.0.0 "$rev1" "$tree1" "$release1"
make_release 2.0.0 "$rev2" "$tree2" "$release2"

tool_path_root="$test_root/tools"
make_tool_path "$tool_path_root"
[ ! -e "$tool_path_root/python" ] && [ ! -e "$tool_path_root/cargo" ] &&
  [ ! -e "$tool_path_root/node" ] && [ ! -e "$tool_path_root/npm" ] ||
  fail "cold-install tool path contains a forbidden runtime"

home="$test_root/home"
prefix="$home/.local"
dse_home="$home/.dse"
mkdir -p "$home" "$dse_home"
printf 'preserve-config\n' >"$dse_home/config.toml"
printf 'preserve-store\n' >"$dse_home/runs.sqlite3"
printf 'preserve-secret\n' >"$dse_home/credentials"
printf 'preserve-profile\n' >"$home/.profile"
data_before="$test_root/data-before"
for file in config.toml runs.sqlite3 credentials; do
  printf '%s  %s\n' "$(sha256_file "$dse_home/$file")" "$file"
done >"$data_before"
profile_before="$(sha256_file "$home/.profile")"
export HOME="$home" DSE_HOME="$dse_home"

active_release="$release1"
active_tag="v1.0.0"

# All four public target mappings must select and install the matching archive.
while IFS=' ' read -r fake_os fake_arch expected_target; do
  matrix_prefix="$test_root/matrix-$expected_target"
  env \
    PATH="$tool_path_root" \
    DSE_FAKE_UNAME_S="$fake_os" \
    DSE_FAKE_UNAME_M="$fake_arch" \
    DSE_FAKE_RELEASE_DIR="$release1" \
    DSE_FAKE_RELEASE_TAG="$active_tag" \
    DSE_RELEASE_TAG="$active_tag" \
    DSE_RELEASE_MANIFEST="$release1/dist-manifest.json" \
    /bin/sh "$release1/dse-installer.sh" --prefix "$matrix_prefix" --no-modify-path
  [ "$(awk -F '\t' '$1 == "target" { print $2 }' \
    "$matrix_prefix/lib/dse/current/manifest.tsv")" = "$expected_target" ] ||
    fail "target mapping selected the wrong archive for $fake_os $fake_arch"
  env \
    PATH="$tool_path_root" \
    DSE_FAKE_UNAME_S="$fake_os" \
    DSE_FAKE_UNAME_M="$fake_arch" \
    /bin/sh "$release1/dse-installer.sh" --prefix "$matrix_prefix" --uninstall >/dev/null
done <<'EOF'
Darwin arm64 aarch64-apple-darwin
Darwin x86_64 x86_64-apple-darwin
Linux aarch64 aarch64-unknown-linux-gnu
Linux x86_64 x86_64-unknown-linux-gnu
EOF

run_installer "$release1/dse-installer.sh" --no-modify-path
[ "$("$prefix/bin/dse" --version)" = "dse 1.0.0 (fixture)" ] ||
  fail "fresh install did not activate v1"
"$prefix/bin/dse" doctor --json >/dev/null
current_v1="$(readlink "$prefix/lib/dse/current")"
[ "$(sha256_file "$home/.profile")" = "$profile_before" ] ||
  fail "installer modified the shell profile"

run_installer "$release1/dse-installer.sh"
[ "$(readlink "$prefix/lib/dse/current")" = "$current_v1" ] ||
  fail "same-version install was not idempotent"
[ ! -L "$prefix/lib/dse/previous" ] ||
  fail "same-version install incorrectly created previous"

active_release="$release2"
active_tag="v2.0.0"
run_installer "$release2/dse-installer.sh" --version 2.0.0
[ "$("$prefix/bin/dse" --version)" = "dse 2.0.0 (fixture)" ] ||
  fail "upgrade did not activate v2"
[ -L "$prefix/lib/dse/previous" ] || fail "upgrade did not retain previous"

run_installer "$release2/dse-installer.sh" --rollback
[ "$("$prefix/bin/dse" --version)" = "dse 1.0.0 (fixture)" ] ||
  fail "rollback did not reactivate v1"
run_installer "$release2/dse-installer.sh" --rollback
[ "$("$prefix/bin/dse" --version)" = "dse 2.0.0 (fixture)" ] ||
  fail "second rollback did not reactivate v2"
previous_before_failure="$(readlink "$prefix/lib/dse/previous")"

# Network and integrity failures must preserve the active v2 release.
for mode in 404-manifest timeout-archive reset-archive partial-archive; do
  assert_rejected "$mode" env \
    PATH="$tool_path_root" \
    DSE_FAKE_CURL_MODE="$mode" \
    DSE_FAKE_RELEASE_DIR="$release2" \
    DSE_FAKE_RELEASE_TAG="v2.0.0" \
    DSE_RELEASE_TAG="v2.0.0" \
    /bin/sh "$release2/dse-installer.sh" --prefix "$prefix"
  case "$mode" in
    404-manifest)
      grep -F "release v2.0.0 is missing dist-manifest.json" \
        "$test_root/rejected.stderr" >/dev/null ||
        fail "$mode did not report a missing release asset"
      ;;
    timeout-archive)
      grep -F "download for dse-2.0.0-aarch64-apple-darwin.tar.gz timed out" \
        "$test_root/rejected.stderr" >/dev/null ||
        fail "$mode did not report a timeout"
      ;;
    reset-archive)
      grep -F "download for dse-2.0.0-aarch64-apple-darwin.tar.gz failed during HTTPS transport" \
        "$test_root/rejected.stderr" >/dev/null ||
        fail "$mode did not report an HTTPS transport failure"
      ;;
    partial-archive)
      grep -F "download for dse-2.0.0-aarch64-apple-darwin.tar.gz was partial" \
        "$test_root/rejected.stderr" >/dev/null ||
        fail "$mode did not report a partial response"
      ;;
  esac
  [ "$("$prefix/bin/dse" --version)" = "dse 2.0.0 (fixture)" ] ||
    fail "$mode changed the active release"
done

tampered_release="$test_root/tampered-release"
mkdir -p "$tampered_release"
for asset in dse-installer.sh dist-manifest.json SHA256SUMS SBOM.spdx.json \
  dse-2.0.0-aarch64-apple-darwin.tar.gz \
  dse-2.0.0-x86_64-apple-darwin.tar.gz \
  dse-2.0.0-aarch64-unknown-linux-gnu.tar.gz \
  dse-2.0.0-x86_64-unknown-linux-gnu.tar.gz; do
  cp "$release2/$asset" "$tampered_release/$asset"
done
printf 'tampered\n' >>"$tampered_release/dse-2.0.0-aarch64-apple-darwin.tar.gz"
assert_rejected "tampered archive" env \
  PATH="$tool_path_root" \
  DSE_FAKE_RELEASE_DIR="$tampered_release" \
  DSE_FAKE_RELEASE_TAG="v2.0.0" \
  DSE_RELEASE_TAG="v2.0.0" \
  DSE_RELEASE_MANIFEST="$tampered_release/dist-manifest.json" \
  /bin/sh "$tampered_release/dse-installer.sh" --prefix "$prefix"
[ "$("$prefix/bin/dse" --version)" = "dse 2.0.0 (fixture)" ] ||
  fail "tampered archive changed the active release"

# A post-install smoke failure must restore the exact previous release.
active_release="$release1"
active_tag="v1.0.0"
assert_rejected "doctor smoke failure" env \
  PATH="$tool_path_root" \
  DSE_FIXTURE_DOCTOR_FAIL=1 \
  DSE_FAKE_RELEASE_DIR="$release1" \
  DSE_FAKE_RELEASE_TAG="$active_tag" \
  DSE_RELEASE_TAG="$active_tag" \
  DSE_RELEASE_MANIFEST="$release1/dist-manifest.json" \
  /bin/sh "$release1/dse-installer.sh" --prefix "$prefix"
[ "$("$prefix/bin/dse" --version)" = "dse 2.0.0 (fixture)" ] ||
  fail "doctor smoke failure did not restore v2"
[ "$(readlink "$prefix/lib/dse/previous")" = "$previous_before_failure" ] ||
  fail "doctor smoke failure changed the previous release slot"

fresh_failure_prefix="$test_root/fresh-smoke-failure"
assert_rejected "fresh doctor smoke failure" env \
  PATH="$tool_path_root" \
  DSE_FIXTURE_DOCTOR_FAIL=1 \
  DSE_FAKE_RELEASE_DIR="$release1" \
  DSE_FAKE_RELEASE_TAG="v1.0.0" \
  DSE_RELEASE_TAG="v1.0.0" \
  DSE_RELEASE_MANIFEST="$release1/dist-manifest.json" \
  /bin/sh "$release1/dse-installer.sh" --prefix "$fresh_failure_prefix"
[ ! -e "$fresh_failure_prefix" ] ||
  fail "fresh doctor smoke failure mutated the install prefix"

foreign="$test_root/foreign"
mkdir -p "$foreign"
printf '#!/bin/sh\nexit 0\n' >"$foreign/dse"
chmod 0755 "$foreign/dse"
assert_rejected "foreign dse" env \
  PATH="$foreign:$tool_path_root" \
  DSE_FAKE_RELEASE_DIR="$release2" \
  DSE_FAKE_RELEASE_TAG="v2.0.0" \
  DSE_RELEASE_TAG="v2.0.0" \
  /bin/sh "$release2/dse-installer.sh" --prefix "$test_root/foreign-prefix"
[ ! -e "$test_root/foreign-prefix" ] || fail "foreign-manager rejection changed prefix"

assert_rejected "unsupported platform" env \
  PATH="$tool_path_root" \
  DSE_FAKE_UNAME_S=Plan9 \
  DSE_FAKE_UNAME_M=mips \
  DSE_FAKE_RELEASE_DIR="$release2" \
  DSE_FAKE_RELEASE_TAG="v2.0.0" \
  DSE_RELEASE_TAG="v2.0.0" \
  /bin/sh "$release2/dse-installer.sh" --prefix "$test_root/unsupported-prefix"
[ ! -e "$test_root/unsupported-prefix" ] || fail "unsupported target changed prefix"

no_gzip_path="$test_root/tools-no-gzip"
mkdir -p "$no_gzip_path"
for tool_path in "$tool_path_root"/*; do
  [ "${tool_path##*/}" = "gzip" ] ||
    ln -s "$tool_path" "$no_gzip_path/${tool_path##*/}"
done
assert_rejected "missing gzip" env \
  PATH="$no_gzip_path" \
  DSE_FAKE_RELEASE_DIR="$release2" \
  DSE_FAKE_RELEASE_TAG="v2.0.0" \
  DSE_RELEASE_TAG="v2.0.0" \
  /bin/sh "$release2/dse-installer.sh" --prefix "$test_root/no-gzip-prefix"
[ ! -e "$test_root/no-gzip-prefix" ] || fail "missing gzip changed prefix"

run_installer "$release2/dse-installer.sh" --verify >/dev/null
run_installer "$release2/dse-installer.sh" --uninstall
[ ! -e "$prefix/bin/dse" ] && [ ! -L "$prefix/bin/dse" ] ||
  fail "uninstall left dse"
[ ! -e "$prefix/lib/dse" ] || fail "uninstall left delivery metadata"

data_after="$test_root/data-after"
for file in config.toml runs.sqlite3 credentials; do
  printf '%s  %s\n' "$(sha256_file "$dse_home/$file")" "$file"
done >"$data_after"
cmp "$data_before" "$data_after" >/dev/null ||
  fail "install lifecycle changed DSE_HOME data"
[ "$(sha256_file "$home/.profile")" = "$profile_before" ] ||
  fail "install lifecycle changed the shell profile"

printf 'installer self-test: PASS (POSIX bootstrap + canonical delivery)\n'
