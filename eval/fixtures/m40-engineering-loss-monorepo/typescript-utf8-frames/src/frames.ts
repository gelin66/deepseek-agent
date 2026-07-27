import { stableSequence } from "./checksum.ts";

export function decodeFrames(chunks: Uint8Array[]): unknown[] {
  let text = "";
  for (const chunk of chunks) {
    text += Buffer.from(chunk).toString("utf8");
  }
  return text
    .split("\n")
    .filter((line) => line.length > 0)
    .map((line) => {
      const value = JSON.parse(line);
      stableSequence(value);
      return value;
    });
}
