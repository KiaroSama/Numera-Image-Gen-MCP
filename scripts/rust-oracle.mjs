import { readFile, mkdir, writeFile } from "node:fs/promises";
import { resolve, join } from "node:path";
import { pathToFileURL } from "node:url";
import { createHash } from "node:crypto";

// CI-only source oracle. Output is workspace-contained and never includes private inputs.
const root = resolve(process.env.GITHUB_WORKSPACE ?? process.cwd());
const baseline = resolve(process.env.NUMERA_ORACLE_ROOT ?? root);
const output = join(root, ".ci-work", "parity");
const source = (file) =>
  import(pathToFileURL(join(baseline, "dist", file)).href);
const { fingerprint } = await source("jobs/store.js");
const stable = (v) =>
  Array.isArray(v)
    ? v.map(stable)
    : v && typeof v === "object"
      ? Object.fromEntries(
          Object.entries(v)
            .filter(([, x]) => x !== undefined)
            .sort(([a], [b]) => a.localeCompare(b, "en"))
            .map(([k, x]) => [k, stable(x)]),
        )
      : v;
const cases = [
  { name: "null", input: null },
  {
    name: "numeric-boundaries",
    input: [0, -0, 1e-7, 1e-6, 1e20, 1e21, 9007199254740991],
  },
  {
    name: "english-key-order",
    input: { A: 1, a: 2, Z: 3, z: 4, _: 5, "-": 6, é: 7, e: 8 },
  },
  {
    name: "integer-property-order",
    input: { 10: 1, 2: 2, "01": 3, 4294967295: 4, 4294967294: 5 },
  },
  {
    name: "nested-unicode",
    input: { سلام: "🌍", nested: { z: [null, false], A: { e: "é", é: "é" } } },
  },
  {
    name: "escaping",
    input: { text: 'quote" slash/ back\\ newline\n\t\u0000 ' },
  },
  { name: "ordered-references", input: [{ hash: "a" }, { hash: "b" }] },
];
await mkdir(output, { recursive: true });
const emit = (name, value) =>
  writeFile(
    join(output, name + ".json"),
    JSON.stringify(value, null, 2) + "\n",
    "utf8",
  );
await emit(
  "canonical",
  cases.map((c) => ({
    ...c,
    canonical: JSON.stringify(stable(c.input)),
    fingerprint: fingerprint(c.input),
  })),
);
const { createRequire } = await import("node:module");
const baselineRequire = createRequire(join(baseline, "package.json"));
const { z } = baselineRequire("zod");
const { configSchema, requestSchema } = await source("config/schema.js");
for (const [name, schema] of [
  ["config-input", configSchema],
  ["image-request", requestSchema],
]) {
  await emit(name + ".schema", z.toJSONSchema(schema, { io: "input" }));
}
const { loadConfig, selectConnection } = await source("config/load.js");
const { parseEnvConfig } = await source("config/env.js");
const outcome = async (action) => {
  try {
    return { value: await action() };
  } catch (error) {
    if (typeof error?.code !== "string") throw error;
    return { error: error.code };
  }
};
const compact = {
  api_endpoint: "https://images.example/v1",
  api_key: "synthetic-oracle-only",
  models: [{ id: "opaque/image", name: "تصویر 日本語 🌍" }],
};
const advanced = {
  schemaVersion: 1,
  defaultConnection: "default",
  outputDir: join(output, "outputs"),
  stateDir: join(output, "state"),
  logging: { directory: join(output, "logs") },
  connections: {
    default: {
      adapter: "openai-images",
      baseUrl: "https://images.example/v1",
      auth: { type: "none" },
    },
    disabled: {
      adapter: "gemini",
      baseUrl: "https://gemini.example/v1beta",
      auth: { type: "none" },
      enabled: false,
    },
  },
};
const envText =
  'API_ENDPOINT=https://images.example/v1\nAPI_KEY="synthetic-oracle-only"\nMODEL_1_ID=opaque/image\nMODEL_1_NAME="تصویر #1"\n';
const parse = [];
const add = (name, input, args = [], env = {}, filename = "config.json") =>
  parse.push({
    name,
    text: typeof input === "string" ? input : JSON.stringify(input),
    file: join(output, filename),
    args,
    env,
  });
