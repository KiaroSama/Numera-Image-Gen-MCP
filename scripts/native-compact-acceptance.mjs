import assert from "node:assert/strict";
import { createServer } from "node:http";
import {
  chmod,
  copyFile,
  mkdir,
  readFile,
  readdir,
  stat,
  writeFile,
} from "node:fs/promises";
import { basename, isAbsolute, join } from "node:path";

// Frozen compact config cases use the actual native binary, including adjacent-file discovery.

async function fixture(bounded, respond, action) {
  const sockets = new Set();
  const requests = [];
  let failure;
  const wire = createServer((request, response) => {
    const chunks = [];
    let length = 0;
    request.on("data", (chunk) => {
      length += chunk.length;
      if (length > 1048576) {
        failure ??= new Error("Compact fixture body exceeds one MiB.");
        request.destroy();
      } else chunks.push(chunk);
    });
    request.on("error", (error) => {
      failure ??= error;
      response.destroy();
    });
    request.on("end", () => {
      try {
        assert.ok(length <= 1048576);
        const captured = {
          method: request.method,
          url: request.url,
          headers: request.headers,
          body: Buffer.concat(chunks, length),
        };
        requests.push(captured);
        const value = respond(captured);
        response.writeHead(200, { "Content-Type": "application/json" });
        response.end(JSON.stringify(value));
      } catch (error) {
        failure ??= error;
        response.writeHead(500, { "Content-Type": "application/json" });
        response.end("{}");
      }
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
    if (wire.listening)
      await bounded(
        new Promise((accept, reject) => {
          wire.close((error) => (error ? reject(error) : accept()));
        }),
      );
  }
}

function call(host, bounded, name, args = {}) {
  return bounded(
    host.client.callTool({ name, arguments: args }, { timeout: 10000 }),
    11000,
  );
}

function sessionStderr() {
  let text = "";
  let overflow = false;
  return {
    observe(chunk) {
      const next = chunk.toString("utf8");
      if (
        Buffer.byteLength(text, "utf8") + Buffer.byteLength(next, "utf8") >
        1048576
      ) {
        overflow = true;
      } else text += next;
    },
    read() {
      assert.equal(
        overflow,
        false,
        "full session stderr exceeded the fixture bound",
      );
      return text;
    },
  };
}

async function logAssertions(install, key) {
  const events = [];
  for (const name of await readdir(join(install, "logs"))) {
    const file = join(install, "logs", name);
    assert.ok(
      (await stat(file)).size <= 1048576,
      "bounded compact fixture log",
    );
    const text = await readFile(file, "utf8");
    for (const privateText of [key, "private-error-prompt", "image 日本語"]) {
      assert.equal(text.includes(privateText), false);
    }
    events.push(
      ...text
        .trim()
        .split("\n")
        .filter(Boolean)
        .map((line) => JSON.parse(line)),
    );
  }
  const tools = events.filter((event) => event.component === "tool");
  assert.ok(
    tools.some(
      (event) =>
        event.message === "Tool failed." &&
        event.code === "request_id_conflict",
    ),
  );
  for (const done of tools.filter(
    (event) => event.message !== "Tool started.",
  )) {
    assert.equal(typeof done.duration_ms, "number");
    assert.match(done.run_id, /^[a-f0-9-]{36}$/);
    assert.ok(
      tools.some(
        (start) =>
          start.call_id === done.call_id &&
          start.run_id === done.run_id &&
          start.message === "Tool started.",
      ),
    );
  }
  assert.ok(
    tools.some(
      (event) =>
        event.tool === "list_models" && event.message === "Tool completed.",
    ),
  );
}

// Canonical 8a5313bc729eb75ab8e667058579a65a148dac2832881f4dce64c206f0225906.
async function adjacent({ directory, binary, start, closeHost, bounded, png }) {
  const root = join(directory, "adjacent-case");
  const install = join(root, "installation with spaces");
  await mkdir(join(install, "bin"), { recursive: true });
  const installedBinary = join(install, "bin", basename(binary));
  await copyFile(binary, installedBinary);
  if (process.platform !== "win32") await chmod(installedBinary, 0o755);
  const models = [];
  let gets = 0;
  const key = "synthetic-simple-private-key";
  await fixture(
    bounded,
    (request) => {
      if (request.method === "GET") {
        gets++;
        return { data: [] };
      }
      assert.equal(request.headers.authorization, `Bearer ${key}`);
      const body = JSON.parse(request.body.toString("utf8"));
      models.push(body.model);
      assert.equal(request.url, "/custom/api/v1/images/generations");
      assert.equal(body.prompt, "image 日本語");
      assert.equal(
        body.size,
        body.model === "custom/exact-one" ? "1024x1024" : "1536x1024",
      );
      assert.equal(
        body.quality,
        body.model === "custom/exact-one" ? "medium" : "high",
      );
      assert.equal(body.output_format, "png");
      return { data: [{ b64_json: png.toString("base64") }] };
    },
    async (origin) => {
      await writeFile(
        join(install, "config.local.json"),
        JSON.stringify({
          api_endpoint: `${origin}/custom/api/v1`,
          api_key: key,
          models: [
            {
              id: "custom/exact-one",
              name: "First friendly name",
              size: "1024x1024",
              quality: "medium",
              output_format: "png",
            },
            {
              id: "custom/exact-two",
              name: "Second 日本語",
              size: "512x512",
              quality: "low",
              output_format: "jpeg",
            },
          ],
        }),
        { encoding: "utf8", mode: 0o600 },
      );
      const stderr = sessionStderr();
      const launch = {
        command: installedBinary,
        args: ["bridge"],
        cwd: root,
        clientInfo: { name: "simple-config-fixture", version: "1.0.0" },
      };
      let host;
      try {
        host = await start(
          undefined,
          "auto",
          {},
          { ...launch, onStderr: stderr.observe },
        );
        assert.deepEqual(
          host.launch,
          launch,
          "adjacent lookup requires the actual copied binary and foreign CWD, without --config",
        );
        const listed = await call(host, bounded, "list_models");
        assert.notEqual(listed.isError, true);
        assert.deepEqual(
          listed.structuredContent.models.map(({ id, name }) => ({ id, name })),
          [
            { id: "custom/exact-one", name: "First friendly name" },
            { id: "custom/exact-two", name: "Second 日本語" },
          ],
        );
        assert.equal(gets, 0);
        const first = await call(host, bounded, "generate_image", {
          prompt: "image 日本語",
          request_id: "simple-first",
        });
        const second = await call(host, bounded, "generate_image", {
          prompt: "image 日本語",
          model: "custom/exact-two",
          request_id: "simple-second",
          size: "1536x1024",
          quality: "high",
          output_format: "png",
        });
        assert.equal(first.structuredContent.status, "partial");
        assert.equal(
          first.structuredContent.requested_model,
          "custom/exact-one",
        );
        assert.equal(second.structuredContent.status, "partial");
        assert.equal(
          second.structuredContent.requested_model,
          "custom/exact-two",
        );
        assert.deepEqual(models, ["custom/exact-one", "custom/exact-two"]);
        assert.deepEqual(
          await readFile(first.structuredContent.outputs[0].path),
          png,
        );
        assert.equal(
          (JSON.stringify([listed, first, second]) + stderr.read()).includes(
            key,
          ),
          false,
        );
        const duplicate = await call(host, bounded, "generate_image", {
          prompt: "image 日本語",
          request_id: "simple-first",
        });
        assert.equal(duplicate.structuredContent.status, "partial");
        assert.equal(models.length, 2);
        const failed = await call(host, bounded, "generate_image", {
          prompt: "private-error-prompt",
          request_id: "simple-first",
        });
        assert.equal(failed.isError, true);
      } finally {
        if (host) await closeHost(host);
      }
      assert.equal(stderr.read().includes(key), false);
      await logAssertions(install, key);
    },
  );
}

// Canonical 978d41e2ce380337dc678129466098c25420ea32f624c6e9aa19676506527f14.
async function responses({
  root,
  directory,
  binary,
  start,
  closeHost,
  bounded,
  png,
}) {
  const caseRoot = join(directory, "responses-case");
  await mkdir(caseRoot);
  let posts = 0;
  await fixture(
    bounded,
    (request) => {
      posts++;
      assert.equal(request.url, "/deployment/v1/responses");
      const body = JSON.parse(request.body.toString("utf8"));
      assert.equal(body.model, "opaque/orchestrator");
      assert.deepEqual(body.input[0].content, [
        { type: "input_text", text: "Responses 日本語" },
      ]);
      assert.deepEqual(body.tools, [
        {
          type: "image_generation",
          action: "generate",
          model: "opaque/image",
          output_format: "png",
          size: posts === 1 ? "1024x1024" : "1536x1024",
          quality: posts === 1 ? "medium" : "high",
        },
      ]);
      assert.deepEqual(body.tool_choice, { type: "image_generation" });
      return {
        output: [
          {
            type: "image_generation_call",
            status: "completed",
            result: png.toString("base64"),
          },
        ],
      };
    },
    async (origin) => {
      const file = join(caseRoot, "config.json");
      await writeFile(
        file,
        JSON.stringify({
          api_endpoint: `${origin}/deployment/v1`,
          api_key: "synthetic-responses-key",
          profile: "openai-responses",
          orchestration_model: "opaque/orchestrator",
          models: [
            {
              id: "opaque/image",
              name: "Friendly",
              output_format: "png",
              size: "1024x1024",
              quality: "medium",
            },
          ],
        }),
        "utf8",
      );
      const launch = {
        command: binary,
        args: ["bridge", "--config", file],
        cwd: root,
        clientInfo: { name: "responses-config-fixture", version: "1" },
      };
      let host;
      try {
        host = await start(file, "auto", {}, launch);
        assert.deepEqual(host.launch, launch);
        for (const [id, args] of [
          ["defaults", {}],
          ["override", { size: "1536x1024", quality: "high" }],
        ]) {
          const result = await call(host, bounded, "generate_image", {
            prompt: "Responses 日本語",
            request_id: id,
            ...args,
          });
          assert.notEqual(result.isError, true);
          assert.equal(
            result.structuredContent.requested_model,
            "opaque/image",
          );
          assert.equal(result.structuredContent.status, "partial");
          assert.deepEqual(
            await readFile(result.structuredContent.outputs[0].path),
            png,
          );
        }
        assert.equal(posts, 2);
      } finally {
        if (host) await closeHost(host);
      }
    },
  );
}

export async function compactAcceptance({
  root,
  workspace,
  binary,
  start,
  closeHost,
  bounded,
}) {
  assert.equal(
    process.env.GITHUB_ACTIONS,
    "true",
    "native compact acceptance is hosted only",
  );
  for (const path of [root, workspace, binary]) assert.ok(isAbsolute(path));
  assert.equal(
    start.supportsLaunchOverrides,
    true,
    "Native launch options and lifecycle ownership are required",
  );
  const directory = join(workspace, "compact-acceptance");
  await mkdir(directory);
  const file = join(root, ".ci-work/parity/image-acceptance.json");
  assert.ok((await stat(file)).size <= 1048576);
  const rows = JSON.parse(await readFile(file, "utf8"));
  const original = rows.find((row) => row.name === "png");
  assert.ok(original, "exact source PNG fixture required");
  assert.equal(original.metadata.width, 8);
  assert.equal(original.metadata.height, 6);
  assert.equal(original.metadata.mime, "image/png");
  assert.equal(original.metadata.alpha, true);
  const png = Buffer.from(original.base64, "base64");
  assert.ok(png.length > 0);
  const context = { root, directory, binary, start, closeHost, bounded, png };
  await adjacent(context);
  await responses(context);
  process.stderr.write(
    JSON.stringify({
      component: "native-compact-acceptance",
      message: "Two exact compact source SDK scenarios passed.",
      cases: 2,
    }) + "\n",
  );
}
