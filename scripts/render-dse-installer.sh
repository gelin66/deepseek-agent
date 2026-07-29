#!/usr/bin/env bash
set -euo pipefail

readonly repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
readonly template="$repo_root/scripts/dse-installer.sh.in"
readonly delivery="$repo_root/scripts/dse-delivery.sh"

die() {
  printf 'render-dse-installer: %s\n' "$*" >&2
  exit 1
}

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

[ "$#" -eq 1 ] || die "usage: scripts/render-dse-installer.sh OUTPUT"
output="$1"
case "$output" in
  /*) ;;
  *) output="$PWD/$output" ;;
esac
[ ! -e "$output" ] && [ ! -L "$output" ] || die "refusing to overwrite $output"
mkdir -p "$(dirname "$output")"

delivery_sha="$(sha256_file "$delivery")"
temporary="$output.tmp.$$"
trap 'rm -f "$temporary"' EXIT
awk -v sha="$delivery_sha" '
  {
    gsub(/__DSE_DELIVERY_SHA256__/, sha)
    print
  }
' "$template" >"$temporary"
cat "$delivery" >>"$temporary"
printf '%s\n' '__DSE_DELIVERY_PAYLOAD_END__' >>"$temporary"
chmod 0755 "$temporary"
mv "$temporary" "$output"
trap - EXIT
printf '%s\n' "$output"
