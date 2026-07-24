import assert from "node:assert/strict";
import test from "node:test";

import { parseCacheControl } from "../src/cache_policy.ts";

test("parses a basic max-age", () => {
  assert.deepEqual(parseCacheControl("public, max-age=60"), {
    cacheable: true,
    ttlSeconds: 60,
  });
});
