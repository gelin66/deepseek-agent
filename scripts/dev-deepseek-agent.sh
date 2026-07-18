#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

mode="${1:-focused}"
test_args=(-p codewhale-tui --bin codewhale-tui --locked)

run_focused_tests() {
  local filters=(
    "memory::tests"
    "fleet::worker_runtime::tests"
    "api_url_"
    "deepseek_owned_legacy_routes_never_select_strict_by_url"
    "deepseek_tool_reasoning_replay_tests"
    "stream_decoder_tests"
    "schema_sanitize::tests"
    "client::deepseek::tests"
    "strict_tool_mode_doctor"
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

  cargo test -p codewhale-tools --locked
  cargo test -p codewhale-deepseek --locked
  cargo test -p codewhale-runtime --test conformance --locked
  cargo test -p codewhale-app --locked
  cargo test -p codewhale-app-server --lib --locked
  cargo test -p codewhale-tui --test exec_terminal_acceptance --locked
  cargo test -p codewhale-tui --test canonical_tui_run_acceptance --locked
  cargo test -p codewhale-tui --test canonical_tui_pty_acceptance --locked -- --test-threads=1
}

case "$mode" in
  focused)
    cargo fmt --all -- --check
    run_focused_tests
    cargo check -p codewhale-tui --bin codewhale-tui --locked
    ;;
  crate)
    cargo fmt --all -- --check
    cargo test "${test_args[@]}"
    ;;
  full)
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo test --workspace --locked
    ;;
  *)
    echo "usage: $0 [focused|crate|full]" >&2
    exit 2
    ;;
esac
