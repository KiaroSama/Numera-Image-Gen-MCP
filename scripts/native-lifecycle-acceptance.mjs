import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { provider, json, call, ownedScenario } from "./native-host-fixture.mjs";

// Frozen terminal-lifecycle literals, with native stdio instead of InMemoryTransport.
// The seed Store is separate from the child's Store; this is not same-allocation proof.
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

function configuration(context, root, origin, adapter) {
  const config = context.configSchema.parse({
    schemaVersion: 1,
    defaultConnection: "local",
    outputDir: join(root, "outputs"),
    stateDir: join(root, "state"),
    logging: { directory: join(root, "logs") },
    files: { allowedInputRoots: [join(root, "inputs")] },
    connections: {
      local: {
        adapter,
        baseUrl: origin,
        auth: { type: "none" },
        ...(adapter === "comfyui"
          ? { baseUrlMode: "origin" }
          : { defaultModel: "image-model" }),
      },
    },
  });
  assert.equal(config.connections.local.requestTimeoutMs, 300000);
  assert.equal(config.connections.local.discoveryTimeoutMs, 15000);
  return config;
}

function seed(store, identity, kind, completed = false) {
  const old = store.prepare(receipt("old"), "hash", identity).receipt;
  assert.equal(store.admit("old", "local", 1, 1), true);
  old.status = completed ? "completed" : "running";
  old.generation_outcome = completed ? "completed" : "running";
  old.upstream_job = { id: "job", kind };
  store.update(old);
  store.release("old", old);
}

function admission(store, id, identity, expected) {
  store.prepare(receipt(id), id, identity);
  assert.equal(store.admit(id, "local", 1, 1), expected);
  if (expected) {
    const done = store.get(id);
    done.status = "failed";
    store.update(done);
    store.release(id, done);
  }
}

async function start(context, config, root, name) {
  const file = join(root, "config.json");
  await writeFile(file, JSON.stringify(config), "utf8");
  context.signal.throwIfAborted();
  const host = await context.start(
    file,
    "auto",
    { signal: context.signal },
    {
      clientInfo: { name, version: "1" },
    },
  );
  host.fixtureSignal = context.signal;
  try {
    context.signal.throwIfAborted();
    assert.equal(host.client.getProtocolEra(), "legacy");
    return host;
  } catch (error) {
    await context.closeHost(host);
    throw error;
  }
}

async function authoritative(context, adapter, kind, status) {
  const root = join(context.workspace, `literal-terminal-${adapter}-${status}`);
  await mkdir(root);
  let gets = 0;
  let posts = 0;
  await provider(
    (request, response) => {
      if (request.method === "POST") posts++;
      else gets++;
      assert.equal(request.method, "GET");
      assert.equal(request.url, `/${kind}/job`);
      assert.equal(request.headers.authorization, undefined);
      json(response, { id: "job", status });
    },
    async (origin) => {
      const config = configuration(context, root, origin, adapter);
      const identity = await context.connectionIdentity(
        "local",
        config.connections.local,
      );
      let store = new context.Store(config.stateDir);
      try {
        seed(store, identity, kind);
        for (let reopen = 0; reopen < 3; reopen++) {
          let host;
          try {
            host = await start(context, config, root, "terminal-test");
            for (let refresh = 0; refresh < 2; refresh++) {
              const result = await call(
                host,
                "get_job",
                { request_id: "old", refresh: true },
                context.bounded,
              );
              assert.notEqual(result.isError, true, JSON.stringify(result));
              const value = result.structuredContent;
              assert.ok(value);
              assert.equal(
                value.status,
                status === "incomplete" ? "failed" : status,
              );
              assert.equal(value.generation_outcome, status);
              assert.deepEqual(value.upstream_job, { id: "job", kind });
            }
            admission(store, `fresh-${reopen}`, identity, true);
            assert.equal(
              store.prepare(receipt("old"), "hash", identity).fresh,
              false,
            );
          } finally {
            if (host) await context.closeHost(host);
          }
          store.close();
          store = null;
          if (reopen < 2) store = new context.Store(config.stateDir);
        }
        assert.equal(gets, 1);
        assert.equal(posts, 0);
      } finally {
        store?.close();
      }
    },
    context.bounded,
  );
}

