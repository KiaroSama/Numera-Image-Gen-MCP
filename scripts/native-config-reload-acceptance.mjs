import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdir, readFile, readdir, stat, writeFile } from "node:fs/promises";
import { join } from "node:path";

// Frozen integration/config-reload.test.ts, actual native SDK sessions only.
// The second case needs public Store.release before crossing from the seed Store to a child.
// That explicit fixture ownership handoff is not the source's same-Store seeding schedule.

async function fixture(bounded, respond, action) {
  const sockets = new Set();
  const inFlight = new Set();
  const requests = [];
  let failure;
  const wire = createServer((request, response) => {
    const chunks = [];
    let length = 0;
    request.on("data", (chunk) => {
      length += chunk.length;
      if (length > 1048576) {
        failure ??= new Error("Reload fixture body exceeds one MiB.");
        request.destroy();
      } else chunks.push(chunk);
    });
    request.on("error", (error) => {
      failure ??= error;
      response.destroy();
    });
    request.on("end", () => {
      const task = Promise.resolve()
        .then(async () => {
          assert.ok(length <= 1048576);
          const captured = {
            method: request.method,
            url: request.url,
            body: Buffer.concat(chunks, length),
          };
          requests.push(captured);
          const value = await respond(captured);
          if (!response.destroyed) {
            response.writeHead(200, { "Content-Type": "application/json" });
            response.end(JSON.stringify(value));
          }
        })
        .catch((error) => {
          failure ??= error;
          if (!response.destroyed) {
            if (!response.headersSent) response.writeHead(500);
            response.end("{}");
          }
        });
      inFlight.add(task);
      void task.then(
        () => inFlight.delete(task),
        (error) => {
          failure ??= error;
          inFlight.delete(task);
        },
      );
    });
  });
  wire.requestTimeout = 5000;
  wire.headersTimeout = 5000;
  wire.on("connection", (socket) => {
    sockets.add(socket);
    socket.once("close", () => sockets.delete(socket));
  });
  try {
    await bounded(
      new Promise((accept, reject) => {
        wire.once("error", reject);
        wire.listen(0, "127.0.0.1", accept);
      }),
    );
    try {
      await action(`http://127.0.0.1:${wire.address().port}`, requests);
    } catch (error) {
      throw failure ?? error;
    }
    if (failure) throw failure;
  } finally {
    for (const socket of sockets) socket.destroy();
    try {
      if (wire.listening)
        await bounded(
          new Promise((accept, reject) => {
            wire.close((error) => (error ? reject(error) : accept()));
          }),
        );
    } finally {
      await bounded(Promise.allSettled([...inFlight]));
    }
  }
}

function configuration(configSchema, directory, profile) {
  return configSchema.parse({
    schemaVersion: 1,
    defaultConnection: "local",
    outputDir: join(directory, "outputs"),
    stateDir: join(directory, "state"),
    logging: { directory: join(directory, "logs") },
    files: { allowedInputRoots: [join(directory, "inputs")] },
    connections: { local: profile },
  });
}

function call(host, bounded, name, args = {}, timeout = 5000) {
  return bounded(
    host.client.callTool({ name, arguments: args }, { timeout }),
    timeout + 1000,
  );
}

async function logs(directory) {
  const parts = [];
  for (const name of await readdir(directory)) {
    const path = join(directory, name);
    assert.ok((await stat(path)).size <= 1048576, "bounded reload fixture log");
    parts.push(await readFile(path, "utf8"));
  }
  return parts.join("\n");
}

// d974fb6cbf33401975d7f4802ff8b6977f0a5d5e78a06a3f696fd66cd711030d.
async function envModels({ workspace, start, closeHost, bounded }) {
  const directory = join(workspace, "config-reload-env");
  await mkdir(directory);
  await fixture(
    bounded,
    () => ({ data: [] }),
    async (origin, requests) => {
      const file = join(directory, ".env");
      const envText = (id, name) =>
        `API_ENDPOINT=${origin}/v1\nAPI_KEY=synthetic-test-key\nMODEL_1_ID=${id}\nMODEL_1_NAME="${name}"\nMODEL_1_SIZE=\n`;
      await writeFile(file, envText("first", "First"), "utf8");
      let host;
      try {
        host = await start(
          file,
          "auto",
          {},
          {
            clientInfo: { name: "reload-fixture", version: "1" },
          },
        );
        const models = async () =>
          (await call(host, bounded, "list_models")).structuredContent;
        const pairs = (result) =>
          result.models.map(({ id, name }) => ({ id, name }));
        assert.deepEqual(pairs(await models()), [
          { id: "first", name: "First" },
        ]);
        await writeFile(file, envText("second", "تصویر دوم"), "utf8");
        assert.deepEqual(pairs(await models()), [
          { id: "second", name: "تصویر دوم" },
        ]);
        await writeFile(
          file,
          envText("third", "Third") + "MODEL_1_ENABLED=maybe\n",
          "utf8",
        );
        assert.deepEqual(pairs(await models()), [
          { id: "second", name: "تصویر دوم" },
        ]);
        assert.deepEqual(pairs(await models()), [
          { id: "second", name: "تصویر دوم" },
        ]);
        assert.equal(requests.length, 0);
        const text = await logs(join(directory, "logs"));
        assert.equal(
          (text.match(/Saved configuration rejected/g) ?? []).length,
          1,
        );
        assert.equal(text.includes("synthetic-test-key"), false);
      } finally {
        if (host) await closeHost(host);
      }
    },
  );
}

