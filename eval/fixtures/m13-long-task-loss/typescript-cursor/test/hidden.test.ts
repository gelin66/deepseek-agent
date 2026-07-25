import assert from "node:assert/strict";
import test from "node:test";

import { paginate } from "../src/page.ts";
import { decodeCursor, encodeCursor } from "../src/token.ts";

test("uses canonical base64url and validates decoded shape", () => {
  const token = encodeCursor({ offset: 2, snapshot: "release/七" });
  assert.doesNotMatch(token, /[+/=]/);
  assert.deepEqual(decodeCursor(token), { offset: 2, snapshot: "release/七" });
  assert.equal(decodeCursor(Buffer.from('{"offset":-1,"snapshot":"s"}').toString("base64url")), null);
  assert.equal(decodeCursor(Buffer.from('{"offset":1.5,"snapshot":"s"}').toString("base64url")), null);
  assert.equal(decodeCursor(Buffer.from('{"offset":1,"snapshot":"s","extra":true}').toString("base64url")), null);
  assert.equal(decodeCursor("%%%"), null);
});

test("rejects invalid limits and snapshot drift", () => {
  const first = paginate(["a", "b", "c"], 1, "s1", null);
  assert.throws(() => paginate(["a", "b"], 0, "s1", null), /limit/i);
  assert.throws(() => paginate(["a", "b"], 1.2, "s1", null), /limit/i);
  assert.throws(
    () => paginate(["a", "b"], 1, "s2", first.nextCursor),
    /snapshot/i,
  );
});

test("rejects offsets beyond the collection", () => {
  const token = encodeCursor({ offset: 4, snapshot: "s1" });
  assert.throws(() => paginate(["a"], 1, "s1", token), /offset/i);
});
