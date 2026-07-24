export type RouteMatch = {
  params: Record<string, string>;
};

export function matchRoute(pattern: string, path: string): RouteMatch | null {
  const expected = pattern.split("/").filter(Boolean);
  const actual = path.split("/").filter(Boolean);
  if (expected.length !== actual.length) return null;

  const params: Record<string, string> = {};
  for (let index = 0; index < expected.length; index += 1) {
    const segment = expected[index];
    if (segment.startsWith(":")) {
      params[segment.slice(1)] = actual[index];
    } else if (segment !== actual[index]) {
      return null;
    }
  }
  return { params };
}
