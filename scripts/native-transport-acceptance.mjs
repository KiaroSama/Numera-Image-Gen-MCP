import assert from "node:assert/strict";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { provider, json, call, ownedScenario } from "./native-host-fixture.mjs";

// Frozen transport-boundaries literals; native stdio replaces InMemoryTransport.
// The parent seed Store complements the same-owner Application fixtures.
function receipt(id) {
  return {
    request_id: id,
    connection: "local",
    adapter: "openai-images",
    operation: "generate",
    status: "prepared",
    requested_model: "image-model",
    upstream_model: null,
    outputs: [],
    warnings: [],
    deviations: [],
    errors: [],
    generation_outcome: "not_submitted",
    storage_outcome: "not_started",
    another_submission_may_charge: false,
    created_at: "2026-10-02T00:00:00.000Z",
    updated_at: "2026-10-02T00:00:00.000Z",
  };
}

function configuration(context, directory, origin, adapter) {
  const config = context.configSchema.parse({
    schemaVersion: 1,
    defaultConnection: "local",
    outputDir: join(directory, "outputs"),
    stateDir: join(directory, "state"),
    logging: { directory: join(directory, "logs") },
    files: { allowedInputRoots: [join(directory, "inputs")] },
    connections: {
      local: {
        adapter,
        baseUrl: `${origin}/v1`,
        auth: { type: "none" },
        orchestrationModel: "text",
        defaultModel: "image",
      },
    },
  });
  assert.equal(config.connections.local.requestTimeoutMs, 300000);
  assert.equal(config.connections.local.discoveryTimeoutMs, 15000);
  return config;
}

async function withHost(context, directory, origin, adapter, action) {
  const config = configuration(context, directory, origin, adapter);
  const file = join(directory, "config.json");
  await writeFile(file, JSON.stringify(config), "utf8");
  const store = new context.Store(config.stateDir);
  let host;
  try {
    context.signal.throwIfAborted();
    host = await context.start(
      file,
      "auto",
      { signal: context.signal },
      {
        clientInfo: { name: "transport-audit", version: "1" },
      },
    );
    host.fixtureSignal = context.signal;
    assert.equal(host.client.getProtocolEra(), "legacy");
    await action(host, store, config);
  } finally {
    try {
      if (host) await context.closeHost(host);
    } finally {
      store.close();
    }
  }
}

async function structured(host, name, args, bounded) {
  const result = await call(host, name, args, bounded);
  assert.ok(
    result.structuredContent,
    "actual SDK structuredContent is required",
  );
  const value = result.structuredContent;
  assert.equal(
    result.isError === true,
    name === "generate_image" &&
      ["failed", "outcome_unknown"].includes(value.status),
    "the SDK error flag must preserve the structured receipt",
  );
  return value;
}

async function savedOriginal(result, bytes) {
  assert.equal(result.outputs.length, 1);
  assert.deepEqual(await readFile(result.outputs[0].path), bytes);
}

async function terminal(context, state, bytes) {
  const directory = join(context.workspace, `transport-terminal-${state}`);
  await mkdir(directory);
  let posts = 0;
  await provider(
    (request, response) => {
      if (request.method === "POST") posts++;
      const data = {
        response: {
          id: "owned",
          status: state,
          output:
            state === "incomplete"
              ? [
                  {
                    type: "image_generation_call",
                    status: "completed",
                    result: bytes.toString("base64"),
                  },
                ]
              : [],
        },
      };
      response.writeHead(200, { "Content-Type": "text/event-stream" });
      const frame = `event: response.${state}\ndata: ${JSON.stringify(data)}\n\n`;
      response.write(frame.slice(0, 19));
      response.end(frame.slice(19));
    },
    async (origin) => {
      await withHost(
        context,
        directory,
        origin,
        "openai-responses",
        async (host, store) => {
          const args = { prompt: "fixture", request_id: "sse" };
          const result = await structured(
            host,
            "generate_image",
            args,
            context.bounded,
          );
          assert.equal(result.generation_outcome, state);
          assert.equal(
            result.status,
            state === "failed" ? "failed" : "partial",
          );
          if (state === "incomplete") await savedOriginal(result, bytes);
          await call(host, "generate_image", args, context.bounded);
          assert.equal(posts, 1);
          store.prepare(receipt("next"), "next", "fixture");
          assert.equal(store.admit("next", "local", 1, 1), true);
        },
      );
    },
    context.bounded,
  );
}

