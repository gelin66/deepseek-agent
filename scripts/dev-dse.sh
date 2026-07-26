#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if command -v rustup >/dev/null 2>&1; then
  pinned_channel="$(
    awk -F '"' '/^channel[[:space:]]*=/ { print $2; exit }' rust-toolchain.toml
  )"
  installed_toolchains="$(cd "${TMPDIR:-/tmp}" && rustup toolchain list)"
  if ! printf '%s\n' "$installed_toolchains" |
    awk -v expected="$pinned_channel" '
      $1 == expected || index($1, expected "-") == 1 { found = 1 }
      END { exit(found ? 0 : 1) }
    '; then
    stable_version="$(rustup run stable rustc --version | awk '{ print $2; exit }')"
    if [[ "$stable_version" == "$pinned_channel" ]]; then
      export RUSTUP_TOOLCHAIN=stable
    else
      echo "pinned Rust $pinned_channel is not installed" >&2
      exit 1
    fi
  fi
fi

mode="${1:-focused}"
test_args=(-p dse-tui --bin dse-tui --locked)

run_public_repository_gate() {
  ./scripts/check-public-repository.py
}

run_focused_tests() {
  local filters=(
    "m8a_deepseek_only_entry_tests::canonical_cli_has_no_fleet_or_direct_sandbox_shell"
    "tui::canonical_commands::tests"
    "tui::run_client::tests"
    "tui::run_projection::tests"
    "tui::run_presenter::tests"
    "tui::ui::tests::canonical_"
  )

  # Cargo exits successfully when a filter matches zero tests. Refuse that
  # false-green state so renamed/removed regressions break the local gate.
  local available_tests
  available_tests="$(cargo test "${test_args[@]}" -- --list)"

  for filter in "${filters[@]}"; do
    if ! grep -Fq -- "$filter" <<<"$available_tests"; then
      echo "focused test filter matched no test: $filter" >&2
      exit 1
    fi
    cargo test "${test_args[@]}" "$filter"
  done

  cargo test -p dse-tools --locked
  cargo test -p dse-deepseek --locked
  cargo test -p dse-runtime --test conformance --locked
  cargo test -p dse-app --locked
  cargo test -p dse-app-server --lib --locked
  cargo test -p dse-tui --test exec_terminal_acceptance --locked
  cargo test -p dse-tui --test canonical_tui_run_acceptance --locked
  cargo test -p dse-tui --test canonical_tui_pty_acceptance --locked -- --test-threads=1
}

case "$mode" in
  focused)
    run_public_repository_gate
    cargo fmt --all -- --check
    run_focused_tests
    cargo check -p dse-tui --bin dse-tui --locked
    ;;
  crate)
    run_public_repository_gate
    cargo fmt --all -- --check
    cargo test "${test_args[@]}"
    ;;
  full)
    run_public_repository_gate
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo test --workspace --locked
    ;;
  *)
    echo "usage: $0 [focused|crate|full]" >&2
    exit 2
    ;;
esac
