import { canonicalRoute } from "./protocol.ts";

export function dispatchRoute(raw: string): string {
  return `dispatch:${canonicalRoute(raw)}`;
}
