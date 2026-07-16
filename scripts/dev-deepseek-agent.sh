#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

mode="${1:-focused}"
test_args=(-p codewhale-tui --bin codewhale-tui --locked)

run_focused_tests() {
  local filters=(
    "tools::verify::tests"
    "memory::tests"
    "native_tool_failure_enters_tool_error_recovery_path"
    "subagent_registry_preserves_native_tool_failure_and_metadata"
    "subagent_feedback_marks_native_failure_and_retains_metadata"
    "work_state"
    "max_steps_exhaustion"
    "deepseek_incomplete_finish_reason"
    "reasoning_only_response_fails_instead_of_reporting_completion"
    "fim_parser"
    "api_url_"
    "deepseek_beta_strict_flag_follows_the_final_custom_chat_path"
    "deepseek_tool_reasoning_replay_tests"
    "stream_decoder_tests"
    "schema_sanitize::tests"
    "tools::search::tests"
    "tools::test_runner::tests"
    "tools::verifier::tests"
    "effective_max_output_tokens"
    "official_deepseek_endpoint_requires_exact_final_route_identity"
    "third_party_and_self_hosted_v4_routes_stay_conservative"
    "strict_schema_mode_does_not_force_a_tool_call"
    "client::deepseek::tests"
    "thinking_tool_call_"
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

  cargo test -p codewhale-app --locked
  cargo test -p codewhale-app-server --lib --locked
  cargo test -p codewhale-tui --test exec_terminal_acceptance --locked
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
