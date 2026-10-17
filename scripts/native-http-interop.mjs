import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createServer } from "node:net";
import { mkdir, writeFile, readFile, rm } from "node:fs/promises";
import { join, resolve } from "node:path";
import { randomUUID } from "node:crypto";
import { pathToFileURL } from "node:url";
import { createRequire } from "node:module";

if (process.env.GITHUB_ACTIONS !== "true")
  throw new Error("Native HTTP verification is hosted CI only.");
const root = resolve(process.env.GITHUB_WORKSPACE ?? process.cwd());
const baseline = resolve(process.env.NUMERA_ORACLE_ROOT);
const requireBaseline = createRequire(join(baseline, "package.json"));
const { Client, StreamableHTTPClientTransport } = await import(
  pathToFileURL(requireBaseline.resolve("@modelcontextprotocol/client")).href
);
const binary =
  process.env.NUMERA_NATIVE_BINARY ??
  join(
    root,
    "target/release",
    process.platform === "win32"
      ? "numera-image-gen-mcp.exe"
      : "numera-image-gen-mcp",
  );
const workspace = join(root, ".ci-work/native-http", randomUUID());
const token = "synthetic-local-http-token-not-provider-key";
const clients = [];
let child,
  exited,
  port,
  stderr = "",
  started,
  stdoutBytes = 0;
async function bounded(promise, ms = 5000) {
  let timer;
  try {
    return await Promise.race([
      promise,
      new Promise((_, reject) => {
        timer = setTimeout(
          () => reject(new Error("Native HTTP fixture deadline exceeded.")),
          ms,
        );
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}
async function freePort() {
  const listener = createServer();
  await bounded(
    new Promise((accept, reject) => {
      listener.once("error", reject);
      listener.listen(0, "127.0.0.1", accept);
    }),
  );
  const port = listener.address().port;
  await bounded(
    new Promise((accept, reject) =>
      listener.close((error) => (error ? reject(error) : accept())),
    ),
  );
  return port;
}
async function start() {
  child = spawn(
    binary,
    [
      "http",
      "--port",
      String(port),
      "--config",
      join(workspace, "config.json"),
    ],
    {
      cwd: workspace,
      windowsHide: true,
      stdio: ["ignore", "pipe", "pipe"],
      env: {
        PATH: process.env.PATH ?? "",
        NUMERA_HTTP_TOKEN: token,
        TEMP: join(workspace, "temp"),
        TMP: join(workspace, "temp"),
        TMPDIR: join(workspace, "temp"),
        ...(process.env.LLVM_PROFILE_FILE
          ? { LLVM_PROFILE_FILE: process.env.LLVM_PROFILE_FILE }
          : {}),
      },
    },
  );
  exited = new Promise((accept, reject) => {
    child.once("error", reject);
    child.once("close", (code, signal) => accept({ code, signal }));
  });
  child.stdout.on("data", (chunk) => {
    stdoutBytes += chunk.length;
  });
  const ready = new Promise((accept, reject) => {
    child.stderr.on("data", (chunk) => {
      stderr = (stderr + chunk.toString("utf8")).slice(-65536);
      if (stderr.includes("Local MCP HTTP:")) accept();
    });
    child.once("exit", () =>
      reject(new Error("Native HTTP startup exited before readiness.")),
    );
  });
  started = child.pid;
  assert.ok(started > 0);
  await bounded(ready, 8000);
}
async function connect() {
  const client = new Client({ name: "native-http-fixture", version: "1" });
  const transport = new StreamableHTTPClientTransport(
    new URL(`http://127.0.0.1:${port}/mcp`),
    {
      requestInit: {
        headers: { Authorization: `Bearer ${token}` },
        redirect: "error",
      },
      fetch: (url, options) =>
        fetch(url, {
          ...options,
          signal: options?.signal
            ? AbortSignal.any([options.signal, AbortSignal.timeout(6000)])
            : AbortSignal.timeout(6000),
        }),
    },
  );
  clients.push({ client, transport });
  await bounded(client.connect(transport), 8000);
  return client;
}
try {
  await mkdir(join(workspace, "temp"), { recursive: true });
  await writeFile(
    join(workspace, "config.json"),
    JSON.stringify({
      schemaVersion: 1,
      defaultConnection: "offline",
      outputDir: join(workspace, "outputs"),
      stateDir: join(workspace, "state"),
      logging: { directory: join(workspace, "logs"), level: "ERROR" },
      connections: {
        offline: {
          adapter: "openai-images",
          baseUrl: "https://invalid.example/v1",
          auth: { type: "none" },
          defaultModel: "opaque/image",
          requestTimeoutMs: 3000,
        },
      },
    }),
    "utf8",
  );
  port = await freePort();
  await start();
  const pair = [await connect(), await connect()];
  for (const client of pair) {
    assert.equal((await bounded(client.listTools())).tools.length, 10);
    assert.equal(
      (await bounded(client.listResourceTemplates())).resourceTemplates.length,
      1,
    );
    const result = await bounded(
      client.callTool(
        { name: "health_check", arguments: { probe: false } },
        { timeout: 5000 },
      ),
      6000,
    );
    assert.equal(result.structuredContent.generation_verified, false);
    assert.ok(result.structuredContent.connections);
    const models = await bounded(
      client.callTool(
        { name: "list_models", arguments: { refresh: false } },
        { timeout: 5000 },
      ),
      6000,
    );
    assert.ok(models.structuredContent);
    const invalid = await bounded(
      client.callTool(
        { name: "list_outputs", arguments: { limit: 0 } },
        { timeout: 5000 },
      ),
      6000,
    );
    assert.equal(invalid.isError, true);
  }
  const url = `http://127.0.0.1:${port}/mcp`;
  for (const [headers, status] of [
    [{}, 401],
    [{ Authorization: "Bearer wrong" }, 401],
    [{ Authorization: `Bearer ${token}`, Origin: "https://evil.example" }, 403],
  ]) {
    const response = await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json", ...headers },
      body: "{}",
      signal: AbortSignal.timeout(3000),
    });
    assert.equal(response.status, status);
    await response.arrayBuffer();
  }
  const logs = await readFile(join(workspace, "config.json"), "utf8");
  assert.ok(!logs.includes(token));
  assert.ok(!stderr.includes(token));
  assert.equal(stdoutBytes, 0);
  process.stderr.write(
    JSON.stringify({
      component: "native-http-interop",
      message: "Two actual HTTP SDK clients verified.",
      tools: 10,
      paid_posts: 0,
    }) + "\n",
  );
} finally {
  const errors = [];
  for (const { client, transport } of clients) {
    try {
      await bounded(transport.terminateSession());
    } catch (error) {
      errors.push(error);
    }
    try {
      await bounded(client.close());
    } catch (error) {
      errors.push(error);
    }
  }
  if (child && child.exitCode === null && child.signalCode === null) {
    child.kill("SIGTERM");
    try {
      await bounded(exited, 10000);
    } catch (error) {
      child.kill("SIGKILL");
      await bounded(exited, 3000);
      errors.push(error);
    }
  }
  if (exited) {
    const result = await bounded(exited, 3000);
    if (process.platform !== "win32")
      assert.equal(result.code, 0, "HTTP shutdown must be graceful on POSIX");
  }
  if (started)
    assert.throws(() => process.kill(started, 0), "HTTP child must be reaped");
  if (errors.length)
    throw new AggregateError(
      errors,
      "HTTP fixture cleanup failed; state retained.",
    );
  await rm(workspace, { recursive: true, force: true });
}
