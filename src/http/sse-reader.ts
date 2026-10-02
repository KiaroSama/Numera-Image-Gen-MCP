import { fail, record } from "../errors.js";
export type SseEvent = { type: string; data: Record<string, unknown> };
export async function readSse(
  body: AsyncIterable<Uint8Array>,
  limit: number,
  signal: AbortSignal,
): Promise<Buffer> {
  const decoder = new TextDecoder("utf-8", { fatal: true });
  let pending = "",
    total = 0;
  const finals: string[] = [];
  for await (const chunk of body) {
    signal.throwIfAborted();
    total += chunk.byteLength;
    if (total > limit)
      fail(
        "invalid_response",
        "Stream exceeds aggregate byte limit.",
        "response",
      );
    pending += decoder.decode(chunk, { stream: true });
    let boundary: RegExpExecArray | null;
    while ((boundary = /\r?\n\r?\n/.exec(pending))) {
      const frame = pending.slice(0, boundary.index);
      pending = pending.slice(boundary.index + boundary[0].length);
      const lines = frame.split(/\r?\n/),
        data = lines
          .filter((l) => l.startsWith("data:"))
          .map((l) => l.slice(5).trimStart())
          .join("\n");
      if (!data) continue;
      if (data === "[DONE]") {
        finals.push("data: [DONE]\n\n");
        continue;
      }
      let value: Record<string, unknown>;
      try {
        value = record(JSON.parse(data));
      } catch {
        return fail(
          "invalid_response",
          "Invalid streaming event JSON.",
          "response",
        );
      }
      const type =
        lines
          .find((l) => l.startsWith("event:"))
          ?.slice(6)
          .trim() ?? String(value.type ?? value.event_type ?? "");
      if (["error", "response.failed", "response.incomplete"].includes(type))
        fail(
          "outcome_unknown",
          "Image stream ended with an error; no resubmission made.",
          "response",
        );
      if (
        [
          "progress",
          "partial_image",
          "image_generation.partial_image",
          "response.image_generation_call.partial_image",
        ].includes(type)
      )
        continue;
      finals.push(
        (type ? "event: " + type + "\n" : "") +
          "data: " +
          JSON.stringify(value) +
          "\n\n",
      );
      if (finals.length > 1000)
        fail("invalid_response", "Stream event count exceeded.", "response");
    }
  }
  pending += decoder.decode();
  if (pending.trim())
    fail(
      "outcome_unknown",
      "Stream ended without a complete terminal frame.",
      "response",
    );
  return Buffer.from(finals.join(""), "utf8");
}
