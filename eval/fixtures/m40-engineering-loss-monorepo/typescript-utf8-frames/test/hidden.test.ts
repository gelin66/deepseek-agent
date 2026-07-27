import assert from "node:assert/strict";
import { decodeFrames } from "../src/frames.ts";

const bytes = new TextEncoder().encode('{"sequence":0,"text":"海豚"}\r\n{"sequence":1,"text":"reef"}\n');
const split = bytes.indexOf(0xe6) + 1;
assert.deepEqual(decodeFrames([bytes.slice(0, split), bytes.slice(split)]), [
  { sequence: 0, text: "海豚" },
  { sequence: 1, text: "reef" },
]);
assert.throws(
  () => decodeFrames([new TextEncoder().encode('{"sequence":2}')]),
  /frame_incomplete/,
);
