import { matchRoute, type RouteMatch } from "../core/route_matcher.ts";

export function dispatchRoute(pattern: string, requestTarget: string): RouteMatch | null {
  return matchRoute(pattern, requestTarget);
}