function oldReceipt() {
  return {
    request_id: "existing",
    connection: "local",
    adapter: "comfyui",
    operation: "generate",
    status: "running",
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
    upstream_job: { kind: "comfyui", id: "original-handle" },
  };
}

// 0b69e44fffd8f6dcb352a8a06c921870705893e537533e745ad8109370d3acf2.
async function retainedHandles({
  workspace,
  configSchema,
  Store,
  connectionIdentity,
  start,
  closeHost,
  bounded,
}) {
  const directory = join(workspace, "config-reload-handles");
  await mkdir(directory);
  await fixture(
    bounded,
    (request) =>
      request.method === "POST"
        ? { cancelled: true }
        : request.url.startsWith("/api/jobs/")
          ? { id: "original-handle", status: "cancelled" }
          : {},
    async (origin, requests) => {
      const file = join(directory, "config.json");
      const first = configuration(configSchema, directory, {
        adapter: "comfyui",
        baseUrl: origin,
        baseUrlMode: "origin",
        auth: { type: "none" },
      });
      await writeFile(file, JSON.stringify(first), "utf8");
      const seeded = new Store(first.stateDir);
      try {
        const old = oldReceipt();
        const prepared = seeded.prepare(
          old,
          "fixture-fingerprint",
          await connectionIdentity("local", first.connections.local),
        ).receipt;
        const unknown = seeded.prepare(
          { ...old, request_id: "unknown" },
          "fixture-fingerprint",
          "unavailable-account",
        ).receipt;
        // Cross-process fixture handoff only: closing a Store does not release its live writer lease.
        seeded.release("existing", prepared);
        seeded.release("unknown", unknown);
      } finally {
        seeded.close();
      }
      let host;
      try {
        host = await start(
          file,
          "auto",
          {},
          {
            clientInfo: { name: "recovery-fixture", version: "1" },
            // The source injects an ERROR Logger directly; pin the same level at the native CLI.
            args: ["bridge", "--config", file, "--log-level", "ERROR"],
          },
        );
        await writeFile(
          file,
          JSON.stringify({
            ...first,
            connections: {
              local: {
                ...first.connections.local,
                baseUrl: origin + "/changed",
                defaultModel: "new-model",
              },
            },
          }),
          "utf8",
        );
        const poll = await call(host, bounded, "get_job", {
          request_id: "existing",
          refresh: true,
        });
        assert.notEqual(poll.isError, true);
        const cancel = await call(host, bounded, "cancel_job", {
          request_id: "existing",
        });
        assert.equal(cancel.structuredContent.upstream_requested, true);
        assert.equal(cancel.structuredContent.upstream_cancelled, true);
        assert.deepEqual(
          requests.map((request) => request.url),
          [
            "/history/original-handle",
            "/api/jobs/original-handle/cancel",
            "/api/jobs/original-handle",
          ],
        );
        const unknown = await call(host, bounded, "get_job", {
          request_id: "unknown",
          refresh: true,
        });
        assert.equal(unknown.isError, true);
        assert.equal(unknown.structuredContent.error.code, "permission_denied");
        assert.equal(requests.length, 3);
        await writeFile(
          file,
          JSON.stringify({
            ...first,
            connections: {
              local: {
                ...first.connections.local,
                paths: { history: "changed-history" },
              },
            },
          }),
          "utf8",
        );
        const ambiguous = await call(host, bounded, "get_job", {
          request_id: "existing",
          refresh: true,
        });
        assert.equal(ambiguous.isError, true);
        assert.equal(
          ambiguous.structuredContent.error.code,
          "permission_denied",
        );
        assert.equal(requests.length, 3);
      } finally {
        if (host) await closeHost(host);
      }
    },
  );
}