add("compact", compact);
add("advanced", advanced);
add("advanced-no-default", { ...advanced, defaultConnection: undefined });
add(
  "flags-over-environment",
  advanced,
  [
    "--output-dir",
    join(output, "flag"),
    "--log-level",
    "DEBUG",
    "--request-timeout-ms",
    "900",
  ],
  {
    NUMERA_OUTPUT_DIR: join(output, "env"),
    NUMERA_STATE_DIR: join(output, "env-state"),
    NUMERA_LOG_LEVEL: "ERROR",
    NUMERA_REQUEST_TIMEOUT_MS: "1000",
  },
);
add("dotenv", envText, [], {}, ".env");
for (const profile of [
  "gemini",
  "gemini-interactions",
  "chat-images",
  "openrouter-images",
  "omniroute",
  "9router",
]) {
  add("profile-" + profile, { ...compact, profile });
}
add("responses", {
  ...compact,
  profile: "openai-responses",
  orchestration_model: "opaque/orchestrator",
});
add("disabled-null-models", {
  ...compact,
  models: [
    {
      id: "unused",
      name: "Disabled",
      enabled: false,
      output_format: null,
      size: null,
      quality: null,
    },
    {
      id: "actual",
      name: "Actual",
      output_format: null,
      size: null,
      quality: null,
    },
  ],
});
for (const [name, change] of Object.entries({
  blank_key: { api_key: " " },
  newline_key: { api_key: "bad\nkey" },
  empty_models: { models: [] },
  duplicate_models: {
    models: [
      { id: "a", name: "A" },
      { id: "a", name: "B" },
    ],
  },
  blank_id: { models: [{ id: " ", name: "Name" }] },
  blank_name: { models: [{ id: "a", name: " " }] },
  bad_profile: { profile: "guessed" },
  missing_orchestrator: { profile: "openai-responses" },
  unexpected_orchestrator: { orchestration_model: "unused" },
  extra: { extra: true },
  credential_url: { api_endpoint: "https://user:password@images.example/v1" },
  credential_query: { api_endpoint: "https://images.example/v1?key=synthetic" },
  invalid_format: { models: [{ id: "a", name: "A", output_format: "gif" }] },
}))
  add("invalid-compact-" + name, { ...compact, ...change });
for (const [name, change] of Object.entries({
  version: { schemaVersion: 2 },
  extra: { extra: true },
  relative_output: { outputDir: "relative" },
  absent_default: { defaultConnection: "missing" },
  zero_concurrency: { maxConcurrentRequests: 0 },
  log_prompts: {
    logging: { directory: join(output, "logs"), logPrompts: true },
  },
}))
  add("invalid-advanced-" + name, { ...advanced, ...change });
for (const n of [1, 127, 129, 512]) {
  let nested = "leaf";
  for (let i = 0; i < n; i++) nested = { nested };
  add("nested-unknown-" + n, {
    ...advanced,
    connections: {
      ...advanced.connections,
      default: { ...advanced.connections.default, extensions: { nested } },
    },
  });
}
const dotenv = [];
for (const [name, suffix] of [
  ["valid", ""],
  ["empty-settings", 'MODEL_1_SIZE=\nMODEL_1_QUALITY=""\n'],
  [
    "multiline",
    'MODEL_2_ID=another\nexport MODEL_2_NAME="line one\nline two" # comment\n',
  ],
  ["literal-newline", 'MODEL_2_ID=another\nMODEL_2_NAME="line\\ntwo"\n'],
  ["duplicate", "API_KEY=duplicate\n"],
  ["unknown-key", "PROFILEE=typo\n"],
  ["invalid-boolean", "MODEL_1_ENABLED=yes\n"],
  ["overflow", "MODEL_101_ID=overflow\n"],
  ["leading-zero", "MODEL_01_ID=zero\n"],
  ["broken", "broken line\n"],
  ["quote", 'MODEL_1_SIZE="unterminated\n'],
  ["quote-tail", 'MODEL_1_SIZE="512x512" garbage\n'],
  ["edit-without-mode", "EDIT_MASKS=true\n"],
  [
    "valid-edit",
    "EDIT_MODE=json\nEDIT_ENCODING=images\nEDIT_MAX_REFERENCES=4\nEDIT_MASKS=true\nEDIT_MASK_POLARITY=white-edit\n",
  ],
]) {
  const text = envText + suffix;
  dotenv.push({
    name,
    text,
    expected: await outcome(() => parseEnvConfig(text)),
  });
}
for (const item of parse)
  item.expected = await outcome(() =>
    loadConfig(["--config", item.file, ...item.args], item.env, item.text),
  );
