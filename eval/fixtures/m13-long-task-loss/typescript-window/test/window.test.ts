import assert from "node:assert/strict";
import test from "node:test";

import { mergeWindows, parseWindow } from "../src/window.ts";

test("parses a basic window", () => {
  assert.deepEqual(parseWindow("1:3"), { start: 1, end: 3 });
});

test("sorts windows", () => {
  assert.deepEqual(mergeWindows([{ start: 5, end: 7 }, { start: 1, end: 2 }]), [
    { start: 1, end: 2 },
    { start: 5, end: 7 },
  ]);
});
