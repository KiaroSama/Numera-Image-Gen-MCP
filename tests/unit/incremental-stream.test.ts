import { it, expect } from "vitest";
import { readSse } from "../../src/http/sse-reader.js";
import { normalizeResponse } from "../../src/adapters/normalize.js";
import { connection } from "../fixtures/runtime.js";
it("drops interim previews across chunk boundaries and retains terminal images", async () => {
  const text =
    'data: {"type":"image_generation.partial_image","b64_json":"preview"}\r\n\r\ndata: {"type":"image_generation.completed","b64_json":"ZmluYWw="}\r\n\r\ndata: [DONE]\r\n\r\n';
  async function* chunks() {
    for (let i = 0; i < text.length; i += 7)
      yield Buffer.from(text.slice(i, i + 7));
  }
  const bytes = await readSse(chunks(), 10000, AbortSignal.timeout(1000));
  expect(bytes.toString()).not.toContain("preview");
  expect(
    normalizeResponse(
      connection("openrouter-images"),
      bytes,
      "text/event-stream",
    ).images,
  ).toEqual([{ base64: "ZmluYWw=" }]);
});
it("fails truncated or excessive streaming input without final success", async () => {
  async function* chunks() {
    yield Buffer.from('data: {"type":"image_generation.completed"}');
  }
  await expect(
    readSse(chunks(), 10000, AbortSignal.timeout(1000)),
  ).rejects.toMatchObject({ code: "outcome_unknown" });
  await expect(
    readSse(chunks(), 1, AbortSignal.timeout(1000)),
  ).rejects.toMatchObject({ code: "invalid_response" });
});
