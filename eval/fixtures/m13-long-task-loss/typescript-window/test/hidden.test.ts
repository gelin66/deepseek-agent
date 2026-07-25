import assert from "node:assert/strict";
import test from "node:test";

import { mergeWindows, parseWindow } from "../src/window.ts";

test("strictly parses bounded integer windows", () => {
  assert.deepEqual(parseWindow(" 2 : 9 "), { start: 2, end: 9 });
  for (const invalid of ["1:2:3", "1.5:2", "-1:2", "3:2", "1:", ":2", "x:2"]) {
    assert.equal(parseWindow(invalid), null, invalid);
  }
});

test("merges overlapping and adjacent windows without mutating input", () => {
  const input = [
    { start: 5, end: 8 },
    { start: 1, end: 2 },
    { start: 2, end: 4 },
    { start: 10, end: 11 },
  ];
  const snapshot = structuredClone(input);
  assert.deepEqual(mergeWindows(input), [
    { start: 1, end: 8 },
    { start: 10, end: 11 },
  ]);
  assert.deepEqual(input, snapshot);
});
