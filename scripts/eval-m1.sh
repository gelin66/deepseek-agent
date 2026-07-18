#!/usr/bin/env bash
set -euo pipefail

harness_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
repo_root="$harness_root"
manifest="$harness_root/eval/manifests/m1-offline.tsv"
scope="all"
output=""

usage() {
  cat <<'EOF'
usage: bash scripts/eval-m1.sh [options]

Options:
  --repo PATH                 Clean Git worktree to evaluate (default: this repository)
  --manifest PATH             TSV task manifest (default: eval/manifests/m1-offline.tsv)
  --scope all|cross-revision  Run all cases or only cross-revision cases
  --output PATH               JSONL result path, relative paths use the harness repository
  -h, --help                  Show this help
EOF
}

while (($# > 0)); do
  case "$1" in
    --repo)
      [[ $# -ge 2 ]] || { echo "--repo requires a path" >&2; exit 2; }
      repo_root="$2"
      shift 2
      ;;
    --manifest)
      [[ $# -ge 2 ]] || { echo "--manifest requires a path" >&2; exit 2; }
      manifest="$2"
      shift 2
      ;;
    --scope)
      [[ $# -ge 2 ]] || { echo "--scope requires a value" >&2; exit 2; }
      scope="$2"
      shift 2
      ;;
    --output)
      [[ $# -ge 2 ]] || { echo "--output requires a path" >&2; exit 2; }
      output="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

case "$scope" in
  all|cross-revision) ;;
  *)
    echo "invalid scope: $scope" >&2
    exit 2
    ;;
esac

repo_root="$(cd "$repo_root" && pwd)"
if [[ "$manifest" != /* ]]; then
  manifest="$harness_root/$manifest"
fi
[[ -f "$manifest" ]] || { echo "manifest not found: $manifest" >&2; exit 2; }
git -C "$repo_root" rev-parse --is-inside-work-tree >/dev/null

revision="$(git -C "$repo_root" rev-parse HEAD)"
short_revision="$(git -C "$repo_root" rev-parse --short=12 HEAD)"
harness_revision="$(git -C "$harness_root" rev-parse HEAD)"
if [[ -n "$(git -C "$repo_root" status --porcelain)" ]]; then
  echo "refusing to evaluate a dirty worktree: $repo_root" >&2
  exit 2
fi
if [[ "$repo_root" != "$harness_root" ]] && [[ -n "$(git -C "$harness_root" status --porcelain)" ]]; then
  echo "refusing to use an uncommitted evaluation harness: $harness_root" >&2
  exit 2
fi
manifest_blob="$(git -C "$harness_root" hash-object "$manifest")"

timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
if [[ -z "$output" ]]; then
  output="$harness_root/eval/results/m1-offline-${short_revision}-${timestamp}.jsonl"
elif [[ "$output" != /* ]]; then
  output="$harness_root/$output"
fi
if [[ -e "$output" ]]; then
  echo "refusing to overwrite existing result: $output" >&2
  exit 2
fi

run_name="$(basename "${output%.jsonl}")"
final_raw_dir="$harness_root/eval/raw/$run_name"
if [[ -e "$final_raw_dir" ]]; then
  echo "refusing to overwrite existing raw logs: $final_raw_dir" >&2
  exit 2
fi

expected_header=$'case_id\tslice\tevidence_level\tcomparison\tpackage\ttarget\ttest_name\trequirement'
IFS= read -r header < "$manifest"
if [[ "$header" != "$expected_header" ]]; then
  echo "invalid manifest header: $manifest" >&2
  exit 2
fi

rows=()
seen_file="$(mktemp "${TMPDIR:-/tmp}/codewhale-m1-seen.XXXXXX")"
list_cache_dir="$(mktemp -d "${TMPDIR:-/tmp}/codewhale-m1-list.XXXXXX")"
output_tmp=""
published=false
cleanup() {
  rm -f "$seen_file"
  rm -rf "$list_cache_dir"
  if [[ "$published" != true && -n "$output_tmp" ]]; then
    rm -f "$output_tmp"
  fi
}
trap cleanup EXIT

line_number=1
while IFS= read -r line || [[ -n "$line" ]]; do
  line_number=$((line_number + 1))
  [[ -n "$line" ]] || continue
  IFS=$'\t' read -r case_id slice evidence_level comparison package target test_name requirement extra <<< "$line"

  if [[ -n "${extra:-}" ]] || [[ -z "${case_id:-}" || -z "${slice:-}" || -z "${evidence_level:-}" || -z "${comparison:-}" || -z "${package:-}" || -z "${target:-}" || -z "${test_name:-}" || -z "${requirement:-}" ]]; then
    echo "invalid or incomplete manifest row at line $line_number" >&2
    exit 2
  fi
  if grep -Fxq "$case_id" "$seen_file"; then
    echo "duplicate case_id at line $line_number: $case_id" >&2
    exit 2
  fi
  echo "$case_id" >> "$seen_file"

  case "$evidence_level" in
    full-runtime-offline|protocol-unit|runtime-contract|critic-plumbing|config-contract) ;;
    *) echo "invalid evidence_level at line $line_number: $evidence_level" >&2; exit 2 ;;
  esac
  case "$comparison" in
    cross_revision|candidate_only) ;;
    *) echo "invalid comparison at line $line_number: $comparison" >&2; exit 2 ;;
  esac
  case "$target" in
    bin:*|test:*|lib) ;;
    *) echo "invalid Cargo target at line $line_number: $target" >&2; exit 2 ;;
  esac

  rows+=("$line")
done < <(tail -n +2 "$manifest")

if ((${#rows[@]} == 0)); then
  echo "manifest contains no cases: $manifest" >&2
  exit 2
fi

# Resolve every selected exact test before creating result files. Cargo exits
# successfully when a filter matches zero tests, so a stale manifest must fail
# here instead of being discovered after a long partial suite. Cache each
# package/target listing because many cases share the same test binary.
preflight_selected=0
for line in "${rows[@]}"; do
  IFS=$'\t' read -r case_id slice evidence_level comparison package target test_name requirement <<< "$line"
  if [[ "$scope" == "cross-revision" && "$comparison" != "cross_revision" ]]; then
    continue
  fi

  preflight_selected=$((preflight_selected + 1))
  target_args=()
  case "$target" in
    bin:*) target_args=(--bin "${target#bin:}") ;;
    test:*) target_args=(--test "${target#test:}") ;;
    lib) target_args=(--lib) ;;
  esac

  cache_key="${package}-${target//:/_}"
  cache_key="${cache_key//\//_}"
  list_path="$list_cache_dir/$cache_key.list"
  if [[ ! -f "$list_path" ]]; then
    set +e
    (
      cd "$repo_root"
      cargo test -p "$package" "${target_args[@]}" --locked -- --list
    ) >"$list_path" 2>&1
    list_status=$?
    set -e
    if [[ $list_status -ne 0 ]]; then
      cat "$list_path" >&2
      echo "failed to list tests for $package $target" >&2
      exit 2
    fi
  fi

  match_count="$(grep -Fxc -- "$test_name: test" "$list_path" || true)"
  if [[ "$match_count" != "1" ]]; then
    echo "manifest test must resolve exactly once: $case_id -> $package $target $test_name (found $match_count)" >&2
    exit 2
  fi
done

if ((preflight_selected == 0)); then
  echo "scope selected no cases: $scope" >&2
  exit 2
fi

json_escape() {
  local value="$1"
  value=${value//\\/\\\\}
  value=${value//\"/\\\"}
  value=${value//$'\n'/\\n}
  value=${value//$'\r'/\\r}
  value=${value//$'\t'/\\t}
  printf '%s' "$value"
}

relative_log_path() {
  local path="$1"
  case "$path" in
    "$harness_root"/*) printf '%s' "${path#"$harness_root"/}" ;;
    *) printf '%s' "$path" ;;
  esac
}

output_tmp="${output}.incomplete.$$"
raw_dir="${final_raw_dir}.incomplete.$$"
if [[ -e "$output_tmp" || -e "$raw_dir" ]]; then
  echo "refusing to overwrite incomplete evaluation artifacts" >&2
  exit 2
fi
mkdir -p "$(dirname "$output")" "$raw_dir"
: > "$output_tmp"
suite_started="$(date +%s)"
selected=0
passed=0
failed=0

for line in "${rows[@]}"; do
  IFS=$'\t' read -r case_id slice evidence_level comparison package target test_name requirement <<< "$line"
  if [[ "$scope" == "cross-revision" && "$comparison" != "cross_revision" ]]; then
    continue
  fi

  selected=$((selected + 1))
  log_path="$raw_dir/${case_id}.log"
  started="$(date +%s)"

  target_args=()
  case "$target" in
    bin:*) target_args=(--bin "${target#bin:}") ;;
    test:*) target_args=(--test "${target#test:}") ;;
    lib) target_args=(--lib) ;;
  esac

  echo "[$selected] $case_id"
  set +e
  (
    cd "$repo_root"
    cargo test -p "$package" "${target_args[@]}" --locked "$test_name" -- --exact
  ) 2>&1 | tee "$log_path"
  cargo_status=${PIPESTATUS[0]}
  set -e

  finished="$(date +%s)"
  harness_duration_seconds=$((finished - started))
  status="failed"
  if [[ $cargo_status -eq 0 ]] && grep -Eq '^running 1 test$' "$log_path" && grep -Eq 'test result: ok\. 1 passed; 0 failed;' "$log_path"; then
    status="passed"
    passed=$((passed + 1))
  else
    failed=$((failed + 1))
    if [[ $cargo_status -eq 0 ]]; then
      status="missing"
      echo "case did not execute exactly one passing test: $case_id" >&2
    fi
  fi

  log_record="$(relative_log_path "$final_raw_dir/${case_id}.log")"
  printf '{"schema":"codewhale.eval.m1.v1","record_type":"case","record_class":"regression_contract","product_metric_eligible":false,"verified_success":null,"suite":"m1-offline","case_id":"%s","slice":"%s","evidence_level":"%s","comparison":"%s","revision":"%s","harness_revision":"%s","manifest_blob":"%s","dirty":false,"status":"%s","harness_duration_seconds":%d,"package":"%s","target":"%s","test_name":"%s","requirement":"%s","log":"%s"}\n' \
    "$(json_escape "$case_id")" \
    "$(json_escape "$slice")" \
    "$(json_escape "$evidence_level")" \
    "$(json_escape "$comparison")" \
    "$(json_escape "$revision")" \
    "$(json_escape "$harness_revision")" \
    "$(json_escape "$manifest_blob")" \
    "$(json_escape "$status")" \
    "$harness_duration_seconds" \
    "$(json_escape "$package")" \
    "$(json_escape "$target")" \
    "$(json_escape "$test_name")" \
    "$(json_escape "$requirement")" \
    "$(json_escape "$log_record")" >> "$output_tmp"
done

if ((selected == 0)); then
  echo "scope selected no cases: $scope" >&2
  exit 2
fi

suite_finished="$(date +%s)"
suite_harness_duration_seconds=$((suite_finished - suite_started))
suite_status="passed"
if ((failed > 0)); then
  suite_status="failed"
fi
printf '{"schema":"codewhale.eval.m1.v1","record_type":"summary","record_class":"regression_suite","product_metric_eligible":false,"verified_success":null,"suite":"m1-offline","scope":"%s","revision":"%s","harness_revision":"%s","manifest_blob":"%s","dirty":false,"status":"%s","total":%d,"passed":%d,"failed":%d,"harness_duration_seconds":%d}\n' \
  "$(json_escape "$scope")" \
  "$(json_escape "$revision")" \
  "$(json_escape "$harness_revision")" \
  "$(json_escape "$manifest_blob")" \
  "$(json_escape "$suite_status")" \
  "$selected" "$passed" "$failed" "$suite_harness_duration_seconds" >> "$output_tmp"

publish_revision="$(git -C "$repo_root" rev-parse HEAD)"
if [[ "$publish_revision" != "$revision" ]]; then
  echo "refusing to publish: target HEAD changed during evaluation: $repo_root" >&2
  exit 2
fi
if [[ -n "$(git -C "$repo_root" status --porcelain)" ]]; then
  echo "refusing to publish: target worktree became dirty during evaluation: $repo_root" >&2
  exit 2
fi

publish_harness_revision="$(git -C "$harness_root" rev-parse HEAD)"
if [[ "$publish_harness_revision" != "$harness_revision" ]]; then
  echo "refusing to publish: harness HEAD changed during evaluation: $harness_root" >&2
  exit 2
fi
if [[ -n "$(git -C "$harness_root" status --porcelain)" ]]; then
  echo "refusing to publish: harness worktree became dirty during evaluation: $harness_root" >&2
  exit 2
fi
publish_manifest_blob="$(git -C "$harness_root" hash-object "$manifest")"
if [[ "$publish_manifest_blob" != "$manifest_blob" ]]; then
  echo "refusing to publish: manifest changed during evaluation: $manifest" >&2
  exit 2
fi

mv "$raw_dir" "$final_raw_dir"
mv "$output_tmp" "$output"
published=true

echo "M1 offline: $suite_status ($passed/$selected passed)"
echo "result: $output"
echo "raw logs: $final_raw_dir"

if ((failed > 0)); then
  exit 1
fi
