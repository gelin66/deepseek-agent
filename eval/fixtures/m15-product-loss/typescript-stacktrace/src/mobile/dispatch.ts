// Mobile has a separate exact matcher and is not part of the failing web stack.
export function dispatchRoute(pattern: string, requestTarget: string): boolean {
  return pattern === requestTarget;
}