const select = [];
for (const [name, config, connection] of [
  ["default", "advanced", null],
  ["explicit", "advanced", "default"],
  ["disabled", "advanced", "disabled"],
  ["missing", "advanced", "missing"],
  ["no-default", "advanced-no-default", null],
]) {
  const item = parse.find((p) => p.name === config);
  if (!item?.expected.value)
    throw new Error("Selection fixture requires valid baseline config");
  const result = await outcome(() => {
    const [id, value] = selectConnection(
      item.expected.value,
      connection ?? undefined,
    );
    return { id, value };
  });
  select.push({
    name,
    config,
    connection,
    expected: result.error ? result : result.value,
  });
}
await emit("config", { parse, select, dotenv });
for (const [name, args] of [
  ["unknown-option", ["--unknown"]],
  ["missing-value", ["--log-level"]],
  ["negative-separated", ["--request-timeout-ms", "-1"]],
  ["negative-inline", ["--request-timeout-ms=-1"]],
  ["positional", ["unexpected"]],
  ["boolean-inline", ["--openai-compat=true"]],
]) {
  const item = {
    name,
    text: JSON.stringify(advanced),
    file: join(output, "config.json"),
    args,
    env: {},
  };
  item.expected = await outcome(() =>
    loadConfig(["--config", item.file, ...args], item.env, item.text),
  );
  parse.push(item);
}
await emit("config", { parse, select, dotenv });
const { capabilities, effectiveRequest, safeOptions } =
  await source("capabilities.js");
const { imageDimensions, dimensionEvidence, validateImageDimensions } =
  await source("services/image-dimensions.js");
