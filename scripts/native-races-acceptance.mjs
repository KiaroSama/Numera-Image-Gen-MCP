import assert from "node:assert/strict";
import { mkdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import {
  provider,
  json,
  call,
  job,
  pair,
  close,
  ownedScenario,
} from "./native-host-fixture.mjs";

function gate() {
  let resolve;
  const promise = new Promise((accept) => {
    resolve = accept;
  });
  return { resolve, promise };
}

function config(root, connection, configSchema) {
  return configSchema.parse({
    schemaVersion: 1,
    defaultConnection: "local",
    outputDir: join(root, "outputs"),
    stateDir: join(root, "state"),
    logging: { directory: join(root, "logs") },
    files: { allowedInputRoots: [join(root, "inputs")] },
    connections: { local: connection },
  });
}

async function responses(state, context, bytes) {
  const root = join(context.workspace, `stdio-responses-race-${state}`);
  await mkdir(root);
  const polled = gate();
  const release = gate();
  let posts = 0;
  let polls = 0;
  await provider(
    async (request, response) => {
      if (request.method === "POST") {
        posts++;
        json(response, { id: "owned", status: "in_progress" });
      } else {
        polls++;
        if (polls === 1) {
          polled.resolve();
          await context.bounded(release.promise);
        }
        json(response, {
          id: "owned",
          status: state,
          ...(["completed", "incomplete"].includes(state)
            ? {
                output: [
                  {
                    type: "image_generation_call",
                    status: "completed",
                    result: bytes.toString("base64"),
                  },
                ],
              }
            : {}),
        });
      }
    },
    async (origin) => {
      const settings = config(
        root,
        {
          adapter: "openai-responses",
          baseUrl: `${origin}/v1`,
          auth: { type: "none" },
          orchestrationModel: "text",
          defaultModel: "image",
          requestTimeoutMs: 5000,
        },
        context.configSchema,
      );
      const instances = new Set();
      const hosts = await pair(root, settings, context, instances);
      let original;
      try {
        original = call(
          hosts[0],
          "generate_image",
          { prompt: "draw", request_id: "race" },
          context.bounded,
        );
        void original.catch(() => {});
        await context.bounded(polled.promise);
        const busy = await job(hosts[1], "race", true, context.bounded);
        assert.equal(busy.status, "running");
        assert.equal(busy.upstream_job?.id, "owned");
        assert.equal(polls, 1);
        release.resolve();
        await original;
        const persisted = await job(hosts[0], "race", true, context.bounded);
        const terminal = await job(hosts[1], "race", true, context.bounded);
        assert.equal(persisted.generation_outcome, state);
        assert.equal(terminal.generation_outcome, state);
        assert.deepEqual(persisted.outputs, terminal.outputs);
        if (["completed", "incomplete"].includes(state)) {
          assert.equal(terminal.outputs.length, 1);
          assert.deepEqual(await readFile(terminal.outputs[0].path), bytes);
        }
        assert.equal(posts, 1);
        assert.equal(polls, 1);
      } finally {
        release.resolve();
        await original?.catch(() => {});
        await close(hosts, context.closeHost);
      }
      for (let restart = 0; restart < 2; restart++) {
        const reopened = await pair(root, settings, context, instances);
        try {
          for (const host of reopened)
            assert.equal(
              (await job(host, "race", true, context.bounded))
                .generation_outcome,
              state,
            );
        } finally {
          await close(reopened, context.closeHost);
        }
      }
      assert.equal(posts, 1);
    },
    context.bounded,
  );
}

async function comfy(state, context, bytes) {
  const root = join(context.workspace, `stdio-comfy-race-${state}`);
  await mkdir(root);
  const observed = gate();
  const release = gate();
  let posts = 0;
  let cancellations = 0;
  let downloads = 0;
  await provider(
    async (request, response) => {
      if (request.url === "/object_info")
        json(response, {
          Text: { input: { required: { text: ["STRING"] } } },
          Save: {
            output_node: true,
            input: { required: { text: ["STRING"] } },
          },
        });
      else if (request.url === "/system_stats")
        json(response, { devices: [{ type: "cuda" }] });
      else if (request.url === "/prompt") {
        posts++;
        json(response, { prompt_id: "owned" });
      } else if (request.url === "/history/owned") {
        if (state !== "completed") {
          observed.resolve();
          await context.bounded(release.promise);
          json(response, {});
        } else
          json(response, {
            owned: {
              status: { completed: true, status_str: "success" },
              outputs: {
                save: {
                  images: [
                    { filename: "final.png", subfolder: "", type: "output" },
                  ],
                },
              },
            },
          });
      } else if (request.url.startsWith("/view")) {
        downloads++;
        if (downloads === 1) {
          observed.resolve();
          await context.bounded(release.promise);
        }
        response.writeHead(200, { "Content-Type": "image/png" });
        response.end(bytes);
      } else if (request.url === "/api/jobs/owned/cancel") {
        cancellations++;
        json(response, { cancelled: true });
      } else if (request.url === "/api/jobs/owned")
        json(response, {
          id: "owned",
          status: state === "completed" ? "cancelled" : state,
        });
      else json(response, {}, 404);
    },
    async (origin) => {
      const settings = config(
        root,
        {
          adapter: "comfyui",
          baseUrl: origin,
          baseUrlMode: "origin",
          auth: { type: "none" },
          defaultModel: "workflow",
          requestTimeoutMs: 5000,
          workflow: {
            graph: {
              text: { class_type: "Text", inputs: { text: "" } },
              save: { class_type: "Save", inputs: { text: "output" } },
            },
            bindings: { prompt: { node: "text", input: "text" } },
            outputNodes: ["save"],
          },
        },
        context.configSchema,
      );
      const instances = new Set();
      let hosts = await pair(root, settings, context, instances);
      let original;
      try {
        original = call(
          hosts[0],
          "generate_image",
          { prompt: "draw", request_id: "race" },
          context.bounded,
        );
        void original.catch(() => {});
        await context.bounded(observed.promise);
        const cancelled = await call(
          hosts[1],
          "cancel_job",
          { request_id: "race" },
          context.bounded,
        );
        assert.notEqual(cancelled.isError, true);
        assert.equal(cancelled.structuredContent.refund_verified, false);
        assert.equal(
          cancelled.structuredContent.upstream_cancelled,
          state === "cancelled",
        );
        await original;
        release.resolve();
        const current = await job(hosts[1], "race", false, context.bounded);
        assert.equal(current.generation_outcome, state);
        if (state !== "completed") assert.equal(current.status, state);
      } finally {
        release.resolve();
        await original?.catch(() => {});
        await close(hosts, context.closeHost);
      }
      let outputId;
      for (let restart = 0; restart < 2; restart++) {
        hosts = await pair(root, settings, context, instances);
        try {
          for (const host of hosts) {
            const current = await job(host, "race", true, context.bounded);
            assert.equal(current.generation_outcome, state);
            assert.equal(current.status, state);
            if (state === "completed") {
              assert.equal(current.outputs.length, 1);
              assert.deepEqual(await readFile(current.outputs[0].path), bytes);
              if (outputId)
                assert.equal(current.outputs[0].output_id, outputId);
              outputId = current.outputs[0].output_id;
            }
          }
        } finally {
          await close(hosts, context.closeHost);
        }
      }
      assert.equal(posts, 1);
      assert.equal(cancellations, 1);
      assert.equal(downloads, state === "completed" ? 2 : 0);
    },
    context.bounded,
  );
}

// Seven frozen actual-stdio races, not seeded Store or Application-only substitutes.
export async function racesAcceptance(context) {
  const images = JSON.parse(
    await readFile(
      join(context.root, ".ci-work/parity/image-acceptance.json"),
      "utf8",
    ),
  );
  const original = images.find((image) => image.name === "png");
  assert.ok(original);
  assert.equal(original.metadata.width, 8);
  assert.equal(original.metadata.height, 6);
  const bytes = Buffer.from(original.base64, "base64");
  for (const state of ["failed", "cancelled", "incomplete", "completed"]) {
    await ownedScenario(
      context,
      (scoped) => responses(state, scoped, bytes),
      30000,
    );
    process.stderr.write(
      JSON.stringify({
        component: "native-races-acceptance",
        adapter: "openai-responses",
        state,
        message: "Actual two-host race and restarts completed.",
      }) + "\n",
    );
  }
  for (const state of ["cancelled", "failed", "completed"]) {
    await ownedScenario(
      context,
      (scoped) => comfy(state, scoped, bytes),
      30000,
    );
    process.stderr.write(
      JSON.stringify({
        component: "native-races-acceptance",
        adapter: "comfyui",
        state,
        message: "Actual two-host race and restarts completed.",
      }) + "\n",
    );
  }
}
