import assert from "node:assert/strict";
import test from "node:test";
import { mergeHeaders } from "../src/headers.ts";

test("merges distinct headers", () => {
  assert.deepEqual(
    mergeHeaders({"accept": "application/json"}, {"x-trace": "on"}),
    {"accept": "application/json", "x-trace": "on"},
  );
});
