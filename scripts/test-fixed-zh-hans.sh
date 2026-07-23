#!/usr/bin/env bash
set -euo pipefail

readonly repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$repo_root"

catalogs="$(find crates/localization/locales -maxdepth 1 -type f -name '*.json' -print | sort)"
if [[ "$catalogs" != "crates/localization/locales/zh-Hans.json" ]]; then
  echo "fixed zh-Hans gate: expected exactly crates/localization/locales/zh-Hans.json" >&2
  printf 'observed: %s\n' "${catalogs:-<none>}" >&2
  exit 1
fi

rust_i18n_owners="$(rg -l 'rust-i18n' Cargo.toml crates/*/Cargo.toml | sort)"
if [[ "$rust_i18n_owners" != "crates/localization/Cargo.toml" ]]; then
  echo "fixed zh-Hans gate: rust-i18n must have one Cargo owner" >&2
  printf 'observed: %s\n' "${rust_i18n_owners:-<none>}" >&2
  exit 1
fi

if rg -n 'crate::localization|crates/tui/locales|tui/src/localization' \
  crates Cargo.toml scripts --glob '!test-fixed-zh-hans.sh'; then
  echo "fixed zh-Hans gate: retired TUI-private localization path remains" >&2
  exit 1
fi

if rg -n 'LANG|LC_ALL|LC_MESSAGES|locale.*(select|switch|detect)|/translate' \
  crates/localization crates/cli/src/lib.rs crates/tui/src/main.rs; then
  echo "fixed zh-Hans gate: locale detection/switching entered the product message owner" >&2
  exit 1
fi

echo "fixed zh-Hans source contract: pass"
