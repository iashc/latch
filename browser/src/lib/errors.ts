import type { ErrorPayload } from "../../../shared/types";

export class LatchApiError extends Error {
  constructor(
    message: string,
    public readonly code?: string,
    public readonly status?: number,
    public readonly details?: Record<string, unknown>
  ) {
    super(message);
    this.name = "LatchApiError";
  }
}

export function getErrorMessage(error: unknown) {
  if (error instanceof LatchApiError) {
    return error.message;
  }

  if (error instanceof TypeError) {
    return "无法连接 Latch 服务";
  }

  if (error instanceof Error) {
    return error.message;
  }

  return "发生未知错误";
}

export function toApiError(response: Response, parsed: unknown) {
  const payload = parsed as ErrorPayload | null;
  return new LatchApiError(
    payload?.error.message || `请求失败：${response.status}`,
    payload?.error.code,
    response.status,
    payload?.error.details
  );
}
