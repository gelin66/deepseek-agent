import { normalizeRequestId } from "../core/request_id.ts";

export type DispatchContext = { requestId: string };

export function dispatchRequest(
  headers: Record<string, string | undefined>,
): DispatchContext | null {
  const requestId = normalizeRequestId(headers["x-dse-request-id"]);
  return requestId === null ? null : { requestId };
}
