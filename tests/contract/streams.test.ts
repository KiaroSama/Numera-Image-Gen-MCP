import { beforeAll, describe, expect, it } from "vitest";
import { FormData } from "undici";
import { join } from "node:path";
import {
  buildRequest,
  normalize,
  normalizeResponse,
} from "../../src/adapters/index.js";
import type { AdapterInput } from "../../src/adapters/types.js";
import { requestSchema, type Connection } from "../../src/config/schema.js";
import { inspectImage, type Image } from "../../src/files/images.js";
import { sseEvents } from "../../src/http/stream.js";
import { configuration, connection, imageBytes } from "../fixtures/runtime.js";

let image: Image;
let encoded: string;
beforeAll(async () => {
  image = await inspectImage(
    await imageBytes(),
    configuration(join(process.cwd(), ".ci-work", "wire")),
  );
  encoded = image.bytes.toString("base64");
});
const event = (type: string, data: unknown) =>
  `${type ? `event: ${type}\n` : ""}data: ${JSON.stringify(data)}\n\n`;
const bytes = (value: string) => Buffer.from(value, "utf8");
const stream = (profile: Connection, text: string) =>
  normalizeResponse(profile, bytes(text), "text/event-stream; charset=utf-8");
const adapterInput = (
  adapter: Connection["adapter"],
  changes: Record<string, unknown> = {},
  overrides: Record<string, unknown> = {},
  references: Image[] = [],
): AdapterInput => ({
  connection: connection(adapter, overrides),
  request: requestSchema.parse({ prompt: "A café", ...changes }),
  model: "image/model",
  operation: references.length ? "edit" : "generate",
  references,
});
const body = (input: AdapterInput): Record<string, unknown> =>
  JSON.parse(String(buildRequest(input).body)) as Record<string, unknown>;

