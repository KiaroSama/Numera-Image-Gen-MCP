import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";

// Exact frozen integration/mcp.test.ts case, through two actual native hosts.
export async function twoHostAcceptance({
  root,
  workspace,
  start,
  closeHost,
  bounded,
}) {
  const directory = join(workspace, "two-host-acceptance");
  await mkdir(directory);
  const images = JSON.parse(
    await readFile(join(root, ".ci-work/parity/image-acceptance.json"), "utf8"),
  );
  const original = images.find((image) => image.name === "mcp-png");
  assert.ok(original, "exact frozen RGB20/40/60 alpha0.5 8x6 PNG required");
  assert.equal(original.metadata.width, 8);
  assert.equal(original.metadata.height, 6);
  const png = Buffer.from(original.base64, "base64");
  const sockets = new Set();
  const bodies = [];
  let submissions = 0;
  const provider = createServer((request, response) => {
    const chunks = [];
    let length = 0;
    request.on("data", (chunk) => {
      length += chunk.length;
      if (length > 1048576) request.destroy();
      else chunks.push(chunk);
    });
    request.on("error", () => response.destroy());
    request.on("end", () => {
      let result = { data: [{ id: "family/arbitrary" }] };
      if (request.method === "POST") {
        submissions++;
        try {
          bodies.push(JSON.parse(Buffer.concat(chunks).toString("utf8")));
        } catch {
          response.writeHead(500).end("{}");
          return;
        }
        result = { data: [{ b64_json: original.base64 }] };
      }
      response
        .writeHead(200, { "Content-Type": "application/json" })
        .end(JSON.stringify(result));
    });
  });
  provider.requestTimeout = 5000;
  provider.headersTimeout = 5000;
  provider.on("connection", (socket) => {
    sockets.add(socket);
    socket.once("close", () => sockets.delete(socket));
  });
  const hosts = [];
  try {
    await bounded(
      new Promise((accept, reject) => {
        provider.once("error", reject);
        provider.listen(0, "127.0.0.1", accept);
      }),
    );
    const config = join(directory, "config.json");
    await writeFile(
      config,
      JSON.stringify({
        schemaVersion: 1,
        defaultConnection: "fixture",
        outputDir: join(directory, "outputs"),
        stateDir: join(directory, "state"),
        logging: { directory: join(directory, "logs") },
        connections: {
          fixture: {
            adapter: "openai-images",
            baseUrl: `http://127.0.0.1:${provider.address().port}/v1`,
            auth: { type: "none" },
            defaultModel: "family/arbitrary",
          },
        },
      }),
      "utf8",
    );
    for (let index = 0; index < 2; index++) {
      const host = await start(
        undefined,
        "auto",
        {},
        {
          args: ["bridge"],
          cwd: root,
          clientInfo: { name: `host-${index}`, version: "1.0.0" },
          env: {
            NUMERA_CONFIG: config,
            NUMERA_LOG_DIR: join(directory, "logs"),
          },
        },
      );
      assert.deepEqual(host.launch.args, ["bridge"]);
      assert.equal(host.launch.clientInfo.name, `host-${index}`);
      hosts.push(host);
    }
    const catalog = await bounded(hosts[0].client.listTools());
    assert.equal(catalog.tools.length, 10);
    const args = {
      prompt: "تصویر بدون بازنویسی 123",
      request_id: "same-logical-id",
    };
    const results = await Promise.all(
      hosts.map((host) =>
        bounded(
          host.client.callTool(
            { name: "generate_image", arguments: args },
            { timeout: 10000 },
          ),
          11000,
        ),
      ),
    );
    assert.equal(submissions, 1);
    assert.equal(bodies[0].model, "family/arbitrary");
    assert.equal(bodies[0].prompt, args.prompt);
    assert.equal(bodies[0].n, 1);
    const job = await bounded(
      hosts[0].client.callTool({
        name: "get_job",
        arguments: { request_id: args.request_id },
      }),
    );
    const receipt = job.structuredContent;
    assert.equal(receipt.status, "completed");
    assert.equal(receipt.outputs[0].width, 8);
    assert.equal(receipt.outputs[0].height, 6);
    assert.deepEqual(await readFile(receipt.outputs[0].path), png);
    const conflict = await bounded(
      hosts[1].client.callTool({
        name: "generate_image",
        arguments: { ...args, prompt: "different" },
      }),
    );
    assert.equal(conflict.isError, true);
    assert.ok(JSON.stringify(conflict).includes("request_id_conflict"));
    assert.equal(submissions, 1);
    assert.ok(results.some((result) => !result.isError));
    process.stderr.write(
      JSON.stringify({
        component: "native-two-host-acceptance",
        message: "Exact shared request submitted once across two hosts.",
        submissions,
      }) + "\n",
    );
  } finally {
    try {
      const results = await Promise.allSettled(hosts.map(closeHost));
      const failures = results.filter((result) => result.status === "rejected");
      if (failures.length)
        throw new AggregateError(
          failures.map((result) => result.reason),
          "Two-host cleanup failed.",
        );
    } finally {
      for (const socket of sockets) socket.destroy();
      if (provider.listening)
        await bounded(new Promise((accept) => provider.close(accept)));
    }
  }
}