async function uncertain(context, mode) {
  const directory = join(context.workspace, `transport-uncertain-${mode}`);
  await mkdir(directory);
  let posts = 0;
  await provider(
    (request, response) => {
      if (request.method === "POST") posts++;
      response.writeHead(200, { "Content-Type": "text/event-stream" });
      response.end(
        mode === "error"
          ? 'event: error\ndata: {"error":"synthetic"}\n\n'
          : `event: response.failed\ndata: ${JSON.stringify({
              response: {
                status: "failed",
                ...(mode === "empty-terminal-id"
                  ? { id: "" }
                  : mode === "blank-terminal-id"
                    ? { id: " " }
                    : {}),
              },
            })}\n\n`,
      );
    },
    async (origin) => {
      await withHost(
        context,
        directory,
        origin,
        "openai-responses",
        async (host) => {
          const result = await structured(
            host,
            "generate_image",
            { prompt: "fixture", request_id: "uncertain" },
            context.bounded,
          );
          assert.equal(result.status, "outcome_unknown");
          assert.deepEqual(result.outputs, []);
          assert.equal(posts, 1);
        },
      );
    },
    context.bounded,
  );
}

async function identity(context, adapter, returned, bytes) {
  const directory = join(
    context.workspace,
    `transport-identity-${adapter}-${returned ?? "missing"}`,
  );
  await mkdir(directory);
  let posts = 0;
  await provider(
    (request, response) => {
      if (request.method === "POST") {
        posts++;
        json(response, { id: "owned", status: "in_progress" });
      } else {
        json(response, {
          ...(returned ? { id: returned } : {}),
          status: "completed",
          ...(adapter === "openai-responses"
            ? {
                output: [
                  {
                    type: "image_generation_call",
                    status: "completed",
                    result: bytes.toString("base64"),
                  },
                ],
              }
            : {
                steps: [
                  {
                    type: "model_output",
                    content: [
                      { type: "image", data: bytes.toString("base64") },
                    ],
                  },
                ],
              }),
        });
      }
    },
    async (origin) => {
      await withHost(
        context,
        directory,
        origin,
        adapter,
        async (host, store, config) => {
          const first = await structured(
            host,
            "generate_image",
            { prompt: "fixture", request_id: "waiting" },
            context.bounded,
          );
          const saved = store.prepare(
            receipt("detached"),
            "fixture",
            await context.connectionIdentity("local", config.connections.local),
          ).receipt;
          assert.equal(store.admit("detached", "local", 4, 4), true);
          saved.status = "running";
          saved.generation_outcome = "running";
          saved.adapter = adapter;
          saved.upstream_job = {
            id: "owned",
            kind: adapter === "openai-responses" ? "responses" : "interactions",
          };
          store.update(saved);
          store.release("detached", saved);
          const second = await structured(
            host,
            "get_job",
            { request_id: "detached", refresh: true },
            context.bounded,
          );
          for (const result of [first, second]) {
            if (returned === "owned") await savedOriginal(result, bytes);
            else {
              assert.equal(result.status, "running");
              assert.deepEqual(result.outputs, []);
              assert.ok(
                result.errors.some(
                  (error) => error.code === "invalid_response",
                ),
              );
            }
          }
          assert.equal(posts, 1);
        },
      );
    },
    context.bounded,
  );
}

