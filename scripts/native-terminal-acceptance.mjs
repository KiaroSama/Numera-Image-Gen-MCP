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

export function probeStages(before, after) {
  if (!after.startsWith(before)) return { capture: "unobserved", stages: [] };
  const stages = [];
  let firstTimestamp;
  for (const line of after.slice(before.length).split("\n").slice(0, -1)) {
    let event;
    try {
      event = JSON.parse(line);
    } catch {
      continue;
    }
    if (!event || typeof event !== "object") continue;
    let stage;
    if (event.component === "tool" && event.tool === "generate_image") {
      stage =
        event.message === "Tool started."
          ? "started"
          : event.message === "Tool completed."
            ? "completed"
            : event.message === "Tool failed."
              ? "failed"
              : undefined;
    } else if (
      event.component === "generation" &&
      event.message === "Submission intent persisted."
    )
      stage = "intent";
    else if (
      event.component === "http" &&
      event.message === "Submission transport prepared."
    )
      stage = "transport_prepared";
    if (!stage) continue;
    const entry = { stage };
    const fields =
      stage === "transport_prepared"
        ? ["client_build_ms", "pre_send_ms", "timeout_ms"]
        : stage === "intent"
          ? [
              "before_submit_ms",
              "prepare_ms",
              "admission_ms",
              "reserve_ms",
              "update_ms",
            ]
          : ["duration_ms"];
    for (const field of fields)
      if (Number.isFinite(event[field]) && event[field] >= 0)
        entry[field] = event[field];
    if (
      typeof event.timestamp === "string" &&
      /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/.test(event.timestamp)
    ) {
      const timestamp = Date.parse(event.timestamp);
      if (Number.isFinite(timestamp)) {
        firstTimestamp ??= timestamp;
        if (timestamp >= firstTimestamp)
          entry.elapsed_from_first_ms = timestamp - firstTimestamp;
      }
    }
    stages.push(entry);
  }
  // Missing, clipped or late rows remain unobserved; this is not a stream cursor.
  return { capture: "available_rows_only", stages: stages.slice(-8) };
}

