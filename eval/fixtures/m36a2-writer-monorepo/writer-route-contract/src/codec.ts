import type { Route } from "./route.ts";

export function decodeRoute(payload: any): Route {
  return {
    model: String(payload.model),
    effort: String(payload.reasoning),
    scopes: Array.from(payload.paths),
  };
}
