import assert from "node:assert/strict";
import { decodeFrames } from "../src/frames.ts";

const encoder = new TextEncoder();
assert.deepEqual(decodeFrames([encoder.encode('{"sequence":0,"text":"ok"}\n')]), [
  { sequence: 0, text: "ok" },
]);