const policy = [];
for (const adapter of [
  "openai-images",
  "gemini",
  "gemini-interactions",
  "openai-responses",
  "openrouter-images",
  "chat-images",
  "comfyui",
]) {
  for (const model of [
    "opaque/image",
    "antigravity/gemini-3.1-flash-image",
    "models/gemini-3-pro-image-preview",
    "cx/gpt-image-2",
  ]) {
    const connection = configSchema.parse({
      ...advanced,
      connections: { default: { ...advanced.connections.default, adapter } },
    }).connections.default;
    for (const gateway of [undefined, "omniroute", "9router"]) {
      const c = { ...connection, ...(gateway ? { gateway } : {}) };
      policy.push({
        name: `${adapter}-${gateway ?? "native"}-${model}`,
        connection: c,
        model,
        capabilities: capabilities(c, model),
        dimensions: imageDimensions(c, model, { maxPixels: 64000000 }),
        evidence: dimensionEvidence(c, model),
      });
    }
  }
}
const effective = [];
const plain = configSchema.parse(advanced).connections.default;
for (const [name, input, changes] of [
  [
    "defaults",
    {},
    {
      defaults: { quality: "low", size: "small" },
      modelOverrides: { model: { defaults: { quality: "high" }, maxCount: 3 } },
    },
  ],
  ["batch", { count: 2 }, {}],
  [
    "jpeg-transparent",
    { output_format: "jpeg", background: "transparent" },
    {},
  ],
  [
    "conflict",
    { quality: "high", provider_options: { quality: "low" } },
    { providerOptionKeys: ["quality"] },
  ],
  ["denied", {}, { deniedModels: ["model"] }],
  [
    "unverified-reference",
    { reference_images: [{ type: "path", path: "/synthetic.png" }] },
    {},
  ],
]) {
  const connection = { ...plain, ...changes },
    request = requestSchema.parse({ prompt: "Draw a café 🌍", ...input });
  effective.push({
    name,
    connection,
    request,
    model: "model",
    expected: await outcome(() =>
      effectiveRequest(connection, request, "model"),
    ),
  });
}
const options = [];
for (const input of [
  { strength: 0.5 },
  { nested: { api_key: "synthetic" } },
  { choices: [{ x: 1 }] },
  { choices: [1, null, "two"] },
]) {
  options.push({ input, expected: await outcome(() => safeOptions(input)) });
}
const dimensionValidation = [];
for (const model of [
  "opaque/image",
  "gemini-3.1-flash-image",
  "gemini-3-pro-image",
  "cx/gpt-image-2",
]) {
  for (const request of [
    { size: "3840x2160" },
    { size: "4096x4096" },
    { aspect_ratio: "1:8" },
    { image_size: "0.5K" },
  ]) {
    dimensionValidation.push({
      connection: plain,
      model,
      request,
      expected: await outcome(() => {
        validateImageDimensions(plain, model, request);
        return true;
      }),
    });
  }
}
await emit("policy", { policy, effective, options, dimensionValidation });
const sharp = baselineRequire("sharp");
const { inspectImage, decodeBase64 } = await source("files/images.js");
const imageConfig = configSchema.parse(advanced);
const { imageEdgeOracle } = await import("./rust-image-edge-oracle.mjs");
await imageEdgeOracle({ sharp, inspectImage, config: imageConfig, emit });
const images = [];
for (const format of ["png", "jpeg", "webp"]) {
  for (const alpha of format === "jpeg" ? [false] : [false, true]) {
    const bytes = await sharp({
      create: {
        width: 7,
        height: 5,
        channels: alpha ? 4 : 3,
        background: { r: 140, g: 30, b: 210, alpha: alpha ? 0.5 : 1 },
      },
    })
      .toFormat(format)
      .toBuffer();
    const { bytes: original, ...metadata } = await inspectImage(
      bytes,
      imageConfig,
    );
    images.push({
      name: `${format}-${alpha ? "alpha" : "opaque"}`,
      base64: original.toString("base64"),
      metadata,
    });
  }
}
await emit("images", images);
const { buildRequest } = await source("adapters/index.js");
const { normalize, normalizeResponse } = await source("adapters/normalize.js");
const image = await inspectImage(
  Buffer.from(images[0].base64, "base64"),
  imageConfig,
);
const wires = [];
for (const adapter of [
  "openai-images",
  "gemini",
  "gemini-interactions",
  "openai-responses",
  "openrouter-images",
  "chat-images",
  "comfyui",
]) {
  const connection = {
    ...plain,
    adapter,
    chatImageOutput: true,
    orchestrationModel: "opaque/orchestrator",
  };
  for (const refs of [false, true]) {
    const request = requestSchema.parse({
      prompt: "Draw a café 🌍",
      ...(refs ? {} : { size: "1024x1024" }),
    });
    const input = {
      connection,
      request,
      model: "opaque/image",
      operation: refs ? "edit" : "generate",
      references: refs ? [image] : [],
    };
    wires.push({
      name: `${adapter}-${refs ? "edit" : "generate"}`,
      connection,
      request,
      model: input.model,
      operation: input.operation,
      references: refs ? [images[0]] : [],
      expected: await outcome(async () => {
        const prepared = buildRequest(input);
        if (typeof prepared.body === "string")
          return {
            operation: prepared.operation,
            prefix: prepared.prefix,
            body: JSON.parse(prepared.body),
          };
        const multipart = [];
        for (const [name, value] of prepared.body.entries()) {
          multipart.push(
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
        return {
          operation: prepared.operation,
          prefix: prepared.prefix,
          multipart,
        };
      }),
    });
  }
}
const normalization = [];
for (const adapter of [
  "openai-images",
  "gemini",
  "gemini-interactions",
  "openai-responses",
  "openrouter-images",
  "chat-images",
]) {
  const connection = { ...plain, adapter };
  for (const raw of [
    null,
    {},
    {
      data: [
        null,
        { b64_json: images[0].base64 },
        { url: "https://assets.example/image.png" },
      ],
    },
    { status: "queued", id: "same-job", model: "opaque/image" },
    { status: "failed", id: "same-job", data: [] },
    {
      status: "completed",
      id: "same-job",
      output: [
        {
          type: "image_generation_call",
          status: "completed",
          result: images[0].base64,
        },
      ],
    },
    {
      candidates: [
        {
          finishReason: "STOP",
          content: {
            parts: [
              { thought: true, thoughtSignature: "synthetic-private" },
              { inlineData: { data: images[0].base64 } },
            ],
          },
        },
      ],
    },
    {
      steps: [
        { type: "thought", signature: "synthetic-private" },
        {
          type: "model_output",
          content: [{ type: "image", data: images[0].base64 }],
        },
      ],
    },
    {
      choices: [
        {
          message: {
            images: [
              { image_url: { url: "https://assets.example/image.png" } },
            ],
          },
        },
      ],
    },
  ])
    normalization.push({
      connection,
      raw,
      expected: await outcome(() => normalize(connection, raw)),
    });
}
await emit("adapters", { wires, normalization });
const { normalizationOracle } = await import("./rust-normalization-oracle.mjs");
await normalizationOracle({ source, emit, sharp });
const { imageAcceptanceOracle } =
  await import("./rust-image-acceptance-oracle.mjs");
await imageAcceptanceOracle({ source, emit, sharp });
const { requirementsOracle } = await import("./rust-requirements-oracle.mjs");
await requirementsOracle({ source, emit, plain, images });
const { streamOracle } = await import("./rust-stream-oracle.mjs");
await streamOracle({ source, emit, plain, images, outcome });
const { streamAcceptanceOracle } =
  await import("./rust-stream-acceptance-oracle.mjs");
await streamAcceptanceOracle({ source, emit, sharp });
const { jsonOracle } = await import("./rust-json-oracle.mjs");
await jsonOracle({ emit, fingerprint, configSchema, requestSchema });
if (process.platform === "win32") {
  const { spawn } = await import("node:child_process");
  const file = join(output, "current-user-dpapi.txt");
  const script =
    '$ErrorActionPreference="Stop"; $s=ConvertTo-SecureString "synthetic-dpapi-秘密" -AsPlainText -Force; $encrypted=ConvertFrom-SecureString $s; [IO.File]::WriteAllText($env:NUMERA_DPAPI_FIXTURE,$encrypted,[Text.UTF8Encoding]::new($false))';
  await new Promise((accept, reject) => {
    const child = spawn(
      "pwsh",
      ["-NoProfile", "-NonInteractive", "-Command", script],
      {
        windowsHide: true,
        stdio: ["ignore", "ignore", "pipe"],
        env: {
          ...process.env,
          NUMERA_DPAPI_FIXTURE: file,
          TEMP: output,
          TMP: output,
          TMPDIR: output,
        },
      },
    );
    child.stderr.resume();
    const deadline = setTimeout(() => {
      child.kill();
      reject(new Error("DPAPI fixture deadline exceeded"));
    }, 10000);
    child.once("error", (error) => {
      clearTimeout(deadline);
      reject(error);
    });
    child.once("close", (code) => {
      clearTimeout(deadline);
      code === 0
        ? accept()
        : reject(new Error("DPAPI fixture generation failed"));
    });
  });
  const { credentials } = await source("config/credentials.js");
  const connection = {
    ...plain,
    auth: { type: "bearer", secretDpapiFile: file },
  };
  await emit("config-dpapi", {
    file,
    expected: await credentials(connection, process.env),
  });
}
await emit(
  "base64",
  await Promise.all(
    [
      "Zg==",
      "Zh==",
      "Zm8=",
      "Zm9=",
      "AAAA",
      "AAAA\n",
      "",
      "=AAA",
      "AA==",
      "A===",
    ].map(async (input) => ({
      input,
      expected: await outcome(() => decodeBase64(input, 1024).toString("hex")),
    })),
  ),
);
const manifest = {
  baseline: "fc7da25c3b470199949b1b4c67127d77afa0c34a",
  runtime: process.version,
  canonical_cases: cases.length,
  config_cases: parse.length + select.length + dotenv.length,
  image_cases: images.length,
  source_schema_sha256: createHash("sha256")
    .update(await readFile(join(baseline, "src/config/schema.ts")))
    .digest("hex"),
};
await emit("manifest", manifest);
process.stderr.write(
  JSON.stringify({
    component: "rust-oracle",
    message: "Source oracle generated.",
    ...manifest,
  }) + "\n",
);
