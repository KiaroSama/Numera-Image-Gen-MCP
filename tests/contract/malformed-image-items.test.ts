import { it, expect } from "vitest";
import { readFile } from "node:fs/promises";
import { normalize } from "../../src/adapters/normalize.js";
import { Generation } from "../../src/services/generation.js";
import { Store } from "../../src/jobs/store.js";
import { Logger } from "../../src/logging.js";
import type { Connection } from "../../src/config/schema.js";
import {
  configuration,
  connection,
  workspace,
  server,
  json,
  imageBytes,
} from "../fixtures/runtime.js";

const adapters: Connection["adapter"][] = [
  "openai-images",
  "openrouter-images",
  "openai-responses",
  "gemini",
  "gemini-interactions",
  "chat-images",
];
function payload(
  adapter: Connection["adapter"],
  base64: string,
  invalid: unknown,
) {
  const pair = (valid: unknown) => [invalid, valid, invalid];
  if (adapter === "openai-responses")
    return {
      status: "completed",
      output: pair({
        type: "image_generation_call",
        status: "completed",
        result: base64,
      }),
    };
  if (adapter === "gemini")
    return {
      candidates: [
        { content: { parts: pair({ inlineData: { data: base64 } }) } },
      ],
    };
  if (adapter === "gemini-interactions")
    return {
      status: "completed",
      steps: [
        {
          type: "model_output",
          content: pair({ type: "image", data: base64 }),
        },
      ],
    };
  if (adapter === "chat-images")
    return {
      choices: [
        {
          message: {
            images: pair({
              image_url: { url: `data:image/png;base64,${base64}` },
            }),
          },
        },
      ],
    };
  return { data: pair({ b64_json: base64 }) };
}
for (const adapter of adapters)
  it.each([null, 42, "malformed", []])(
    `${adapter} isolates malformed sibling %j`,
    async (invalid) => {
      const bytes = await imageBytes();
      const normalized = normalize(
        connection(adapter),
        payload(adapter, bytes.toString("base64"), invalid),
      );
      expect(normalized.images).toHaveLength(3);
      expect(normalized.images[0]).toEqual({
        error: "Malformed image result item.",
      });
      expect(normalized.images[2]).toEqual({
        error: "Malformed image result item.",
      });
      const valid = normalized.images[1]!;
      const base64 = valid.base64 ?? valid.url!.split(",")[1]!;
      expect(Buffer.from(base64, "base64")).toEqual(bytes);
    },
  );

it("keeps malformed envelopes, all-invalid results and explicit refusals fatal", () => {
  expect(() => normalize(connection(), null)).toThrow();
  expect(() => normalize(connection(), { data: [null, 42] })).toThrow();
  expect(() =>
    normalize(connection("gemini"), {
      promptFeedback: { blockReason: "SAFETY" },
    }),
  ).toThrow();
  expect(() =>
    normalize(connection("chat-images"), {
      choices: [{ message: { refusal: "fixture" } }],
    }),
  ).toThrow();
});

it("saves a valid original with a partial receipt when another Images item is malformed", async () =>
  workspace(async (root) => {
    const bytes = await imageBytes();
    let posts = 0;
    await server(
      (req, res) => {
        if (req.method === "POST") posts++;
        json(res, { data: [{ b64_json: bytes.toString("base64") }, null] });
      },
      async (origin) => {
        const config = configuration(root, {
          local: connection("openai-images", {
            baseUrl: `${origin}/v1`,
            defaultModel: "fixture",
            modelOverrides: { fixture: { maxCount: 2 } },
          }),
        });
        const store = new Store(config.stateDir);
        const logger = new Logger(config.logging.directory, "ERROR");
        try {
          const generation = new Generation(config, store, logger);
          const args = { prompt: "fixture", count: 2, request_id: "mixed" };
          const result = await generation.run(args, "generate");
          expect(result.status).toBe("partial");
          expect(result.outputs).toHaveLength(1);
          expect(await readFile(result.outputs[0]!.path)).toEqual(bytes);
          expect(result.errors).toEqual(
            expect.arrayContaining([
              expect.objectContaining({ code: "invalid_response" }),
            ]),
          );
          await generation.run(args, "generate");
          expect(posts).toBe(1);
        } finally {
          store.close();
          logger.close();
        }
      },
    );
  }));
