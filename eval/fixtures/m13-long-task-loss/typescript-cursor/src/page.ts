import { decodeCursor, encodeCursor } from "./token.ts";

export interface Page<T> {
  items: T[];
  nextCursor: string | null;
}

export function paginate<T>(
  items: readonly T[],
  limit: number,
  snapshot: string,
  cursor: string | null,
): Page<T> {
  const offset = cursor ? (decodeCursor(cursor)?.offset ?? 0) : 0;
  const selected = items.slice(offset, offset + limit);
  return {
    items: selected,
    nextCursor:
      offset + selected.length < items.length
        ? encodeCursor({ offset: offset + selected.length, snapshot })
        : null,
  };
}
