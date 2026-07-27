import assert from "node:assert/strict";
import { dispatchRoute } from "../src/dispatch.ts";
import { canonicalRoute } from "../src/protocol.ts";

assert.equal(canonicalRoute(" /API//V1/Users/?q=1#top "), "/api/v1/users");
assert.equal(canonicalRoute("api/v1"), "/api/v1");
assert.throws(() => canonicalRoute("/%zz"), /route_escape_invalid/);
assert.throws(() => canonicalRoute("/../admin"), /route_segment_invalid/);
assert.equal(dispatchRoute("API/V1"), "dispatch:/api/v1");
