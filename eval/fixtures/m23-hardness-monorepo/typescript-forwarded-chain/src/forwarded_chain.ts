export function parseForwardedChain(
  value: string | undefined,
): string[] | null {
  if (value === undefined) {
    return null;
  }

  const parts = value
    .split(",")
    .map((part) => part.trim().toLowerCase())
    .filter(Boolean);
  return parts.length > 0 ? parts : null;
}
