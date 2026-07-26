import assert from "node:assert/strict";
import test from "node:test";

import { parseForwardedChain } from "../src/forwarded_chain.ts";

test("normalizes a bounded ASCII chain", () => {
  assert.deepEqual(
    parseForwardedChain(" EDGE-1, Api.Example, origin2 "),
    ["edge-1", "api.example", "origin2"],
  );
});

test("rejects empty and repeated members", () => {
  assert.equal(parseForwardedChain("edge,,origin"), null);
  assert.equal(parseForwardedChain("edge, EDGE"), null);
});

test("rejects invalid labels and oversized chains", () => {
  assert.equal(parseForwardedChain("-edge,origin"), null);
  assert.equal(parseForwardedChain("edge_,origin"), null);
  assert.equal(parseForwardedChain("é,origin"), null);
  assert.equal(
    parseForwardedChain("a,b,c,d,e,f,g,h,i"),
    null,
  );
});
