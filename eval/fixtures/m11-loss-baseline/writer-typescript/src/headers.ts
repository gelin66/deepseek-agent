export function mergeHeaders(
  defaults: Record<string, string>,
  overrides: Record<string, string>,
): Record<string, string> {
  return { ...defaults, ...overrides };
}
