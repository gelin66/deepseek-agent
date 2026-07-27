export type Route = {
  model: string;
  effort: string;
  scopes: string[];
};

export function encodeRoute(model: string, effort: string, scopes: string[]) {
  return { version: 1, model, reasoning: effort, paths: scopes };
}
