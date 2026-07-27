export function stableSequence(value: unknown): number {
  if (typeof value !== "object" || value === null || !("sequence" in value)) {
    throw new Error("frame_sequence_missing");
  }
  const sequence = (value as { sequence: unknown }).sequence;
  if (!Number.isSafeInteger(sequence) || (sequence as number) < 0) {
    throw new Error("frame_sequence_invalid");
  }
  return sequence as number;
}