describe("native final result normalization", () => {
  it.each(["openai-images", "openrouter-images"] as const)(
    "%s preserves final bytes/URLs, partial errors and usage",
    (adapter) => {
      const result = normalize(connection(adapter), {
        model: "actual-model",
        id: "owned-upstream",
        usage: { images: 2 },
        data: [
          { b64_json: encoded },
          { url: "https://fixture.example/output.png" },
          { error: "fixture failure" },
        ],
      });
      expect(result).toMatchObject({
        upstreamModel: "actual-model",
        upstreamId: "owned-upstream",
        usage: { images: 2 },
        images: [
          { base64: encoded },
          { url: "https://fixture.example/output.png" },
          { error: expect.any(String) },
        ],
        warnings: [],
      });
    },
  );

  it("normalizes both gateway profiles through their actual Images contract", () => {
    for (const gateway of ["omniroute", "9router"] as const)
      expect(
        normalize(connection("openai-images", { gateway }), {
          data: [{ b64_json: encoded }],
        }).images,
      ).toEqual([{ base64: encoded }]);
  });

  it("excludes Gemini thought images and retains final candidate warnings", () => {
    const result = normalize(connection("gemini"), {
      candidates: [
        {
          finishReason: "MAX_TOKENS",
          content: {
            parts: [
              { text: "description" },
              { thought: true, inlineData: { data: "not-final" } },
              { inlineData: { mimeType: "image/png", data: encoded } },
            ],
          },
        },
      ],
    });
    expect(result.images).toEqual([{ base64: encoded }]);
    expect(result.warnings).toEqual(["Candidate ended with MAX_TOKENS."]);
    expect(() =>
      normalize(connection("gemini"), {
        promptFeedback: { blockReason: "SAFETY" },
      }),
    ).toThrow(expect.objectContaining({ code: "provider_rejection" }));
  });

  it("normalizes Interactions model-output steps and standalone output_image fallback", () => {
    expect(
      normalize(connection("gemini-interactions"), {
        status: "completed",
        model: "actual",
        steps: [
          { type: "tool", content: [{ type: "image", data: "ignored" }] },
          {
            type: "model_output",
            content: [
              { type: "text", text: "ignored" },
              { type: "image", data: encoded },
              { type: "image", uri: "https://fixture.example/output.png" },
            ],
          },
        ],
      }),
    ).toMatchObject({
      upstreamModel: "actual",
      images: [
        { base64: encoded },
        { url: "https://fixture.example/output.png" },
      ],
    });
    expect(
      normalize(connection("gemini-interactions"), {
        status: "completed",
        output_image: { data: encoded },
      }).images,
    ).toEqual([{ base64: encoded }]);
  });

  it("accepts only completed Responses image-generation calls, not message text or partials", () => {
    const result = normalize(connection("openai-responses"), {
      status: "completed",
      output: [
        { type: "message", content: "text" },
        {
          type: "image_generation_call",
          status: "in_progress",
          result: "partial",
        },
        { type: "image_generation_call", status: "completed", result: encoded },
      ],
    });
    expect(result.images).toEqual([{ base64: encoded }]);
  });

  it("extracts explicit chat image output and rejects refusals", () => {
    expect(
      normalize(connection("chat-images"), {
        choices: [
          {
            message: {
              content: "text",
              images: [
                { image_url: { url: `data:image/png;base64,${encoded}` } },
              ],
            },
          },
        ],
      }).images,
    ).toEqual([{ url: `data:image/png;base64,${encoded}` }]);
    expect(() =>
      normalize(connection("chat-images"), {
        choices: [{ message: { refusal: "blocked" } }],
      }),
    ).toThrow(expect.objectContaining({ code: "provider_rejection" }));
  });

  it.each(["gemini-interactions", "openai-responses"] as const)(
    "%s distinguishes asynchronous job handles and terminal rejection from final output",
    (adapter) => {
      for (const status of ["queued", "in_progress"])
        expect(
          normalize(connection(adapter), {
            status,
            id: "job-123",
            model: "actual",
          }),
        ).toMatchObject({
          images: [],
          job: {
            id: "job-123",
            kind:
              adapter === "gemini-interactions" ? "interactions" : "responses",
          },
        });
      for (const status of ["failed", "cancelled", "incomplete"])
        expect(() => normalize(connection(adapter), { status })).toThrow(
          expect.objectContaining({ code: "provider_rejection" }),
        );
    },
  );

  it.each([
    "openai-images",
    "gemini",
    "gemini-interactions",
    "openai-responses",
    "openrouter-images",
    "chat-images",
  ] as const)("%s rejects empty/text-only payloads", (adapter) => {
    expect(() => normalize(connection(adapter), {})).toThrow(
      expect.objectContaining({ code: "no_image_returned" }),
    );
    expect(() =>
      normalize(connection(adapter), {
        data: [{ b64_json: "", url: "" }],
        candidates: [{ content: { parts: [{ text: "description" }] } }],
        output: [{ type: "message" }],
        choices: [{ message: { content: "text" } }],
      }),
    ).toThrow(expect.objectContaining({ code: "no_image_returned" }));
  });

  it.each(["png", "jpeg", "webp"] as const)(
    "preserves native binary %s responses for later image validation",
    async (format) => {
      const actual = await imageBytes(format);
      expect(
        normalizeResponse(
          connection(),
          actual,
          `image/${format}; charset=binary`,
        ),
      ).toEqual({
        images: [{ bytes: actual }],
        upstreamModel: null,
        warnings: [],
      });
    },
  );

  it("rejects malformed JSON and non-object responses", () => {
    expect(() =>
      normalizeResponse(connection(), bytes("{broken"), "application/json"),
    ).toThrow(expect.objectContaining({ code: "invalid_response" }));
    for (const value of [null, [], 2, "text"])
      expect(() => normalize(connection(), value)).toThrow(
        expect.objectContaining({ code: "invalid_response" }),
      );
    expect(
      normalizeResponse(
        connection(),
        bytes(JSON.stringify({ data: [{ b64_json: encoded }] })),
        "application/json",
      ).images,
    ).toEqual([{ base64: encoded }]);
  });
});

