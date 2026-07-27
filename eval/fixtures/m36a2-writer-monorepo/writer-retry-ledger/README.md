# Retry ledger migration

Migrate the retry ledger from a permissive v1 summary to a canonical v2
accounting contract. The isolated Writer owns the complete cross-file change.

`summarize_attempts(records)` must:

- accept only a list of dictionaries whose exact keys are `attempt_id`,
  `logical_id`, `status`, `input_tokens`, `output_tokens`, and
  `cost_nanousd`;
- accept non-empty string identifiers and statuses `started` or `completed`;
- reject duplicate `attempt_id` values and booleans or negative token/cost
  integers;
- require completed attempts to have integer token/cost fields, while started
  attempts must use `None` for all three usage fields;
- return physical started/completed/in-flight counts, unique logical-request
  count, complete-usage status, and totals from completed attempts only.

`encode_summary(records)` must return a detached `version: 2` dictionary with
the summary under `accounting`. Invalid input raises `TypeError` or
`ValueError`; no compatibility alias or silent coercion is allowed.
