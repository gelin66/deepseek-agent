import assert from "node:assert/strict";
import test from "node:test";
import { dispatch } from "../src/service.ts";

test("dispatches a basic user route", () => {
  assert.equal(dispatch("/users/alice"), "user:alice");
});
