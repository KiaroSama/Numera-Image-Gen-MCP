import { fail, record } from "../errors.js";
export function sseEvents(
  bytes: Buffer,
): { type: string; data: Record<string, unknown> }[] {
  const text = bytes.toString("utf8");
  if (!text.endsWith("\n\n") && !text.endsWith("\r\n\r\n"))
    fail(
      "outcome_unknown",
      "Stream ended without a complete terminal frame.",
      "response",
    );
  return text
    .split(/\r?\n\r?\n/)
    .filter(Boolean)
    .flatMap((frame) => {
      const lines = frame.split(/\r?\n/),
        data = lines
          .filter((l) => l.startsWith("data:"))
          .map((l) => l.slice(5).trimStart())
          .join("\n");
      const type =
        lines
          .find((l) => l.startsWith("event:"))
          ?.slice(6)
          .trim() ?? "";
      if (!data) return [];
      if (data === "[DONE]") return [{ type: "done", data: {} }];
      try {
        const value = record(JSON.parse(data));
        return [{ type: type || String(value.type ?? ""), data: value }];
      } catch {
        return fail(
          "invalid_response",
          "Invalid streaming event JSON.",
          "response",
        );
      }
    });
}
