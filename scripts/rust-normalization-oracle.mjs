// Exact frozen continuation, malformed-image and adapter contracts; no provider calls.
export async function normalizationOracle({ source, emit, sharp }) {
  const { connectionSchema, requestSchema } = await source("config/schema.js");
  const { buildRequest, normalize } = await source("adapters/index.js");
  const { effectiveRequest } = await source("capabilities.js");
  const connection = (adapter, overrides = {}) =>
    connectionSchema.parse({
      adapter,
      baseUrl: "https://api.example/v1",
      auth: { type: "none" },
      ...overrides,
    });
  const outcome = (action) => {
    try {
      return { value: action() };
    } catch (error) {
      if (typeof error?.code !== "string" || typeof error?.message !== "string")
        throw error;
      return { error: { code: error.code, message: error.message } };
    }
  };
  const bytes = await sharp({
    create: {
      width: 8,
      height: 6,
      channels: 4,
      background: { r: 24, g: 100, b: 180, alpha: 0.5 },
    },
  })
    .png()
    .toBuffer();
  const base64 = bytes.toString("base64");
  const adapters = [
    "openai-images",
    "openrouter-images",
    "openai-responses",
    "gemini",
    "gemini-interactions",
    "chat-images",
  ];
  const wires = [];
  for (const [adapter, operation, field] of [
    ["openai-images", "images/generations", "model"],
    ["gemini", "models/family%2Fimage:generateContent", "contents"],
    ["gemini-interactions", "interactions", "input"],
    ["openai-responses", "responses", "tools"],
    ["openrouter-images", "images", "input_references"],
    ["chat-images", "chat/completions", "messages"],
  ]) {
    const c = connection(adapter, {
      orchestrationModel: "chat-model",
      chatImageOutput: true,
    });
    const request = requestSchema.parse({
      prompt: "تصویر 123",
      model: "family/image",
    });
    wires.push({
      name: adapter,
      connection: c,
      request,
      model: "family/image",
      operation: "generate",
      required_operation: operation,
      field,
      expected: outcome(() => {
        const built = buildRequest({
          connection: c,
          request,
          model: "family/image",
          operation: "generate",
          references: [],
        });
        if (typeof built.body !== "string")
          throw new Error("Expected source JSON wire.");
        return {
          operation: built.operation,
          prefix: built.prefix,
          body: JSON.parse(built.body),
          serialized: built.body,
        };
      }),
    });
  }
  const gateways = ["omniroute", "9router"].map((gateway) => {
    const c = connection("openai-images", {
      gateway,
      baseUrl: "http://localhost/v1",
    });
    const request = requestSchema.parse({
      prompt: "edit",
      reference_images: [{ type: "url", url: "https://public.example/x.png" }],
    });
    return {
      name: gateway,
      connection: c,
      request,
      model: "antigravity/gemini-image",
      expected: outcome(() =>
        effectiveRequest(c, request, "antigravity/gemini-image"),
      ),
    };
  });
  const payload = (adapter, invalid) => {
    const pair = (valid) => [invalid, valid, invalid];
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
  };
  const pairs = [];
  for (const adapter of adapters)
    for (const [kind, invalid] of [
      ["null", null],
      ["number", 42],
      ["string", "malformed"],
      ["array", []],
    ]) {
      const c = connection(adapter);
      const raw = payload(adapter, invalid);
      pairs.push({
        name: `${adapter}-${kind}`,
        kind,
        connection: c,
        raw,
        expected: outcome(() => normalize(c, raw)),
      });
    }
  const fatal = [
    ["null-envelope", "openai-images", null],
    ["all-invalid-images", "openai-images", { data: [null, 42] }],
    ["gemini-safety", "gemini", { promptFeedback: { blockReason: "SAFETY" } }],
    [
      "chat-refusal",
      "chat-images",
      { choices: [{ message: { refusal: "fixture" } }] },
    ],
    [
      "gemini-text-only",
      "gemini",
      { candidates: [{ content: { parts: [{ text: "hello" }] } }] },
    ],
  ].map(([name, adapter, raw]) => {
    const c = connection(adapter);
    return {
      name,
      connection: c,
      raw,
      expected: outcome(() => normalize(c, raw)),
    };
  });
  const continuation = [
    [
      "gemini-signatures",
      "gemini",
      {
        candidates: [
          {
            content: {
              parts: [
                {
                  thought: true,
                  thoughtSignature: "opaque-private",
                  text: "hidden",
                },
                {
                  inlineData: { mimeType: "image/png", data: "aW1hZ2U=" },
                  thoughtSignature: "final-private",
                },
              ],
            },
          },
        ],
      },
    ],
    [
      "interactions-signatures",
      "gemini-interactions",
      {
        id: "interaction",
        status: "completed",
        steps: [
          {
            type: "thought",
            signature: "encrypted",
            summary: [{ type: "image", data: "not-final" }],
          },
          {
            type: "model_output",
            content: [{ type: "image", data: "ZmluYWw=" }],
          },
        ],
      },
    ],
  ].map(([name, adapter, raw]) => {
    const c = connection(adapter);
    return {
      name,
      connection: c,
      raw,
      expected: outcome(() => normalize(c, raw)),
    };
  });
  const mixedConnection = connection("openai-images", {
    defaultModel: "fixture",
    modelOverrides: { fixture: { maxCount: 2 } },
  });
  const mixedRaw = { data: [{ b64_json: base64 }, null] };
  await emit("normalization-acceptance", {
    original: base64,
    wires,
    gateways,
    pairs,
    fatal,
    continuation,
    mixed: {
      connection: mixedConnection,
      request: { prompt: "fixture", count: 2, request_id: "mixed" },
      raw: mixedRaw,
      expected: normalize(mixedConnection, mixedRaw),
    },
  });
}
