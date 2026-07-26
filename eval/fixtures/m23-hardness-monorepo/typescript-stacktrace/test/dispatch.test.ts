import assert from "node:assert/strict";
import test from "node:test";
import { dispatchRoute } from "../src/web/dispatch.ts";

test("matches a simple parameter", () => {
  assert.deepEqual(dispatchRoute("/teams/:team", "/teams/core"), {
    params: { team: "core" },
  });
});
