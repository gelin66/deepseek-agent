import assert from "node:assert/strict";
import test from "node:test";

import { parseRequestBudget } from "../src/request_budget.ts";

test("parses the canonical bounded request budget", () => {
  assert.deepEqual(
    parseRequestBudget("requests=8;seconds=120"),
    { requests: 8, seconds: 120 },
  );
});

test("rejects missing, reordered, or duplicate fields", () => {
  assert.equal(parseRequestBudget("requests=8"), null);
  assert.equal(
    parseRequestBudget("seconds=120;requests=8"),
    null,
  );
  assert.equal(
    parseRequestBudget("requests=8;requests=9;seconds=120"),
    null,
  );
});

test("rejects noncanonical or out-of-range integers", () => {
  assert.equal(parseRequestBudget("requests=08;seconds=120"), null);
  assert.equal(parseRequestBudget("requests=8x;seconds=120"), null);
  assert.equal(parseRequestBudget("requests=33;seconds=120"), null);
  assert.equal(parseRequestBudget("requests=8;seconds=601"), null);
});
