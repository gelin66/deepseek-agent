import { restoreSession } from "../src/session";

test("restores an empty session", () => {
  expect(restoreSession(null)).toBe("empty");
});
