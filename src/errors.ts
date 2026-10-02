export class NumeraError extends Error {
  constructor(
    public code: string,
    message: string,
    public stage = "validation",
    public httpStatus?: number,
    public nextAction = "Correct the request or configuration before submitting again.",
  ) {
    super(message);
    this.name = "NumeraError";
  }
}
export function fail(code: string, message: string, stage?: string): never {
  throw new NumeraError(code, message, stage);
}
export function safeError(error: unknown) {
  const e =
    error instanceof NumeraError
      ? error
      : new NumeraError(
          "upstream_error",
          "The operation failed. Inspect the redacted local log.",
          "runtime",
        );
  return {
    code: e.code,
    message: e.message,
    stage: e.stage,
    http_status: e.httpStatus ?? null,
    next_action: e.nextAction,
  };
}
export function record(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value))
    fail("invalid_response", "Expected an object.", "response");
  return value as Record<string, unknown>;
}
export function array(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}
