import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdir, writeFile, readFile, rm } from "node:fs/promises";
import { resolve, join } from "node:path";
import { pathToFileURL } from "node:url";
import { randomUUID } from "node:crypto";
import { createRequire } from "node:module";
import { toolsAcceptance } from "./native-tools-acceptance.mjs";
import { twoHostAcceptance } from "./native-two-host-acceptance.mjs";
import { compactAcceptance } from "./native-compact-acceptance.mjs";
import { envAcceptance } from "./native-env-acceptance.mjs";
import { negotiationAcceptance } from "./native-negotiation-acceptance.mjs";
import { configReloadAcceptance } from "./native-config-reload-acceptance.mjs";
import { terminalAcceptance } from "./native-terminal-acceptance.mjs";
import { racesAcceptance } from "./native-races-acceptance.mjs";
import { lifecycleAcceptance } from "./native-lifecycle-acceptance.mjs";
import { transportAcceptance } from "./native-transport-acceptance.mjs";

// Hosted differential acceptance: the actual native binary, two SDK hosts, two restarts.
if (process.env.GITHUB_ACTIONS !== "true")
  throw new Error("Native interoperability is hosted CI only.");
const root = resolve(process.env.GITHUB_WORKSPACE ?? process.cwd());
const baseline = resolve(process.env.NUMERA_ORACLE_ROOT);
const requireBaseline = createRequire(join(baseline, "package.json"));
const { Client } = await import(
  pathToFileURL(requireBaseline.resolve("@modelcontextprotocol/client")).href
);
const { StdioClientTransport } = await import(
  pathToFileURL(requireBaseline.resolve("@modelcontextprotocol/client/stdio"))
    .href
);
const source = (name) =>
  import(pathToFileURL(join(baseline, "dist", name)).href);
const { configSchema } = await source("config/schema.js");
const { Store } = await source("jobs/store.js");
const { connectionIdentity } = await source("services/connection-identity.js");
const binary =
  process.env.NUMERA_NATIVE_BINARY ??
  join(
    root,
    "target",
    "release",
    process.platform === "win32"
      ? "numera-image-gen-mcp.exe"
      : "numera-image-gen-mcp",
  );
