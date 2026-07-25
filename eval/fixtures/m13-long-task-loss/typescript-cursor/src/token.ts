export interface Cursor {
  offset: number;
  snapshot: string;
}

export function encodeCursor(cursor: Cursor): string {
  return Buffer.from(JSON.stringify(cursor), "utf8").toString("base64");
}

export function decodeCursor(value: string): Cursor | null {
  try {
    return JSON.parse(Buffer.from(value, "base64").toString("utf8")) as Cursor;
  } catch {
    return null;
  }
}
