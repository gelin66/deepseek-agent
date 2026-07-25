export interface Window {
  start: number;
  end: number;
}

export function parseWindow(value: string): Window | null {
  const [start, end] = value.split(":").map(Number);
  if (!Number.isFinite(start) || !Number.isFinite(end)) {
    return null;
  }
  return { start, end };
}

export function mergeWindows(windows: readonly Window[]): Window[] {
  return [...windows].sort((left, right) => left.start - right.start);
}
