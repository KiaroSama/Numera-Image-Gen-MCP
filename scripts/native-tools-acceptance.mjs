import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";

// Frozen integration/tools.test.ts through the actual native stdio SDK boundary.
export async function toolsAcceptance({
  root,
  workspace,
  configSchema,
  start,
  closeHost,
  bounded,
}) {
  const directory = join(workspace, "tools-acceptance");
  await mkdir(directory);
  const images = JSON.parse(
    await readFile(join(root, ".ci-work/parity/image-acceptance.json"), "utf8"),
  );
  const original = images.find((image) => image.name === "png");
  assert.ok(original, "exact source 8x6 PNG required");
  const bytes = Buffer.from(original.base64, "base64");
  assert.equal(original.metadata.width, 8);
  assert.equal(original.metadata.height, 6);
  const sockets = new Set();
  let posts = 0;
  const wire = createServer((request, response) => {
    let length = 0;
    request.on("data", (chunk) => {
      length += chunk.length;
      if (length > 1048576) request.destroy();
    });
    request.on("error", () => response.destroy());
    request.on("end", () => {
      if (request.method === "POST") posts++;
      response
        .writeHead(200, { "Content-Type": "application/json" })
        .end(
          JSON.stringify(
            request.method === "POST"
              ? { data: [{ b64_json: original.base64 }] }
              : { data: [{ id: "image-model" }] },
          ),
        );
    });
  });
  wire.requestTimeout = 5000;
  wire.headersTimeout = 5000;
  wire.on("connection", (socket) => {
    sockets.add(socket);
    socket.once("close", () => sockets.delete(socket));
  });
  let host;
  try {
    await bounded(
      new Promise((accept, reject) => {
        wire.once("error", reject);
        wire.listen(0, "127.0.0.1", accept);
      }),
    );
    const config = configSchema.parse({
      schemaVersion: 1,
      defaultConnection: "local",
      outputDir: join(directory, "outputs"),
      stateDir: join(directory, "state"),
      logging: { directory: join(directory, "logs") },
      files: { allowedInputRoots: [join(directory, "inputs")] },
      connections: {
        local: {
          adapter: "openai-images",
          baseUrl: `http://127.0.0.1:${wire.address().port}/v1`,
          auth: { type: "none" },
          defaultModel: "image-model",
          edit: {
            mode: "multipart",
            encoding: "image",
            maxReferences: 1,
            masks: true,
            maskPolarity: "transparent-edit",
          },
        },
      },
    });
    assert.equal(config.connections.local.requestTimeoutMs, 300000);
    assert.equal(config.connections.local.discoveryTimeoutMs, 15000);
    const file = join(directory, "config.json");
    await writeFile(file, JSON.stringify(config), "utf8");
    host = await start(file);
    const catalog = await bounded(host.client.listTools({}, { timeout: 5000 }));
    assert.equal(catalog.tools.length, 10);
    const editing = catalog.tools.find((tool) => tool.name === "edit_image");
    assert.ok(editing?.description.includes("watermark"));
    assert.ok(editing?.description.includes("edit_region"));
    assert.equal(
      editing.description,
      "Image-to-image, comic editing, in-image translation and authorized watermark removal. Provide reference_images and an explicit prompt; edit_region={x,y,width,height} selects a pixel rectangle on the first reference and requires native mask support. target_language requests in-image translation. upscale=true requests a larger native size through the same model, with exactly one reference and numeric size; no local resizing. Unsupported references/masks fail before submission; no paid fallback.",
    );
    const call = (name, args) =>
      bounded(
        host.client.callTool({ name, arguments: args }, { timeout: 5000 }),
        6000,
      );
    for (const [name, args] of [
      ["health_check", { probe: true }],
      ["list_connections", {}],
      ["list_models", {}],
      ["get_model_capabilities", { model: "image-model" }],
    ]) {
      const result = await call(name, args);
      assert.notEqual(result.isError, true);
      assert.ok(result.structuredContent);
    }
    const generated = await call("generate_image", {
      prompt: "draw",
      request_id: "tools-generation",
      return_mode: "files_and_preview",
    });
    assert.equal(generated.isError, false);
    assert.deepEqual(
      Object.getOwnPropertySymbols(generated.structuredContent),
      [],
    );
    assert.ok(generated.content.some((item) => item.type === "image"));
    const id = generated.structuredContent.outputs[0].output_id;
    const resource = await bounded(
      host.client.readResource({ uri: `numera-image://outputs/${id}` }),
    );
    assert.equal(resource.contents[0].mimeType, "image/png");
    assert.equal(resource.contents[0].blob, bytes.toString("base64"));
    const info = await call("get_output_info", {
      output_id: id,
      preview: true,
    });
    assert.ok(info.content.some((item) => item.type === "image"));
    const edited = await call("edit_image", {
      prompt: "edit",
      request_id: "tools-edit",
      reference_images: [{ type: "output_id", output_id: id }],
    });
    assert.equal(edited.isError, false);
    assert.notEqual(
      (await call("get_job", { request_id: "tools-edit" })).isError,
      true,
    );
    assert.equal(
      (await call("list_outputs", { limit: 1 })).structuredContent.next_offset,
      1,
    );
    assert.equal(
      (await call("cancel_job", { request_id: "tools-edit" })).structuredContent
        .refund_verified,
      false,
    );
    assert.equal((await call("generate_image", { prompt: "" })).isError, true);
    assert.equal(
      (
        await call("get_output_info", {
          output_id: "00000000-0000-4000-8000-000000000000",
        })
      ).isError,
      true,
    );
    assert.equal(posts, 2);
    process.stderr.write(
      JSON.stringify({
        component: "native-tools-acceptance",
        message: "Exact source ten-tool SDK case passed.",
        posts,
      }) + "\n",
    );
  } finally {
    try {
      if (host) await closeHost(host);
    } finally {
      for (const socket of sockets) socket.destroy();
      if (wire.listening)
        await bounded(new Promise((accept) => wire.close(accept)));
    }
  }
}
