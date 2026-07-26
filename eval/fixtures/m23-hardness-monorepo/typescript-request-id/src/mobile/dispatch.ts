// Mobile has a separate exact header contract and is outside the failing stack.
export function dispatchRequest(headers: Record<string, string>): string | null {
  return headers["x-mobile-request-id"] ?? null;
}
