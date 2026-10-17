// Hosted source expectations for the public response/SSE normalization seam.
export async function streamOracle({ source, emit, plain, images, outcome }) {
  const { normalizeResponse, normalize } = await source(
    "adapters/normalize.js",
  );
  const { sseEvents } = await source("http/stream.js");
  const frame = (type, data) =>
    `${type ? `event: ${type}\n` : ""}data: ${JSON.stringify(data)}\n\n`;
  const encoded = images[0].base64;
  const image = { data: [{ b64_json: encoded }], model: "actual-model" };
  const response = {
    id: "same-job",
    status: "completed",
    output: [
      { type: "image_generation_call", status: "completed", result: encoded },
    ],
  };
  const complete = frame("response.completed", { response });
  const cases = [];
  const add = (
    name,
    adapter,
    text,
    changes = {},
    mime = "text/event-stream; charset=utf-8",
  ) =>
    cases.push({
      name,
      connection: { ...plain, adapter, ...changes },
      text,
      mime,
    });
  add(
    "9router-final",
    "openai-images",
    frame("partial", { b64_json: "ignored" }) + frame("done", image),
    { gateway: "9router" },
  );
  add(
    "9router-no-final",
    "openai-images",
    frame("partial", image) + "data: [DONE]\n\n",
    { gateway: "9router" },
  );
  const router = frame("image_generation.completed", {
    b64_json: encoded,
    usage: { cost: 0 },
  });
  add(
    "openrouter-final",
    "openrouter-images",
    frame("image_generation.partial", { b64_json: "ignored" }) +
      router +
      "data: [DONE]\n\n",
  );
  add("openrouter-truncated-final", "openrouter-images", router);
  add(
    "openrouter-no-usage",
    "openrouter-images",
    frame("image_generation.completed", { b64_json: encoded }) +
      "data: [DONE]\n\n",
  );
  add(
    "responses-first-empty-final",
    "openai-responses",
    frame("response.completed", {}) + complete,
  );
  add(
    "openrouter-only-preview",
    "openrouter-images",
    frame("image_generation.partial", image) + "data: [DONE]\n\n",
  );
  add(
    "responses-final",
    "openai-responses",
    frame("response.image_generation_call.partial_image", {
      partial_image_b64: "ignored",
    }) + complete,
  );
  add(
    "responses-no-envelope",
    "openai-responses",
    frame("response.completed", {}),
  );
  for (const type of ["error", "response.failed", "response.incomplete"]) {
    add(
      `responses-error-${type}`,
      "openai-responses",
      complete + frame(type, { error: "fixture" }),
    );
  }
  for (const status of ["failed", "incomplete"]) {
    add(
      `responses-terminal-${status}`,
      "openai-responses",
      frame(`response.${status}`, { response: { ...response, status } }),
    );
    for (const envelope of [
      2,
      { status },
      { id: " ", status },
      { id: "same-job", status: "running" },
    ])
      add(
        `responses-unconfirmed-${status}-${cases.length}`,
        "openai-responses",
        frame(`response.${status}`, { response: envelope }),
      );
  }
  const thought = frame("", {
    candidates: [
      {
        content: {
          parts: [{ thought: true, inlineData: { data: "ignored" } }],
        },
      },
    ],
  });
  const final = frame("", {
    candidates: [
      {
        finishReason: "MAX_TOKENS",
        content: { parts: [{ inlineData: { data: encoded } }] },
      },
    ],
  });
  add("gemini-final", "gemini", thought + final);
  add("gemini-only-thought", "gemini", thought);
  add("unsupported-stream", "chat-images", frame("done", image));
  add(
    "json-image",
    "openai-images",
    JSON.stringify(image),
    {},
    "application/json",
  );
  add("json-invalid", "openai-images", "{broken", {}, "application/json");
  for (const value of [null, [], 2, "text"])
    add(
      `json-scalar-${cases.length}`,
      "openai-images",
      JSON.stringify(value),
      {},
      "application/json",
    );
  for (const item of cases)
    item.expected = await outcome(() =>
      normalizeResponse(
        item.connection,
        Buffer.from(item.text, "utf8"),
        item.mime,
      ),
    );
  for (const image of images) {
    const connection = { ...plain, adapter: "openai-images" };
    cases.push({
      name: `binary-${image.name}`,
      connection,
      base64: image.base64,
      mime: `${image.metadata.mime}; charset=binary`,
      expected: await outcome(() =>
        normalizeResponse(
          connection,
          Buffer.from(image.base64, "base64"),
          `${image.metadata.mime}; charset=binary`,
        ),
      ),
    });
  }
  const frames = [];
  for (const [name, text] of [
    [
      "crlf-multiline",
      ': keepalive\r\n\r\nevent: progress\r\nid: 1\r\ndata: {\r\ndata: "step": 2}\r\n\r\ndata: {"type":"complete","value":3}\r\n\r\ndata: [DONE]\r\n\r\n',
    ],
    ["empty-and-comments", ": comment\n\nid: 1\n\nevent: ignored\ndata:\n\n"],
    ["unicode", frame("done", { text: "日本語 🌍 café" })],
    ["truncated", "data: {}\n"],
    ...["{bad", "null", "[]", "1"].map((value) => [
      `invalid-${value}`,
      `data: ${value}\n\n`,
    ]),
  ])
    frames.push({
      name,
      text,
      expected: await outcome(() => sseEvents(Buffer.from(text, "utf8"))),
    });
  const normalization = [];
  const rawCases = [
    {
      model: "actual",
      usageMetadata: { tokens: 1 },
      data: [{ b64_json: encoded }, { b64_json: "" }, { url: "" }, 1, null],
    },
    { promptFeedback: 2 },
    { promptFeedback: { blockReason: "SAFETY" } },
    {
      candidates: [
        null,
        { finishReason: ["MAX", null, {}], content: 1 },
        {
          finishReason: false,
          content: {
            parts: [
              null,
              { inlineData: 2 },
              { inlineData: {} },
              { inlineData: { data: encoded } },
            ],
          },
        },
      ],
    },
    { status: "queued" },
    { status: "queued", id: "job", model: "actual" },
    { status: "budget_exceeded", id: "job" },
    { status: "unexpected" },
    { status: {} },
    {
      status: "completed",
      output_image: { data: encoded, uri: "https://assets.example/image.png" },
    },
    { status: "completed", output_image: 1 },
    {
      steps: [
        null,
        {
          type: "model_output",
          content: [
            null,
            { type: "text" },
            { type: "image", uri: "https://assets.example/image.png" },
          ],
        },
      ],
    },
    { choices: [null, { message: null }, { message: { refusal: "blocked" } }] },
    {
      choices: [
        {
          message: {
            images: [
              null,
              { image_url: null },
              { image_url: { url: "https://assets.example/image.png" } },
            ],
          },
        },
      ],
    },
  ];
  for (const adapter of [
    "openai-images",
    "gemini",
    "gemini-interactions",
    "openai-responses",
    "chat-images",
  ]) {
    const connection = { ...plain, adapter };
    for (const raw of rawCases)
      normalization.push({
        connection,
        raw,
        expected: await outcome(() => normalize(connection, raw)),
      });
  }
  await emit("streams", { cases, frames, normalization });
}
