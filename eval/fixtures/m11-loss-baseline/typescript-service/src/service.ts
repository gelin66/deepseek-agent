import { matchRoute } from "./router.ts";

export function dispatch(path: string): string {
  const match = matchRoute("/users/:userId", path);
  return match ? `user:${match.params.userId}` : "not-found";
}