function receipt(id, connection, adapter) {
  return {
    request_id: id,
    connection,
    adapter,
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

async function seed(config, modes, kind, { Store, connectionIdentity }) {
  const store = new Store(config.stateDir);
  try {
    for (const mode of modes) {
      const connection = config.connections[mode];
      const saved = store.prepare(
        receipt(mode, mode, connection.adapter),
        mode,
        mode === "identity"
          ? "other-account"
          : await connectionIdentity(mode, connection),
      ).receipt;
      assert.equal(store.admit(mode, mode, 32, 1), true);
      saved.status = "running";
      saved.generation_outcome = "running";
      saved.upstream_job = { id: mode, kind };
      store.update(saved);
      store.release(mode, saved);
    }
  } finally {
    store.close();
  }
}

function configuration(root, connections, configSchema) {
  const config = configSchema.parse({
    schemaVersion: 1,
    defaultConnection: Object.keys(connections)[0],
    outputDir: join(root, "outputs"),
    stateDir: join(root, "state"),
    logging: { directory: join(root, "logs") },
    files: { allowedInputRoots: [join(root, "inputs")] },
    connections,
  });
  config.maxConcurrentRequests = 32;
  config.files.maxAggregateBytes = 4096;
  config.files.maxJournalBytes = 1024 * 1024;
  return config;
}

async function terminal(adapter, context, original) {
  const root = join(context.workspace, `terminal-${adapter}`);
  await mkdir(root);
  const states = [
    "failed",
    "cancelled",
    "incomplete",
    "completed",
    ...(adapter === "gemini-interactions" ? ["budget_exceeded"] : []),
  ];
  const controls = [
    "queued",
    "in_progress",
    "401",
    "429",
    "500",
    "disconnect",
    "malformed",
    "wrong-id",
    "identity",
  ];
  const modes = [...states, ...controls];
  const posts = [];
  const image =
    adapter === "openai-responses"
      ? {
          output: [
            {
              type: "image_generation_call",
              status: "completed",
              result: original.base64,
            },
          ],
        }
      : {
          steps: [
            {
              type: "model_output",
              content: [{ type: "image", data: original.base64 }],
            },
          ],
        };
  await provider(
    (request, response) => {
      const mode = request.url.split("/")[1];
      if (request.method === "POST") {
        posts.push(mode);
        json(response, { id: "probe", status: "completed", ...image });
      } else if (mode === "disconnect") response.destroy();
      else if (mode === "malformed") response.end("not-json");
      else
        json(
          response,
          {
            id: mode === "wrong-id" ? "other" : mode,
            status: states.includes(mode)
              ? mode
              : mode === "queued"
                ? "queued"
                : "in_progress",
            ...(["completed", "incomplete"].includes(mode) ? image : {}),
          },
          ["401", "429", "500"].includes(mode) ? Number(mode) : 200,
        );
    },
    async (origin) => {
      const config = configuration(
        root,
        Object.fromEntries(
          modes.map((mode) => [
            mode,
            {
              adapter,
              baseUrl: `${origin}/${mode}/v1`,
              auth: { type: "none" },
              orchestrationModel: "text",
              defaultModel: "image",
              requestTimeoutMs: 700,
              maxConcurrentRequests: 1,
            },
          ]),
        ),
        context.configSchema,
      );
      const kind =
        adapter === "openai-responses" ? "responses" : "interactions";
      await seed(config, modes, kind, context);
      const outputIds = new Map();
      const instances = new Set();
      for (let round = 0; round < 3; round++) {
        const hosts = await pair(root, config, context, instances);
        try {
          for (const mode of modes) {
            await Promise.all(
              hosts.map((host) =>
                call(
                  host,
                  "get_job",
                  {
                    request_id: mode,
                    refresh: true,
                  },
                  context.bounded,
                ),
              ),
            );
            for (const host of hosts) {
              const current = await job(host, mode, false, context.bounded);
              if (states.includes(mode)) {
                const state = mode === "budget_exceeded" ? "failed" : mode;
                assert.equal(current.generation_outcome, state);
                assert.equal(
                  current.status,
                  mode === "completed"
                    ? "completed"
                    : mode === "incomplete"
                      ? "partial"
                      : mode === "cancelled"
                        ? "cancelled"
                        : "failed",
                );
                if (["completed", "incomplete"].includes(mode)) {
                  assert.equal(current.outputs.length, 1);
                  assert.deepEqual(
                    await readFile(current.outputs[0].path),
                    Buffer.from(original.base64, "base64"),
                  );
                  if (outputIds.has(mode))
                    assert.equal(
                      current.outputs[0].output_id,
                      outputIds.get(mode),
                    );
                  else outputIds.set(mode, current.outputs[0].output_id);
                }
              } else {
                assert.equal(current.status, "running");
                assert.equal(current.upstream_terminal, undefined);
              }
              assert.deepEqual(current.upstream_job, { id: mode, kind });
            }
            const before = posts.length;
            const beforeStderr = hosts[1].stderr();
            const probeStarted = performance.now();
            const probe = await call(
              hosts[1],
              "generate_image",
              {
                connection: mode,
                prompt: "admission probe",
                request_id: `probe-${mode}-${round}`,
              },
              context.bounded,
            );
            const admission = JSON.stringify({
              adapter,
              mode,
              round,
              is_error: probe.isError,
              status: probe.structuredContent?.status,
              error: probe.structuredContent?.error?.code,
              stage: probe.structuredContent?.error?.stage,
              receipt_errors: probe.structuredContent?.errors?.map(
                ({ code, stage }) => ({ code, stage }),
              ),
              elapsed_ms: Math.round(performance.now() - probeStarted),
              observed_posts: posts.length - before,
              native: probeStages(beforeStderr, hosts[1].stderr()),
            });
            assert.equal(
              posts.length - before,
              states.includes(mode) ? 1 : 0,
              admission,
            );
            if (states.includes(mode))
              assert.notEqual(probe.isError, true, admission);
            const replay = await call(
              hosts[0],
              "generate_image",
              {
                connection: mode,
                prompt: "different fingerprint",
                request_id: mode,
              },
              context.bounded,
            );
            assert.equal(replay.isError, true);
            assert.ok(JSON.stringify(replay).includes("request_id_conflict"));
            assert.equal(posts.length - before, states.includes(mode) ? 1 : 0);
          }
        } finally {
          await close(hosts, context.closeHost);
        }
        process.stderr.write(
          JSON.stringify({
            component: "native-terminal-acceptance",
            adapter,
            round,
            message: "Terminal and polling control round completed.",
            cases: modes.length,
          }) + "\n",
        );
      }
      // Startup run IDs identify six actual process instances even if an OS reuses a PID.
      assert.equal(instances.size, 6);
      assert.equal(posts.length, states.length * 3);
    },
    context.bounded,
  );
}

async function comfy(context, original) {
  const root = join(context.workspace, "terminal-comfy-cancel");
  await mkdir(root);
  const modes = [
    "cancelled",
    "failed",
    "dispatched",
    "false",
    "missing",
    "ambiguous",
    "wrong-id",
    "completed",
  ];
  let cancellations = 0;
  let generations = 0;
  await provider(
    (request, response) => {
      const url = new URL(request.url, "http://fixture");
      const mode = url.searchParams.get("fixture");
      if (request.method === "POST") {
        if (url.pathname.endsWith("/cancel")) {
          cancellations++;
          if (mode === "ambiguous") response.destroy();
          else
            json(
              response,
              mode === "missing" ? {} : { cancelled: mode !== "false" },
            );
        } else {
          generations++;
          json(response, { prompt_id: "probe" });
        }
      } else if (request.url.includes("/object_info"))
        json(response, {
          Text: { input: { required: { text: ["STRING"] } } },
          Save: {
            output_node: true,
            input: { required: { text: ["STRING"] } },
          },
        });
      else if (request.url.includes("/system_stats"))
        json(response, { devices: [{ type: "cuda" }] });
      else if (request.url.includes("/history/probe"))
        json(response, {
          probe: {
            status: { completed: true, status_str: "success" },
            outputs: {
              save: {
                images: [
                  { filename: "probe.png", subfolder: "", type: "output" },
                ],
              },
            },
          },
        });
      else if (request.url.includes("/view")) {
        response.writeHead(200, { "Content-Type": "image/png" });
        response.end(Buffer.from(original.base64, "base64"));
      } else if (request.url.includes("/history/")) json(response, {});
      else
        json(response, {
          id: mode === "wrong-id" ? "other" : mode,
          status: ["cancelled", "failed"].includes(mode) ? mode : "in_progress",
        });
    },
    async (origin) => {
      const config = configuration(
        root,
        Object.fromEntries(
          modes.map((mode) => [
            mode,
            {
              adapter: "comfyui",
              baseUrl: origin,
              baseUrlMode: "origin",
              auth: { type: "none" },
              defaultModel: "workflow",
              requestTimeoutMs: 700,
              workflow: {
                graph: {
                  text: { class_type: "Text", inputs: { text: "" } },
                  save: { class_type: "Save", inputs: { text: "output" } },
                },
                bindings: { prompt: { node: "text", input: "text" } },
                outputNodes: ["save"],
              },
            },
          ]),
        ),
        context.configSchema,
      );
      for (const mode of modes)
        config.connections[mode].query = { fixture: mode };
      await seed(config, modes, "comfyui", context);
      const store = new context.Store(config.stateDir);
      try {
        assert.equal(store.claimFinalization("completed"), true);
        const done = store.get("completed");
        done.status = "completed";
        done.generation_outcome = "completed";
        store.update(done);
        store.release("completed", done);
      } finally {
        store.close();
      }
      const instances = new Set();
      for (let round = 0; round < 3; round++) {
        const hosts = await pair(root, config, context, instances);
        try {
          for (const mode of modes) {
            const result = await call(
              hosts[0],
              "cancel_job",
              { request_id: mode },
              context.bounded,
            );
            if (!["ambiguous", "wrong-id"].includes(mode)) {
              assert.notEqual(result.isError, true);
              assert.equal(result.structuredContent.refund_verified, false);
              assert.equal(
                result.structuredContent.upstream_cancelled,
                mode === "cancelled"
                  ? true
                  : ["missing", "completed"].includes(mode) ||
                      (mode === "failed" && round > 0)
                    ? null
                    : false,
              );
            } else assert.equal(result.isError, true);
            for (const host of hosts) {
              const current = await job(host, mode, true, context.bounded);
              assert.equal(
                current.status,
                ["cancelled", "failed", "completed"].includes(mode)
                  ? mode
                  : "running",
              );
            }
            const before = generations;
            const probe = await call(
              hosts[1],
              "generate_image",
              {
                connection: mode,
                prompt: "capacity",
                request_id: `capacity-${mode}-${round}`,
              },
              context.bounded,
            );
            if (["cancelled", "failed", "completed"].includes(mode))
              assert.notEqual(probe.isError, true, JSON.stringify(probe));
            assert.equal(
              generations - before,
              ["cancelled", "failed", "completed"].includes(mode) ? 1 : 0,
            );
          }
        } finally {
          await close(hosts, context.closeHost);
        }
        process.stderr.write(
          JSON.stringify({
            component: "native-terminal-acceptance",
            adapter: "comfyui",
            round,
            message: "Cancellation acknowledgement round completed.",
            cases: modes.length,
          }) + "\n",
        );
      }
      assert.equal(instances.size, 6);
      assert.equal(generations, 9);
      assert.equal(cancellations, 2 + 5 * 3);
    },
    context.bounded,
  );
}

// Exact three frozen stdio-terminal-lifecycle cases; actual native children, not source dispatch.
export async function terminalAcceptance(context) {
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
  for (const adapter of ["openai-responses", "gemini-interactions"])
    await ownedScenario(context, (scoped) =>
      terminal(adapter, scoped, original),
    );
  await ownedScenario(context, (scoped) => comfy(scoped, original));
}
