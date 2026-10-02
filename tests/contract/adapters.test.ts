import { describe, it, expect } from "vitest";
import { connectionSchema, requestSchema } from "../../src/config/schema.js";
import { buildRequest, normalize } from "../../src/adapters/index.js";
import { effectiveRequest } from "../../src/capabilities.js";

describe("adapter wire contracts", () => {
  it.each([
    ["openai-images", "images/generations", "model"],
    ["gemini", "models/family%2Fimage:generateContent", "contents"],
    ["gemini-interactions", "interactions", "input"],
    ["openai-responses", "responses", "tools"],
    ["openrouter-images", "images", "input_references"],
    ["chat-images", "chat/completions", "messages"],
  ] as const)("%s uses its own protocol", (adapter, path, field) => {
    const connection = connectionSchema.parse({
      adapter,
      baseUrl: "https://api.example/v1",
      auth: { type: "none" },
      orchestrationModel: "chat-model",
      chatImageOutput: true,
    });
    const request = requestSchema.parse({
      prompt: "تصویر 123",
      model: "family/image",
    });
    const built = buildRequest({
      connection,
      request,
      model: "family/image",
      operation: "generate",
      references: [],
    });
    expect(built.operation).toBe(path);
    expect(JSON.parse(String(built.body))).toHaveProperty(field);
    expect(String(built.body)).toContain("تصویر 123");
  });
  it.each(["omniroute", "9router"] as const)(
    "%s does not drop Antigravity refs",
    (gateway) => {
      const connection = connectionSchema.parse({
        adapter: "openai-images",
        gateway,
        baseUrl: "http://localhost/v1",
        auth: { type: "none" },
      });
      expect(() =>
        effectiveRequest(
          connection,
          requestSchema.parse({
            prompt: "edit",
            reference_images: [
              { type: "url", url: "https://public.example/x.png" },
            ],
          }),
          "antigravity/gemini-image",
        ),
      ).toThrow("Reference");
    },
  );
  it("rejects text-only successful HTTP bodies", () => {
    const connection = connectionSchema.parse({
      adapter: "gemini",
      baseUrl: "https://api.example/v1beta",
      auth: { type: "none" },
    });
    expect(() =>
      normalize(connection, {
        candidates: [{ content: { parts: [{ text: "hello" }] } }],
      }),
    ).toThrow("final image");
  });
});
