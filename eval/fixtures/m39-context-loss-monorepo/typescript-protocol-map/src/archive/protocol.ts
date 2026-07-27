export function canonicalRoute(raw: string): string {
  return raw.replaceAll("/", "_");
}