const workspace = join(root, ".ci-work", "native-interop", randomUUID());
const imageCases = JSON.parse(
  await readFile(join(root, ".ci-work/parity/images.json"), "utf8"),
);
const bytes = Buffer.from(imageCases[0].base64, "base64");
const states = [
  "failed",
  "cancelled",
  "incomplete",
  "completed",
  "budget_exceeded",
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
];
const posts = [];
let sockets = new Set();
let hosts = [];
const wire = createServer((request, response) => {
  const path = new URL(request.url, "http://localhost").pathname;
  const segments = path.split("/").filter(Boolean);
  const adapter = segments[0],
    mode = segments[1];
  if (request.method === "POST") {
    posts.push(path);
    response
      .writeHead(500, { "Content-Type": "application/json" })
      .end('{"error":"unexpected-submission"}');
    return;
  }
  if (mode === "disconnect") {
    response.destroy();
    return;
  }
  if (mode === "malformed") {
    response
      .writeHead(200, { "Content-Type": "application/json" })
      .end("not-json");
    return;
  }
  if (["401", "429", "500"].includes(mode)) {
    response
      .writeHead(Number(mode), { "Content-Type": "application/json" })
      .end("{}");
    return;
  }
  const image =
    adapter === "responses"
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
              content: [{ type: "image", data: bytes.toString("base64") }],
            },
          ],
        };
  const body = {
    id: mode === "wrong-id" ? "another-job" : "original-" + mode,
    status: mode === "wrong-id" ? "completed" : mode,
    ...(["incomplete", "completed"].includes(mode) || mode === "wrong-id"
      ? image
      : {}),
  };
  response
    .writeHead(200, { "Content-Type": "application/json" })
    .end(JSON.stringify(body));
});
wire.on("connection", (socket) => {
  sockets.add(socket);
  socket.once("close", () => sockets.delete(socket));
});
const closeHost = async (host) => {
  if (host.reaped) return;
  await bounded(host.client.close(), 5000);
  if (host.started) await bounded(host.closed, 2000);
  if (host.pid !== null)
    assert.throws(
      () => process.kill(host.pid, 0),
      "native child must be gone, not just absent from SDK",
    );
  host.reaped = true;
};
async function bounded(promise, ms = 5000) {
  let timer;
  try {
    return await Promise.race([
      promise,
      new Promise((_, reject) => {
        timer = setTimeout(
          () => reject(new Error("Native fixture event deadline exceeded.")),
          ms,
        );
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}
async function start(
  configFile,
  mode = "auto",
  connectOptions = {},
  overrides = {},
) {
  const launch = {
    command: overrides.command ?? binary,
    args: overrides.args ?? ["bridge", "--config", configFile],
    cwd: overrides.cwd ?? workspace,
    clientInfo: overrides.clientInfo ?? {
      name: "native-differential-host",
      version: "1",
    },
  };
  for (const key of Object.keys(overrides.env ?? {}))
    assert.ok(
      ["NUMERA_CONFIG", "NUMERA_LOG_DIR"].includes(key),
      "fixture environment override must be explicit",
    );
  const transport = new StdioClientTransport({
    command: launch.command,
    args: launch.args,
    cwd: launch.cwd,
    env: {
      PATH: process.env.PATH ?? "",
      TEMP: join(workspace, "temp"),
      TMP: join(workspace, "temp"),
      TMPDIR: join(workspace, "temp"),
      ...(process.env.LLVM_PROFILE_FILE
        ? { LLVM_PROFILE_FILE: process.env.LLVM_PROFILE_FILE }
        : {}),
      ...overrides.env,
    },
    stderr: "pipe",
  });
  let stderr = "";
  transport.stderr?.on("data", (chunk) => {
    overrides.onStderr?.(chunk);
    stderr += chunk.toString("utf8").slice(0, 4096);
    if (stderr.length > 65536) stderr = stderr.slice(-65536);
  });
  const client = new Client(launch.clientInfo, {
    versionNegotiation: { mode },
  });
  let signalClosed;
  const closed = new Promise((accept) => {
    signalClosed = accept;
  });
  const host = {
    client,
    transport,
    launch,
    closed,
    pid: null,
    started: false,
    reaped: false,
    stderr: () => stderr,
  };
  const startTransport = transport.start.bind(transport);
  transport.start = () => {
    host.started = true;
    const pending = startTransport();
    // SDK close clears transport.pid before its asynchronous child reaping completes.
    host.pid = transport.pid;
    return pending;
  };
  hosts.push(host);
  client.onclose = signalClosed;
  try {
    await bounded(client.connect(transport, connectOptions), 8000);
    assert.ok(host.pid > 0, "actual native child required");
  } catch (error) {
    await closeHost(host);
    throw error;
  }
  return host;
}
start.supportsLaunchOverrides = true;
async function tool(host, name, args) {
  const result = await bounded(
    host.client.callTool({ name, arguments: args }, { timeout: 5000 }),
    6000,
  );
  assert.ok(result.structuredContent, "native structured result required");
  return result.structuredContent;
}
try {
  await mkdir(join(workspace, "temp"), { recursive: true });
  await bounded(
    new Promise((accept, reject) => {
      wire.once("error", reject);
      wire.listen(0, "127.0.0.1", accept);
    }),
  );
  const origin = `http://127.0.0.1:${wire.address().port}`;
  const connections = {};
  for (const adapter of ["responses", "interactions"]) {
    for (const mode of [
      ...states.filter(
        (s) => adapter === "interactions" || s !== "budget_exceeded",
      ),
      ...controls,
    ]) {
      connections[`${adapter}-${mode}`] = {
        adapter:
          adapter === "responses" ? "openai-responses" : "gemini-interactions",
        baseUrl: `${origin}/${adapter}/${mode}`,
        auth: { type: "none" },
        defaultModel: "opaque/image",
        orchestrationModel: "opaque/orchestrator",
        requestTimeoutMs: 3000,
        discoveryTimeoutMs: 2000,
      };
    }
  }
  const config = configSchema.parse({
    schemaVersion: 1,
    defaultConnection: Object.keys(connections)[0],
    outputDir: join(workspace, "outputs"),
    stateDir: join(workspace, "state"),
    logging: { directory: join(workspace, "logs"), level: "ERROR" },
    files: { maxAggregateBytes: 1048576, maxJournalBytes: 33554432 },
    connections,
  });
  const file = join(workspace, "config.json");
  await writeFile(file, JSON.stringify(config), "utf8");
  for (const mode of ["legacy", "auto"]) {
    const host = await start(file, mode);
    assert.equal(host.client.getProtocolEra(), "legacy");
    assert.equal((await bounded(host.client.listTools())).tools.length, 10);
    await closeHost(host);
  }
  await assert.rejects(start(file, { pin: "2026-07-28" }), {
    code: "ERA_NEGOTIATION_FAILED",
  });
  const ownedBeforeFailure = hosts.length;
  await assert.rejects(
    start(file, "legacy", {
      signal: AbortSignal.abort(new Error("Synthetic handshake cancellation")),
    }),
  );
  assert.equal(hosts.length, ownedBeforeFailure + 1);
  const rejectedHost = hosts.at(-1);
  assert.equal(rejectedHost.started, true);
  assert.ok(
    rejectedHost.pid > 0,
    "failed handshake retained its actual child PID",
  );
  assert.equal(
    rejectedHost.reaped,
    true,
    "failed handshake awaited actual child close",
  );
  const seeded = new Store(config.stateDir);
  try {
    for (const [name, connection] of Object.entries(config.connections)) {
      const mode = name.slice(name.indexOf("-") + 1);
      const id = "native-" + name;
      const now = new Date().toISOString();
      const receipt = seeded.prepare(
        {
          request_id: id,
          connection: name,
          adapter: connection.adapter,
          operation: "generate",
          status: "prepared",
          requested_model: "opaque/image",
          upstream_model: null,
          outputs: [],
          warnings: [],
          errors: [],
          deviations: [],
          generation_outcome: "not_submitted",
          storage_outcome: "not_started",
          another_submission_may_charge: true,
          created_at: now,
          updated_at: now,
          output_requirements: { count: 1 },
        },
        id,
        await connectionIdentity(name, connection),
      ).receipt;
      assert.equal(seeded.admit(id, name, 64, 64), true);
      receipt.status = "running";
      receipt.generation_outcome = "running";
      receipt.upstream_job = {
        id: "original-" + mode,
        kind:
          connection.adapter === "openai-responses"
            ? "responses"
            : "interactions",
      };
      seeded.update(receipt);
      seeded.release(id, receipt);
    }
  } finally {
    seeded.close();
  }
  let pair = [await start(file), await start(file)];
  const templates = await bounded(pair[0].client.listResourceTemplates(), 6000);
  assert.equal(templates.resourceTemplates.length, 1);
  const health = await tool(pair[0], "health_check", { probe: false });
  assert.equal(health.generation_verified, false);
  const models = await tool(pair[0], "list_models", {
    connection: "responses-500",
    refresh: true,
  });
  assert.equal(models.complete, false);
  const capabilities = await tool(pair[0], "get_model_capabilities", {
    connection: "responses-completed",
    model: "opaque/image",
  });
  assert.equal(capabilities.account_availability, "unknown");
  for (const args of [{ limit: 0 }, { offset: -1 }, { limit: "25" }]) {
    const result = await bounded(
      pair[0].client.callTool(
        { name: "list_outputs", arguments: args },
        { timeout: 5000 },
      ),
      6000,
    );
    assert.equal(result.isError, true);
  }
  for (const [name] of Object.entries(config.connections)) {
    const mode = name.slice(name.indexOf("-") + 1),
      id = "native-" + name;
    const receipt = await tool(pair[0], "get_job", {
      request_id: id,
      refresh: true,
    });
    if (states.includes(mode)) {
      if (mode === "completed") {
        assert.equal(receipt.status, "completed");
        assert.equal(receipt.outputs.length, 1);
      } else {
        assert.equal(
          receipt.upstream_terminal,
          mode === "budget_exceeded" ? "failed" : mode,
        );
        assert.equal(
          receipt.status,
          mode === "incomplete"
            ? "partial"
            : mode === "cancelled"
              ? "cancelled"
              : "failed",
        );
        if (mode === "incomplete") assert.equal(receipt.outputs.length, 1);
      }
      for (const output of receipt.outputs) {
        assert.equal(output.sha256, imageCases[0].metadata.sha256);
        assert.deepEqual(await readFile(output.path), bytes);
        const info = await tool(pair[1], "get_output_info", {
          output_id: output.output_id,
        });
        assert.equal(info.verified, true);
        const preview = await bounded(
          pair[1].client.callTool(
            {
              name: "get_output_info",
              arguments: { output_id: output.output_id, preview: true },
            },
            { timeout: 5000 },
          ),
          6000,
        );
        assert.ok(
          preview.content.some(
            (item) => item.type === "image" && item.mimeType === "image/png",
          ),
        );
        const resource = await bounded(
          pair[1].client.readResource({
            uri: `numera-image://outputs/${output.output_id}`,
          }),
          6000,
        );
        assert.deepEqual(
          Buffer.from(resource.contents[0].blob, "base64"),
          bytes,
        );
      }
    } else {
      assert.equal(receipt.status, "running");
      assert.equal(receipt.upstream_terminal, undefined);
      assert.equal(receipt.outputs.length, 0);
      if (!["queued", "in_progress"].includes(mode))
        assert.ok(receipt.errors.length > 0);
    }
    const other = await tool(pair[1], "get_job", {
      request_id: id,
      refresh: false,
    });
    assert.deepEqual(other, receipt);
  }
  for (let restart = 0; restart < 2; restart++) {
    await Promise.all(pair.map(closeHost));
    pair = [await start(file), await start(file)];
    for (const [name] of Object.entries(config.connections)) {
      const mode = name.slice(name.indexOf("-") + 1),
        id = "native-" + name;
      const receipt = await tool(pair[restart % 2], "get_job", {
        request_id: id,
        refresh: false,
      });
      if (states.includes(mode) && mode !== "completed")
        assert.equal(
          receipt.upstream_terminal,
          mode === "budget_exceeded" ? "failed" : mode,
        );
      if (["completed", "incomplete"].includes(mode)) {
        assert.equal(receipt.outputs.length, 1);
        assert.deepEqual(await readFile(receipt.outputs[0].path), bytes);
      }
    }
  }
  await toolsAcceptance({
    root,
    workspace,
    configSchema,
    start,
    closeHost,
    bounded,
  });
  await twoHostAcceptance({ root, workspace, start, closeHost, bounded });
  await compactAcceptance({
    root,
    workspace,
    binary,
    start,
    closeHost,
    bounded,
  });
  await envAcceptance({ root, workspace, binary, start, closeHost, bounded });
  await negotiationAcceptance({
    workspace,
    configSchema,
    start,
    closeHost,
    bounded,
  });
  await configReloadAcceptance({
    root,
    workspace,
    configSchema,
    Store,
    connectionIdentity,
    start,
    closeHost,
    bounded,
  });
  await terminalAcceptance({
    root,
    workspace,
    configSchema,
    Store,
    connectionIdentity,
    start,
    closeHost,
    bounded,
  });
  await lifecycleAcceptance({
    root,
    workspace,
    configSchema,
    Store,
    connectionIdentity,
    start,
    closeHost,
    bounded,
  });
  await transportAcceptance({
    root,
    workspace,
    configSchema,
    Store,
    connectionIdentity,
    start,
    closeHost,
    bounded,
  });
  await racesAcceptance({
    root,
    workspace,
    configSchema,
    start,
    closeHost,
    bounded,
  });
  assert.deepEqual(
    posts,
    [],
    "recovery and restart must never submit paid work",
  );
  process.stderr.write(
    JSON.stringify({
      component: "native-interop",
      message: "Two native SDK hosts and two restarts verified.",
      seeded_jobs: Object.keys(connections).length,
      paid_posts: posts.length,
    }) + "\n",
  );
} finally {
  const cleanupErrors = [];
  for (const host of hosts) {
    try {
      await closeHost(host);
    } catch (error) {
      cleanupErrors.push(error);
    }
  }
  for (const socket of sockets) socket.destroy();
  try {
    if (wire.listening)
      await bounded(new Promise((accept) => wire.close(accept)), 5000);
  } catch (error) {
    cleanupErrors.push(error);
  }
  if (cleanupErrors.length)
    throw new AggregateError(
      cleanupErrors,
      "Native fixture cleanup failed; state retained for diagnosis.",
    );
  await rm(workspace, { recursive: true, force: true });
}