async function uncertainty(context, mode) {
  const root = join(context.workspace, `literal-uncertainty-${mode}`);
  await mkdir(root);
  let gets = 0;
  let posts = 0;
  await provider(
    (request, response) => {
      if (request.method === "POST") posts++;
      else gets++;
      assert.equal(request.method, "GET");
      assert.equal(request.url, "/responses/job");
      assert.equal(request.headers.authorization, undefined);
      if (mode === "disconnect") response.destroy();
      else if (mode === "malformed") response.end("not-json");
      else
        json(
          response,
          mode === "wrong-id"
            ? { id: "other", status: "failed" }
            : { id: "job", status: "in_progress" },
          ["401", "429", "500"].includes(mode) ? Number(mode) : 200,
        );
    },
    async (origin) => {
      const config = configuration(context, root, origin, "openai-responses");
      const identity = await context.connectionIdentity(
        "local",
        config.connections.local,
      );
      const store = new context.Store(config.stateDir);
      let host;
      try {
        seed(store, mode === "identity" ? "other" : identity, "responses");
        host = await start(context, config, root, "poll-control");
        await call(
          host,
          "get_job",
          { request_id: "old", refresh: true },
          context.bounded,
        );
        assert.equal(store.get("old").upstream_terminal, undefined);
        assert.equal(store.get("old").status, "running");
        admission(store, "fresh", identity, false);
        assert.equal(posts, 0);
        assert.equal(gets, mode === "identity" ? 0 : 1);
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

async function cancellation(context, mode) {
  const root = join(context.workspace, `literal-cancellation-${mode}`);
  await mkdir(root);
  let cancellations = 0;
  let generations = 0;
  await provider(
    (request, response) => {
      assert.equal(request.headers.authorization, undefined);
      if (request.method === "POST") {
        if (request.url?.endsWith("/cancel")) {
          cancellations++;
          assert.equal(request.url, "/api/jobs/job/cancel");
          if (mode === "ambiguous") response.destroy();
          else
            json(
              response,
              mode === "missing" ? {} : { cancelled: mode !== "false" },
            );
        } else {
          generations++;
          json(response, {});
        }
      } else {
        assert.equal(request.method, "GET");
        assert.equal(request.url, "/api/jobs/job");
        json(response, {
          id: "job",
          status: mode === "confirmed" ? "cancelled" : "in_progress",
        });
      }
    },
    async (origin) => {
      const config = configuration(context, root, origin, "comfyui");
      const identity = await context.connectionIdentity(
        "local",
        config.connections.local,
      );
      let store = new context.Store(config.stateDir);
      let host;
      try {
        seed(store, identity, "comfyui", mode === "completed");
        host = await start(context, config, root, "cancel-contract");
        for (let n = 0; n < 2; n++) {
          const result = await call(
            host,
            "cancel_job",
            { request_id: "old" },
            context.bounded,
          );
          if (mode !== "ambiguous") {
            assert.ok(result.structuredContent);
            assert.equal(result.structuredContent.refund_verified, false);
            assert.equal(
              result.structuredContent.upstream_cancelled,
              mode === "confirmed"
                ? true
                : ["missing", "completed"].includes(mode)
                  ? null
                  : false,
            );
          }
        }
      } finally {
        try {
          if (host) await context.closeHost(host);
        } finally {
          store.close();
          store = null;
        }
      }
      for (let n = 0; n < 2; n++) {
        store = new context.Store(config.stateDir);
        try {
          assert.equal(
            store.get("old").status,
            mode === "confirmed"
              ? "cancelled"
              : mode === "completed"
                ? "completed"
                : "running",
          );
          admission(
            store,
            `fresh-${n}`,
            identity,
            ["confirmed", "completed"].includes(mode),
          );
        } finally {
          store.close();
          store = null;
        }
      }
      assert.equal(generations, 0);
      assert.equal(
        cancellations,
        mode === "completed" ? 0 : mode === "confirmed" ? 1 : 2,
      );
    },
    context.bounded,
  );
}

export async function lifecycleAcceptance(context) {
  for (const [adapter, kind, status] of [
    ["openai-responses", "responses", "failed"],
    ["openai-responses", "responses", "cancelled"],
    ["gemini-interactions", "interactions", "failed"],
    ["openai-responses", "responses", "incomplete"],
  ]) {
    await ownedScenario(
      context,
      (scoped) => authoritative(scoped, adapter, kind, status),
      10000,
    );
    process.stderr.write(
      JSON.stringify({
        component: "native-lifecycle-acceptance",
        adapter,
        kind,
        status,
        completed: true,
      }) + "\n",
    );
  }
  for (const mode of [
    "in_progress",
    "401",
    "429",
    "500",
    "disconnect",
    "malformed",
    "wrong-id",
    "identity",
  ]) {
    await ownedScenario(context, (scoped) => uncertainty(scoped, mode), 10000);
    process.stderr.write(
      JSON.stringify({
        component: "native-lifecycle-acceptance",
        uncertainty: mode,
        completed: true,
      }) + "\n",
    );
  }
  for (const mode of [
    "confirmed",
    "dispatched",
    "false",
    "missing",
    "ambiguous",
    "completed",
  ]) {
    await ownedScenario(context, (scoped) => cancellation(scoped, mode), 10000);
    process.stderr.write(
      JSON.stringify({
        component: "native-lifecycle-acceptance",
        cancellation: mode,
        completed: true,
      }) + "\n",
    );
  }
}
