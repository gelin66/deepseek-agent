// Experiment retained as a decoy. Production does not import it.
export function normalizeRequestId(value: unknown): boolean {
  return typeof value === "string";
}