describe("complete SSE frames and terminal outcomes", () => {
  it("handles CRLF, comments, ignored metadata, inferred event types and multiline JSON", () => {
    const text =
      ': keepalive\r\n\r\nevent: progress\r\nid: 1\r\ndata: {\r\ndata: "step": 2}\r\n\r\ndata: {"type":"complete","value":3}\r\n\r\ndata: [DONE]\r\n\r\n';
    expect(sseEvents(bytes(text))).toEqual([
      { type: "progress", data: { step: 2 } },
      { type: "complete", data: { type: "complete", value: 3 } },
      { type: "done", data: {} },
    ]);
  });

  it("rejects truncated frames, invalid JSON and scalar event data", () => {
    expect(() => sseEvents(bytes("data: {}\n"))).toThrow(
      expect.objectContaining({ code: "outcome_unknown" }),
    );
    for (const data of ["{bad", "null", "[]", "1"])
      expect(() => sseEvents(bytes(`data: ${data}\n\n`))).toThrow(
        expect.objectContaining({ code: "invalid_response" }),
      );
  });

  it("requires 9router done result data and ignores partial previews", () => {
    const profile = connection("openai-images", { gateway: "9router" });
    expect(
      stream(
        profile,
        event("partial", { b64_json: "preview-only" }) +
          event("done", { data: [{ b64_json: encoded }], model: "actual" }),
      ),
    ).toMatchObject({ images: [{ base64: encoded }], upstreamModel: "actual" });
    expect(() =>
      stream(
        profile,
        event("partial", { b64_json: encoded }) + "data: [DONE]\n\n",
      ),
    ).toThrow(expect.objectContaining({ code: "outcome_unknown" }));
  });

  it("requires dedicated OpenRouter completed image events and terminal DONE", () => {
    const profile = connection("openrouter-images");
    const final = event("image_generation.completed", {
      b64_json: encoded,
      usage: { cost: 0 },
    });
    expect(
      stream(
        profile,
        event("image_generation.partial", { b64_json: "preview" }) +
          final +
          "data: [DONE]\n\n",
      ),
    ).toMatchObject({ images: [{ base64: encoded }], usage: { cost: 0 } });
    for (const text of [
      final,
      event("image_generation.partial", { b64_json: encoded }) +
        "data: [DONE]\n\n",
    ])
      expect(() => stream(profile, text)).toThrow(
        expect.objectContaining({ code: "outcome_unknown" }),
      );
  });

  it("requires a completed Responses envelope rather than partial image events", () => {
    const profile = connection("openai-responses");
    const completed = {
      status: "completed",
      output: [
        { type: "image_generation_call", status: "completed", result: encoded },
      ],
    };
    expect(
      stream(
        profile,
        event("response.image_generation_call.partial_image", {
          partial_image_b64: "preview",
        }) + event("response.completed", { response: completed }),
      ).images,
    ).toEqual([{ base64: encoded }]);
    for (const text of [
      event("response.image_generation_call.partial_image", {
        partial_image_b64: encoded,
      }),
      event("response.completed", {}),
    ])
      expect(() => stream(profile, text)).toThrow(
        expect.objectContaining({ code: "outcome_unknown" }),
      );
  });

  it("normalizes Gemini candidate frames while ignoring thought-only frames", () => {
    const thought = event("", {
      candidates: [
        {
          content: {
            parts: [{ thought: true, inlineData: { data: "ignored" } }],
          },
        },
      ],
    });
    const final = event("", {
      candidates: [
        {
          finishReason: "STOP",
          content: { parts: [{ inlineData: { data: encoded } }] },
        },
      ],
    });
    expect(stream(connection("gemini"), thought + final).images).toEqual([
      { base64: encoded },
    ]);
    expect(() => stream(connection("gemini"), thought)).toThrow(
      expect.objectContaining({ code: "no_image_returned" }),
    );
  });

  it.each(["error", "response.failed", "response.incomplete"])(
    "does not report final success after %s even if final image bytes were seen",
    (type) => {
      const final = event("response.completed", {
        response: {
          status: "completed",
          output: [
            {
              type: "image_generation_call",
              status: "completed",
              result: encoded,
            },
          ],
        },
      });
      expect(() =>
        stream(
          connection("openai-responses"),
          final + event(type, { error: "fixture" }),
        ),
      ).toThrow(expect.objectContaining({ code: "outcome_unknown" }));
    },
  );

  it("rejects unimplemented streaming adapters instead of treating preview data as final", () => {
    expect(() =>
      stream(
        connection("chat-images"),
        event("done", { data: [{ b64_json: encoded }] }),
      ),
    ).toThrow(expect.objectContaining({ code: "unsupported_operation" }));
  });
});

