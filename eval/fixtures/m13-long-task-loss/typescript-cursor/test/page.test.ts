import assert from "node:assert/strict";
import test from "node:test";

import { paginate } from "../src/page.ts";

test("returns the first page", () => {
  const result = paginate(["a", "b", "c"], 2, "s1", null);
  assert.deepEqual(result.items, ["a", "b"]);
  assert.ok(result.nextCursor);
});
