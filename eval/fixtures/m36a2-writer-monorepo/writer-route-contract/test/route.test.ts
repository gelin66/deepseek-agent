import assert from "node:assert/strict";
import test from "node:test";

import { decodeRoute } from "../src/codec.ts";
import { encodeRoute } from "../src/route.ts";

test("round trips the old route", () => {
  const encoded = encodeRoute("deepseek-v4-pro", "high", ["src"]);
  assert.deepEqual(decodeRoute(encoded), {
    model: "deepseek-v4-pro",
    effort: "high",
    scopes: ["src"],
  });
});
