import assert from "node:assert/strict";
import test from "node:test";
import { dispatchRequest } from "../src/http/dispatch.ts";

test("accepts a simple request id", () => {
  assert.deepEqual(dispatchRequest({ "x-dse-request-id": "run-42" }), {
    requestId: "run-42",
  });
});