describe("input and option fidelity at native request seams", () => {
  it("encodes multipart reference files and alpha masks without dropping binary bytes", async () => {
    const input = adapterInput(
      "openai-images",
      { quality: "high" },
      {
        edit: {
          mode: "multipart",
          encoding: "image[]",
          masks: true,
          maskPolarity: "transparent-edit",
          maxReferences: 2,
        },
      },
      [image, image],
    );
    input.mask = image;
    const built = buildRequest(input);
    expect(built.operation).toBe("images/edits");
    expect(built.body).toBeInstanceOf(FormData);
    const form = built.body as FormData;
    expect(form.get("prompt")).toBe("A café");
    expect(form.get("n")).toBe("1");
    expect(form.getAll("image[]")).toHaveLength(2);
    for (const file of form.getAll("image[]")) {
      expect(typeof file).not.toBe("string");
      if (typeof file === "string")
        throw new Error("Expected binary image file.");
      expect(Buffer.from(await file.arrayBuffer())).toEqual(image.bytes);
      expect(file.type).toBe("image/png");
    }
    expect(form.get("mask")).toBeDefined();
  });

  it.each(["image", "images", "image_urls", "input_references"] as const)(
    "retains references under documented JSON %s encoding",
    (encoding) => {
      const input = adapterInput(
        "openai-images",
        {},
        { edit: { mode: "json", encoding, maxReferences: 2, masks: true } },
        [image],
      );
      input.mask = image;
      const result = body(input);
      expect(JSON.stringify(result[encoding])).toContain(encoded);
      expect(result.mask).toBe(`data:image/png;base64,${encoded}`);
      expect(buildRequest(input).operation).toBe("images/edits");
    },
  );

  it("supports explicit generation-route editing and independent gateway reference encodings", () => {
    expect(
      buildRequest(
        adapterInput(
          "openai-images",
          {},
          { edit: { mode: "generation", encoding: "image" } },
          [image],
        ),
      ).operation,
    ).toBe("images/generations");
    const codex = adapterInput("openai-images", {}, { gateway: "9router" }, [
      image,
    ]);
    codex.model = "codex/image";
    const one = body(codex);
    const two = body(
      adapterInput(
        "openai-images",
        {},
        {
          gateway: "9router",
          edit: { mode: "json", encoding: "images", maxReferences: 2 },
        },
        [image, image],
      ),
    );
    // Codex is the only verified implicit 9router reference route.
    expect(one.image).toContain(encoded);
    expect(two.images).toEqual([
      `data:image/png;base64,${encoded}`,
      `data:image/png;base64,${encoded}`,
    ]);
    const omni = adapterInput("openai-images", {}, { gateway: "omniroute" }, [
      image,
    ]);
    omni.model = "codex/image";
    expect(buildRequest(omni).body).toBeInstanceOf(FormData);
  });

  it("maps native Gemini image settings, Interactions privacy and Responses orchestration separately", () => {
    const gemini = adapterInput(
      "gemini",
      { aspect_ratio: "16:9", image_size: "2K" },
      {},
      [image],
    );
    gemini.model = "models/image-family";
    expect(buildRequest(gemini).operation).toBe(
      "models/image-family:generateContent",
    );
    expect(body(gemini)).toMatchObject({
      generationConfig: {
        responseModalities: ["TEXT", "IMAGE"],
        imageConfig: { aspectRatio: "16:9", imageSize: "2K" },
      },
      contents: [
        {
          role: "user",
          parts: [
            { text: "A café" },
            { inlineData: { mimeType: "image/png", data: encoded } },
          ],
        },
      ],
    });
    expect(
      body(
        adapterInput(
          "gemini-interactions",
          { aspect_ratio: "1:1", image_size: "1K", output_format: "webp" },
          {},
          [image],
        ),
      ),
    ).toMatchObject({
      store: false,
      response_format: {
        type: "image",
        mime_type: "image/webp",
        aspect_ratio: "1:1",
        image_size: "1K",
      },
      input: [{ type: "text" }, { type: "image", data: encoded }],
    });
    const responses = body(
      adapterInput(
        "openai-responses",
        { quality: "high", output_format: "png" },
        { orchestrationModel: "text-orchestrator" },
        [image],
      ),
    );
    expect(responses).toMatchObject({
      model: "text-orchestrator",
      store: false,
      tools: [
        {
          type: "image_generation",
          model: "image/model",
          action: "edit",
          quality: "high",
          output_format: "png",
        },
      ],
      tool_choice: { type: "image_generation" },
    });
    expect(JSON.stringify(responses.input)).toContain(encoded);
  });

  it("maps explicit chat image output and disables OpenRouter fallback even when requested", () => {
    expect(
      body(
        adapterInput(
          "chat-images",
          { aspect_ratio: "3:2", image_size: "2K" },
          { chatImageOutput: true },
          [image],
        ),
      ),
    ).toMatchObject({
      modalities: ["image", "text"],
      image_config: { aspect_ratio: "3:2", image_size: "2K" },
    });
    const result = body(
      adapterInput(
        "openrouter-images",
        {
          seed: 42,
          image_size: "2K",
          provider_options: {
            provider: { allow_fallbacks: true, order: ["fixture-provider"] },
          },
        },
        {},
        [image],
      ),
    );
    expect(result).toMatchObject({
      resolution: "2K",
      seed: 42,
      provider: { allow_fallbacks: false, order: ["fixture-provider"] },
    });
    expect(JSON.stringify(result.input_references)).toContain(encoded);
  });

  it.each([
    ["gemini", { quality: "high" }, {}],
    ["gemini", { output_format: "png" }, {}],
    ["gemini-interactions", { background: "transparent" }, {}],
    ["chat-images", {}, {}],
    ["chat-images", { quality: "high" }, { chatImageOutput: true }],
    ["openai-responses", {}, {}],
    ["openai-responses", { seed: 1 }, { orchestrationModel: "text" }],
    ["openrouter-images", { negative_prompt: "avoid" }, {}],
    ["openai-images", { seed: 1 }, {}],
    ["comfyui", {}, {}],
  ] as const)(
    "rejects unmapped %s options instead of silently dropping them",
    (adapter, changes, overrides) => {
      expect(() =>
        buildRequest(adapterInput(adapter, changes, overrides)),
      ).toThrow();
    },
  );

  it.each([
    "gemini",
    "gemini-interactions",
    "openai-responses",
    "chat-images",
    "openrouter-images",
  ] as const)("%s refuses an undefined native mask contract", (adapter) => {
    const input = adapterInput(
      adapter,
      {},
      { orchestrationModel: "text", chatImageOutput: true },
      [image],
    );
    input.mask = image;
    expect(() => buildRequest(input)).toThrow(
      expect.objectContaining({ code: "unsupported_operation" }),
    );
  });

  it("rejects empty editing inputs, unverified 9router edits and clamped OmniRoute tiers", () => {
    const empty = adapterInput("openai-images");
    empty.operation = "edit";
    expect(() => buildRequest(empty)).toThrow(
      expect.objectContaining({ code: "invalid_input" }),
    );
    expect(() =>
      buildRequest(
        adapterInput("openai-images", {}, { gateway: "9router" }, [image]),
      ),
    ).toThrow(expect.objectContaining({ code: "unsupported_operation" }));
    const tier = adapterInput(
      "openai-images",
      { image_size: "3K" },
      { gateway: "omniroute" },
    );
    tier.model = "antigravity/image";
    expect(() => buildRequest(tier)).toThrow("1K, 2K or 4K");
  });
});
