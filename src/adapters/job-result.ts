import { fail } from "../errors.js";
import type { Normalized } from "./types.js";

export function validateJobResult(
  result: Normalized,
  expected: { id: string; kind: string },
): void {
  const matches = result.job
    ? result.job.id === expected.id && result.job.kind === expected.kind
    : result.upstreamId === expected.id;
  if (!matches)
    fail(
      "invalid_response",
      "Polled response does not match the existing job.",
      "response",
    );
}