async function comfyHistory(context) {
  const directory = join(context.workspace, "transport-comfy-error-history");
  await mkdir(directory);
  let views = 0;
  let posts = 0;
  await provider(
    (request, response) => {
      if (request.url?.startsWith("/view")) views++;
      if (request.method === "POST") posts++;
      assert.equal(request.method, "GET");
      assert.equal(request.url, "/history/job");
      json(response, {
        job: {
          status: { status_str: "error" },
          outputs: {
            save: { images: [{ filename: "not-output.png", type: "input" }] },
          },
        },
      });
    },
    async (origin) => {
      const config = context.configSchema.parse({
        schemaVersion: 1,
        defaultConnection: "local",
        outputDir: join(directory, "outputs"),
        stateDir: join(directory, "state"),
        logging: { directory: join(directory, "logs") },
        files: { allowedInputRoots: [join(directory, "inputs")] },
        connections: {
          local: {
            adapter: "comfyui",
            baseUrl: origin,
            auth: { type: "none" },
            discoveryTimeoutMs: 2000,
            requestTimeoutMs: 2000,
            workflow: {
              graph: {
                text: {
                  class_type: "CLIPTextEncode",
                  inputs: { text: "original" },
                },
                sampler: {
                  class_type: "KSampler",
                  inputs: { seed: 0, batch_size: 1 },
                },
                load: {
                  class_type: "LoadImage",
                  inputs: { image: "original.png", mask: "mask.png" },
                },
                second: {
                  class_type: "LoadImage",
                  inputs: { image: "second.png" },
                },
                save: {
                  class_type: "SaveImage",
                  inputs: { images: ["sampler", 0] },
                },
              },
              bindings: {
                prompt: { node: "text", input: "text" },
                seed: { node: "sampler", input: "seed" },
                count: { node: "sampler", input: "batch_size" },
                image: { node: "load", input: "image" },
                image_2: { node: "second", input: "image" },
                mask: { node: "load", input: "mask" },
              },
              outputNodes: ["save"],
              requireGpu: true,
            },
          },
        },
      });
      const file = join(directory, "config.json");
      await writeFile(file, JSON.stringify(config), "utf8");
      const store = new context.Store(config.stateDir);
      let host;
      try {
        const initial = receipt("history");
        initial.status = "running";
        initial.generation_outcome = "running";
        initial.adapter = "comfyui";
        initial.upstream_job = { id: "job", kind: "comfyui" };
        const seeded = store.prepare(
          initial,
          "hash",
          await context.connectionIdentity("local", config.connections.local),
        ).receipt;
        // The native process must claim the released seed; same-owner Rust proof is separate.
        store.release("history", seeded);
        context.signal.throwIfAborted();
        host = await context.start(
          file,
          "auto",
          { signal: context.signal },
          { clientInfo: { name: "comfy-history-audit", version: "1" } },
        );
        host.fixtureSignal = context.signal;
        const result = await structured(
          host,
          "get_job",
          { request_id: "history", refresh: true },
          context.bounded,
        );
        assert.equal(result.status, "failed");
        assert.equal(result.generation_outcome, "failed");
        assert.deepEqual(result.outputs, []);
        assert.ok(
          result.errors.some((error) => error.code === "provider_rejection"),
        );
        assert.ok(
          store
            .get("history")
            .errors.some((error) => error.code === "provider_rejection"),
        );
        assert.equal(views, 0);
        assert.equal(posts, 0);
      } finally {
        try {
          if (host) await context.closeHost(host);
        } finally {
          store.close();
        }
      }
    },
    context.bounded,
  );
}

export async function transportAcceptance(context) {
  const images = JSON.parse(
    await readFile(
      join(context.root, ".ci-work/parity/image-acceptance.json"),
      "utf8",
    ),
  );
  const image = images.find((item) => item.name === "png");
  assert.ok(image);
  assert.equal(image.metadata.width, 8);
  assert.equal(image.metadata.height, 6);
  const bytes = Buffer.from(image.base64, "base64");
  for (const state of ["failed", "incomplete"]) {
    await ownedScenario(
      context,
      (scoped) => terminal(scoped, state, bytes),
      10000,
    );
    process.stderr.write(
      JSON.stringify({
        component: "native-transport-acceptance",
        terminal: state,
        completed: true,
      }) + "\n",
    );
  }
  for (const mode of [
    "error",
    "malformed-terminal",
    "empty-terminal-id",
    "blank-terminal-id",
  ]) {
    await ownedScenario(context, (scoped) => uncertain(scoped, mode), 10000);
    process.stderr.write(
      JSON.stringify({
        component: "native-transport-acceptance",
        uncertain: mode,
        completed: true,
      }) + "\n",
    );
  }
  for (const adapter of ["openai-responses", "gemini-interactions"]) {
    for (const returned of ["other", undefined, "owned"]) {
      await ownedScenario(
        context,
        (scoped) => identity(scoped, adapter, returned, bytes),
        10000,
      );
      process.stderr.write(
        JSON.stringify({
          component: "native-transport-acceptance",
          adapter,
          returned: returned ?? "missing",
          completed: true,
        }) + "\n",
      );
    }
  }
  await ownedScenario(context, comfyHistory, 10000);
  process.stderr.write(
    JSON.stringify({
      component: "native-transport-acceptance",
      comfy_history: "provider_rejection",
      completed: true,
    }) + "\n",
  );
}
