// Exact frozen streams.test.ts literals; pure source seams, never provider calls.
export async function streamAcceptanceOracle({ source, emit, sharp }) {
  const { connectionSchema, requestSchema } = await source("config/schema.js");
  const { buildRequest, normalize, normalizeResponse } =
    await source("adapters/index.js");
  const { inspectImage } = await source("files/images.js");
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
      if (typeof error?.code !== "string") throw error;
      return { error: error.code };
    }
  };
  const originals = [];
  for (const format of ["png", "jpeg", "webp"]) {
    const bytes = await sharp({
      create: {
        width: 8,
        height: 6,
        channels: 4,
        background: { r: 24, g: 100, b: 180, alpha: 0.5 },
      },
    })
      .toFormat(format)
      .toBuffer();
    originals.push({ format, bytes });
  }
  const image = await inspectImage(originals[0].bytes, {
    files: { maxOutputBytes: 1048576, maxPixels: 64000000 },
  });
  const encoded = image.bytes.toString("base64");
  const normalization = [];
  const add = (name, adapter, raw, overrides = {}) => {
    const c = connection(adapter, overrides);
    normalization.push({
      name,
      connection: c,
      raw,
      expected: outcome(() => normalize(c, raw)),
    });
  };
  for (const adapter of ["openai-images", "openrouter-images"])
    add(`images-final-${adapter}`, adapter, {
      model: "actual-model",
      id: "owned-upstream",
      usage: { images: 2 },
      data: [
        { b64_json: encoded },
        { url: "https://fixture.example/output.png" },
        { error: "fixture failure" },
      ],
    });
  for (const gateway of ["omniroute", "9router"])
    add(
      `gateway-final-${gateway}`,
      "openai-images",
      { data: [{ b64_json: encoded }] },
      { gateway },
    );
  add("gemini-thought-warning", "gemini", {
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
  add("gemini-safety", "gemini", { promptFeedback: { blockReason: "SAFETY" } });
  add("interactions-model-output", "gemini-interactions", {
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
  });
  add("interactions-output-image", "gemini-interactions", {
    status: "completed",
    output_image: { data: encoded },
  });
  add("responses-completed-call", "openai-responses", {
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
  add("chat-final", "chat-images", {
    choices: [
      {
        message: {
          content: "text",
          images: [{ image_url: { url: `data:image/png;base64,${encoded}` } }],
        },
      },
    ],
  });
  add("chat-refusal", "chat-images", {
    choices: [{ message: { refusal: "blocked" } }],
  });
  for (const adapter of ["gemini-interactions", "openai-responses"]) {
    for (const status of ["queued", "in_progress"])
      add(`async-${adapter}-${status}`, adapter, {
        status,
        id: "job-123",
        model: "actual",
      });
    for (const status of ["failed", "cancelled", "incomplete"])
      add(`terminal-${adapter}-${status}`, adapter, { status, id: "job-123" });
  }
  for (const adapter of [
    "openai-images",
    "gemini",
    "gemini-interactions",
    "openai-responses",
    "openrouter-images",
    "chat-images",
  ]) {
    add(`empty-${adapter}`, adapter, {});
    add(`text-only-${adapter}`, adapter, {
      data: [{ b64_json: "", url: "" }],
      candidates: [{ content: { parts: [{ text: "description" }] } }],
      output: [{ type: "message" }],
      choices: [{ message: { content: "text" } }],
    });
  }
  for (const [name, raw] of [
    ["null", null],
    ["array", []],
    ["number", 2],
    ["string", "text"],
  ])
    add(`non-object-${name}`, "openai-images", raw);

  const frame = (type, data) =>
    `${type ? `event: ${type}\n` : ""}data: ${JSON.stringify(data)}\n\n`;
  const responses = [];
  const responseCase = (
    name,
    adapter,
    text,
    overrides = {},
    mime = "text/event-stream; charset=utf-8",
  ) => {
    const c = connection(adapter, overrides);
    responses.push({
      name,
      connection: c,
      text,
      mime,
      expected: outcome(() =>
        normalizeResponse(c, Buffer.from(text, "utf8"), mime),
      ),
    });
  };
  for (const { format, bytes } of originals) {
    const c = connection("openai-images");
    const mime = `image/${format}; charset=binary`;
    responses.push({
      name: `binary-${format}`,
      connection: c,
      base64: bytes.toString("base64"),
      mime,
      expected: outcome(() => normalizeResponse(c, bytes, mime)),
    });
  }
  responseCase(
    "json-malformed",
    "openai-images",
    "{broken",
    {},
    "application/json",
  );
  responseCase(
    "json-final",
    "openai-images",
    JSON.stringify({ data: [{ b64_json: encoded }] }),
    {},
    "application/json",
  );
  responseCase(
    "9router-final",
    "openai-images",
    frame("partial", { b64_json: "preview-only" }) +
      frame("done", { data: [{ b64_json: encoded }], model: "actual" }),
    { gateway: "9router" },
  );
  responseCase(
    "9router-preview-only",
    "openai-images",
    frame("partial", { b64_json: encoded }) + "data: [DONE]\n\n",
    { gateway: "9router" },
  );
  const router = frame("image_generation.completed", {
    b64_json: encoded,
    usage: { cost: 0 },
  });
  responseCase(
    "openrouter-final",
    "openrouter-images",
    frame("image_generation.partial", { b64_json: "preview" }) +
      router +
      "data: [DONE]\n\n",
  );
  responseCase("openrouter-no-done", "openrouter-images", router);
  responseCase(
    "openrouter-preview-only",
    "openrouter-images",
    frame("image_generation.partial", { b64_json: encoded }) +
      "data: [DONE]\n\n",
  );
  const completed = {
    status: "completed",
    output: [
      { type: "image_generation_call", status: "completed", result: encoded },
    ],
  };
  const final = frame("response.completed", { response: completed });
  responseCase(
    "responses-final",
    "openai-responses",
    frame("response.image_generation_call.partial_image", {
      partial_image_b64: "preview",
    }) + final,
  );
  responseCase(
    "responses-preview-only",
    "openai-responses",
    frame("response.image_generation_call.partial_image", {
      partial_image_b64: encoded,
    }),
  );
  responseCase(
    "responses-no-envelope",
    "openai-responses",
    frame("response.completed", {}),
  );
  const thought = frame("", {
    candidates: [
      {
        content: {
          parts: [{ thought: true, inlineData: { data: "ignored" } }],
        },
      },
    ],
  });
  const geminiFinal = frame("", {
    candidates: [
      {
        finishReason: "STOP",
        content: { parts: [{ inlineData: { data: encoded } }] },
      },
    ],
  });
  responseCase("gemini-final", "gemini", thought + geminiFinal);
  responseCase("gemini-only-thought", "gemini", thought);
  for (const type of ["error", "response.failed", "response.incomplete"])
    responseCase(
      `responses-after-${type}`,
      "openai-responses",
      final + frame(type, { error: "fixture" }),
    );
  responseCase(
    "chat-unsupported-stream",
    "chat-images",
    frame("done", { data: [{ b64_json: encoded }] }),
  );

  const input = (
    adapter,
    changes = {},
    overrides = {},
    referenceCount = 0,
    mask = false,
    model = "image/model",
    operation,
  ) => ({
    connection: connection(adapter, overrides),
    request: requestSchema.parse({ prompt: "A café", ...changes }),
    model,
    operation: operation ?? (referenceCount ? "edit" : "generate"),
    references: Array.from({ length: referenceCount }, () => image),
    ...(mask ? { mask: image } : {}),
  });
  const describe = (name, i) => ({
    name,
    connection: i.connection,
    request: i.request,
    model: i.model,
    operation: i.operation,
    reference_count: i.references.length,
    mask: Boolean(i.mask),
  });
  const wires = [];
  const wire = (name, i) =>
    wires.push({
      ...describe(name, i),
      expected: outcome(() => {
        const built = buildRequest(i);
        if (typeof built.body !== "string")
          throw new Error("Pure JSON case unexpectedly built multipart.");
        return {
          operation: built.operation,
          prefix: built.prefix,
          body: JSON.parse(built.body),
        };
      }),
    });
  for (const encoding of ["image", "images", "image_urls", "input_references"])
    wire(
      `json-reference-${encoding}`,
      input(
        "openai-images",
        {},
        {
          edit: {
            mode: "json",
            encoding,
            maxReferences: encoding === "image" ? 1 : 2,
            masks: true,
          },
        },
        1,
        true,
      ),
    );
  wire(
    "generation-route-edit",
    input(
      "openai-images",
      {},
      { edit: { mode: "generation", encoding: "image" } },
      1,
    ),
  );
  wire(
    "9router-codex-reference",
    input("openai-images", {}, { gateway: "9router" }, 1, false, "codex/image"),
  );
  wire(
    "9router-two-references",
    input(
      "openai-images",
      {},
      {
        gateway: "9router",
        edit: { mode: "json", encoding: "images", maxReferences: 2 },
      },
      2,
    ),
  );
  wire(
    "gemini-settings",
    input(
      "gemini",
      { aspect_ratio: "16:9", image_size: "2K" },
      {},
      1,
      false,
      "models/image-family",
    ),
  );
  wire(
    "interactions-settings",
    input(
      "gemini-interactions",
      { aspect_ratio: "1:1", image_size: "1K", output_format: "webp" },
      {},
      1,
    ),
  );
  wire(
    "responses-settings",
    input(
      "openai-responses",
      { quality: "high", output_format: "png" },
      { orchestrationModel: "text-orchestrator" },
      1,
    ),
  );
  wire(
    "chat-settings",
    input(
      "chat-images",
      { aspect_ratio: "3:2", image_size: "2K" },
      { chatImageOutput: true },
      1,
    ),
  );
  wire(
    "openrouter-fallback",
    input(
      "openrouter-images",
      {
        seed: 42,
        image_size: "2K",
        provider_options: {
          provider: { allow_fallbacks: true, order: ["fixture-provider"] },
        },
      },
      {},
      1,
    ),
  );
  for (const [name, adapter, changes, overrides] of [
    ["gemini-quality", "gemini", { quality: "high" }, {}],
    ["gemini-format", "gemini", { output_format: "png" }, {}],
    [
      "interactions-background",
      "gemini-interactions",
      { background: "transparent" },
      {},
    ],
    ["chat-no-optin", "chat-images", {}, {}],
    [
      "chat-quality",
      "chat-images",
      { quality: "high" },
      { chatImageOutput: true },
    ],
    ["responses-no-orchestrator", "openai-responses", {}, {}],
    [
      "responses-seed",
      "openai-responses",
      { seed: 1 },
      { orchestrationModel: "text" },
    ],
    [
      "openrouter-negative",
      "openrouter-images",
      { negative_prompt: "avoid" },
      {},
    ],
    ["images-seed", "openai-images", { seed: 1 }, {}],
    ["comfy-prompt-only", "comfyui", {}, {}],
  ])
    wire(name, input(adapter, changes, overrides));
  for (const adapter of [
    "gemini",
    "gemini-interactions",
    "openai-responses",
    "chat-images",
    "openrouter-images",
  ])
    wire(
      `mask-${adapter}`,
      input(
        adapter,
        {},
        { orchestrationModel: "text", chatImageOutput: true },
        1,
        true,
      ),
    );
  wire(
    "images-empty-edit",
    input("openai-images", {}, {}, 0, false, "image/model", "edit"),
  );
  wire(
    "9router-unverified-edit",
    input("openai-images", {}, { gateway: "9router" }, 1),
  );
  wire(
    "omniroute-clamped-tier",
    input(
      "openai-images",
      { image_size: "3K" },
      { gateway: "omniroute" },
      0,
      false,
      "antigravity/image",
    ),
  );
  const multipart = [];
  for (const [name, i] of [
    [
      "multipart-image-array-mask",
      input(
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
        2,
        true,
      ),
    ],
    [
      "omniroute-codex-multipart",
      input(
        "openai-images",
        {},
        { gateway: "omniroute" },
        1,
        false,
        "codex/image",
      ),
    ],
  ]) {
    const prepared = buildRequest(i);
    if (typeof prepared.body === "string")
      throw new Error("Source multipart case unexpectedly built JSON.");
    const parts = [];
    for (const [name, value] of prepared.body.entries()) {
      parts.push(
        typeof value === "string"
          ? { name, text: value }
          : {
              name,
              filename: value.name,
              mime: value.type,
              hex: Buffer.from(await value.arrayBuffer()).toString("hex"),
            },
      );
    }
    multipart.push({
      ...describe(name, i),
      references: i.references.map(() => ({ base64: encoded })),
      mask_base64: i.mask ? encoded : null,
      expected: {
        value: {
          operation: prepared.operation,
          prefix: prepared.prefix,
          multipart: parts,
        },
      },
    });
  }
  if (
    normalization.length !== 37 ||
    responses.length !== 19 ||
    wires.length !== 30 ||
    multipart.length !== 2
  )
    throw new Error("Exact frozen stream acceptance cardinality changed.");
  await emit("stream-acceptance", {
    original: encoded,
    normalization,
    responses,
    wires,
    multipart,
  });
}
