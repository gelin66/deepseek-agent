#!/usr/bin/env bash
set -euo pipefail

readonly repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
readonly delivery="$repo_root/scripts/dse-delivery.sh"
readonly test_root="$(mktemp -d "${TMPDIR:-/tmp}/dse-delivery-test.XXXXXX")"

cleanup() {
  chmod -R u+w "$test_root" 2>/dev/null || true
  rm -rf "$test_root"
}
trap cleanup EXIT

fail() {
  printf 'delivery self-test: %s\n' "$*" >&2
  exit 1
}

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

make_fixture_binaries() {
  local directory="$1"
  local version="$2"
  mkdir -p "$directory"
  for binary in dse dse-tui; do
    {
      printf '#!/bin/sh\n'
      printf 'if [ "${1:-}" = "--version" ]; then\n'
      printf '  printf "%s %s\\n"\n' "$binary" "$version"
      printf 'else\n'
      printf '  printf "%s fixture %s\\n"\n' "$binary" "$version"
      printf 'fi\n'
    } >"$directory/$binary"
    chmod 0755 "$directory/$binary"
  done
}

assert_rejected() {
  local label="$1"
  shift
  if "$@" >"$test_root/rejected.stdout" 2>"$test_root/rejected.stderr"; then
    fail "$label unexpectedly succeeded"
  fi
}

target="$("$delivery" host-target)"
prefix="$test_root/prefix"
artifacts="$test_root/artifacts"
home="$test_root/home"
mkdir -p "$artifacts" "$home"
printf 'preserve-me\n' >"$home/config.toml"
home_before="$(sha256_file "$home/config.toml")"
export DSE_HOME="$home"

rev1="1111111111111111111111111111111111111111"
tree1="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
rev2="2222222222222222222222222222222222222222"
tree2="bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
make_fixture_binaries "$test_root/v1-bin" "1.0.0"
make_fixture_binaries "$test_root/v2-bin" "2.0.0"

artifact_v1="$(
  "$delivery" package \
    --output-dir "$artifacts/v1" \
    --binary-dir "$test_root/v1-bin" \
    --version 1.0.0 \
    --revision "$rev1" \
    --source-tree "$tree1" \
    --target "$target"
)"
artifact_v1_repeat="$(
  "$delivery" package \
    --output-dir "$artifacts/v1-repeat" \
    --binary-dir "$test_root/v1-bin" \
    --version 1.0.0 \
    --revision "$rev1" \
    --source-tree "$tree1" \
    --target "$target"
)"
artifact_v2="$(
  "$delivery" package \
    --output-dir "$artifacts/v2" \
    --binary-dir "$test_root/v2-bin" \
    --version 2.0.0 \
    --revision "$rev2" \
    --source-tree "$tree2" \
    --target "$target"
)"
[ "$(sha256_file "$artifact_v1")" = "$(sha256_file "$artifact_v1_repeat")" ] ||
  fail "same identity and binaries did not reproduce the same archive"

"$delivery" install --artifact "$artifact_v1" --prefix "$prefix"
"$delivery" verify --prefix "$prefix"
[ "$("$prefix/bin/dse" --version)" = "dse 1.0.0" ] ||
  fail "fresh install did not activate v1"
[ ! -e "$prefix/bin/codewhale" ] && [ ! -L "$prefix/bin/codewhale" ] ||
  fail "fresh DSE install created the retired codewhale program link"
[ ! -e "$prefix/lib/codewhale" ] ||
  fail "fresh DSE install created the retired codewhale delivery root"

corrupt_archive="$test_root/corrupt-archive.tar.gz"
cp "$artifact_v2" "$corrupt_archive"
printf 'changed\n' >>"$corrupt_archive"
printf '%s  %s\n' "$(sha256_file "$artifact_v2")" "${corrupt_archive##*/}" \
  >"$corrupt_archive.sha256"
assert_rejected "changed archive" \
  "$delivery" install --artifact "$corrupt_archive" --prefix "$prefix"
[ "$("$prefix/bin/dse" --version)" = "dse 1.0.0" ] ||
  fail "archive checksum failure changed the active release"

tamper_dir="$test_root/tamper"
mkdir -p "$tamper_dir"
tar -xzf "$artifact_v2" -C "$tamper_dir"
tamper_root="$(find "$tamper_dir" -mindepth 1 -maxdepth 1 -type d -print)"
[ -n "$tamper_root" ] || fail "could not locate extracted fixture root"
printf 'changed\n' >>"$tamper_root/bin/dse"
tampered_archive="$test_root/tampered-binary.tar.gz"
(
  cd "$tamper_dir"
  tamper_name="${tamper_root##*/}"
  COPYFILE_DISABLE=1 tar -cf - \
    "$tamper_name/manifest.tsv" \
    "$tamper_name/LICENSE" \
    "$tamper_name/SHA256SUMS" \
    "$tamper_name/bin/dse" \
    "$tamper_name/bin/dse-tui" \
    | gzip -n >"$tampered_archive"
)
printf '%s  %s\n' "$(sha256_file "$tampered_archive")" "${tampered_archive##*/}" \
  >"$tampered_archive.sha256"
assert_rejected "changed binary with recomputed archive checksum" \
  "$delivery" install --artifact "$tampered_archive" --prefix "$prefix"
[ "$("$prefix/bin/dse" --version)" = "dse 1.0.0" ] ||
  fail "internal checksum failure changed the active release"

wrong_target="x86_64-unknown-netbsd"
wrong_artifact="$(
  "$delivery" package \
    --output-dir "$artifacts/wrong-target" \
    --binary-dir "$test_root/v2-bin" \
    --version 2.0.0 \
    --revision "$rev2" \
    --source-tree "$tree2" \
    --target "$wrong_target"
)"
assert_rejected "mismatched platform" \
  "$delivery" install --artifact "$wrong_artifact" --prefix "$prefix"

"$delivery" install --artifact "$artifact_v2" --prefix "$prefix"
"$delivery" verify --prefix "$prefix"
[ "$("$prefix/bin/dse" --version)" = "dse 2.0.0" ] ||
  fail "upgrade did not activate v2"
[ -L "$prefix/lib/dse/previous" ] ||
  fail "upgrade did not retain one previous release"

"$delivery" rollback --prefix "$prefix"
"$delivery" verify --prefix "$prefix"
[ "$("$prefix/bin/dse" --version)" = "dse 1.0.0" ] ||
  fail "rollback did not reactivate v1"

"$delivery" uninstall --prefix "$prefix"
[ ! -e "$prefix/bin/dse" ] && [ ! -L "$prefix/bin/dse" ] ||
  fail "uninstall left the dse program link"
[ ! -e "$prefix/bin/dse-tui" ] && [ ! -L "$prefix/bin/dse-tui" ] ||
  fail "uninstall left the dse-tui program link"
[ ! -e "$prefix/lib/dse" ] ||
  fail "uninstall left delivery metadata"
[ "$(sha256_file "$home/config.toml")" = "$home_before" ] ||
  fail "uninstall changed DSE_HOME data"

if grep -En 'curl|wget|git[[:space:]]+(fetch|pull)|gh[[:space:]]+release' "$delivery" >/dev/null; then
  fail "delivery owner contains a network/release-discovery command"
fi

printf 'delivery self-test: PASS (%s)\n' "$target"
