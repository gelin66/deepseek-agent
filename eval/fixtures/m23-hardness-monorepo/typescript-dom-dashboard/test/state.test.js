import assert from "node:assert/strict";
import test from "node:test";

import { visibleRuns } from "../src/state.js";

test("filters completed runs", () => {
  assert.deepEqual(visibleRuns(true).map((run) => run.id), ["run-1"]);
});
