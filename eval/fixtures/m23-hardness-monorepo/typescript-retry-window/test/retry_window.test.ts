import assert from "node:assert/strict";
import test from "node:test";

import { parseRetryWindow } from "../src/retry_window.ts";

test("accepts canonical relative seconds", () => {
  assert.equal(parseRetryWindow("0", 1_000), 0);
  assert.equal(parseRetryWindow(" 45 ", 1_000), 45);
  assert.equal(parseRetryWindow("3600", 1_000), 3600);
});

test("accepts a bounded absolute epoch", () => {
  assert.equal(parseRetryWindow("@1000", 1_000), 0);
  assert.equal(parseRetryWindow("@1120", 1_000), 120);
});

test("rejects ambiguous or out-of-window values", () => {
  for (const value of ["01", "+1", "-1", "1.5", "45s", "@999", "@4601"]) {
    assert.equal(parseRetryWindow(value, 1_000), null, value);
  }
});