// 3f5b00dd26de8c64952c76e4aa20fac2a7ba3cca5542f7930576d0adfebacd2c.
async function capturedGeneration(
  { workspace, configSchema, start, closeHost, bounded },
  png,
) {
  const directory = join(workspace, "config-reload-captured");
  await mkdir(directory);
  let submitted;
  let release;
  const intent = new Promise((accept) => {
    submitted = accept;
  });
  const held = new Promise((accept) => {
    release = accept;
  });
  const bodies = [];
  await fixture(
    bounded,
    async (request) => {
      bodies.push(JSON.parse(request.body.toString("utf8")));
      submitted();
      await held;
      return { data: [{ b64_json: png.toString("base64") }] };
    },
    async (origin, requests) => {
      const file = join(directory, "config.json");
      const first = configuration(configSchema, directory, {
        adapter: "openai-images",
        baseUrl: origin + "/v1",
        auth: { type: "none" },
        defaultModel: "old",
        configuredModels: [{ id: "old", name: "Old" }],
        requestTimeoutMs: 5000,
      });
      await writeFile(file, JSON.stringify(first), "utf8");
      let host;
      let pending;
      try {
        host = await start(
          file,
          "auto",
          {},
          {
            clientInfo: { name: "snapshot-fixture", version: "1" },
            args: ["bridge", "--config", file, "--log-level", "ERROR"],
          },
        );
        pending = call(
          host,
          bounded,
          "generate_image",
          { prompt: "fixture", request_id: "captured-config" },
          7000,
        );
        // Observe rejection immediately, including before intent or any later assertion settles.
        const settled = pending.then(
          (value) => ({ value }),
          (error) => ({ error }),
        );
        await bounded(
          Promise.race([
            intent,
            settled.then((outcome) => {
              throw (
                outcome.error ??
                new Error("Generation returned before fixture submission.")
              );
            }),
          ]),
          8000,
        );
        await writeFile(
          file,
          JSON.stringify({
            ...first,
            connections: {
              local: {
                ...first.connections.local,
                baseUrl: origin + "/new/v1",
                defaultModel: "new",
                configuredModels: [{ id: "new", name: "New" }],
              },
            },
          }),
          "utf8",
        );
        const listed = await call(host, bounded, "list_models");
        assert.deepEqual(
          listed.structuredContent.models.map(({ id, name }) => ({ id, name })),
          [{ id: "new", name: "New" }],
        );
        release();
        const result = await pending;
        assert.equal(result.structuredContent.status, "completed");
        assert.equal(result.structuredContent.requested_model, "old");
        assert.equal(result.structuredContent.outputs[0].bytes, png.length);
        assert.deepEqual(bodies, [{ model: "old", prompt: "fixture", n: 1 }]);
        assert.deepEqual(
          requests.map((request) => [request.method, request.url]),
          [["POST", "/v1/images/generations"]],
        );
        assert.deepEqual(
          await readFile(result.structuredContent.outputs[0].path),
          png,
        );
      } finally {
        release();
        try {
          if (pending) await pending.catch(() => {});
        } finally {
          if (host) await closeHost(host);
        }
      }
    },
  );
}

export async function configReloadAcceptance({
  root,
  workspace,
  configSchema,
  Store,
  connectionIdentity,
  start,
  closeHost,
  bounded,
}) {
  const context = {
    root,
    workspace,
    configSchema,
    Store,
    connectionIdentity,
    start,
    closeHost,
    bounded,
  };
  assert.equal(
    process.env.GITHUB_ACTIONS,
    "true",
    "native config reload acceptance is hosted only",
  );
  assert.equal(context.start.supportsLaunchOverrides, true);
  const path = join(root, ".ci-work/parity/image-acceptance.json");
  assert.ok((await stat(path)).size <= 1048576);
  const rows = JSON.parse(await readFile(path, "utf8"));
  const original = rows.find((row) => row.name === "png");
  assert.ok(original, "exact source PNG required");
  assert.equal(original.metadata.width, 8);
  assert.equal(original.metadata.height, 6);
  assert.equal(original.metadata.mime, "image/png");
  assert.equal(original.metadata.alpha, true);
  const png = Buffer.from(original.base64, "base64");
  assert.ok(png.length > 0);
  // These are independent fresh sessions, not three updates to one shared fixture configuration.
  await envModels(context);
  await retainedHandles(context);
  await capturedGeneration(context, png);
  process.stderr.write(
    JSON.stringify({
      component: "native-config-reload-acceptance",
      message: "Three source config reload SDK scenarios passed.",
      cases: 3,
    }) + "\n",
  );
}
