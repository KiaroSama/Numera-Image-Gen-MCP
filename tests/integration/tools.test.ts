import { Client, InMemoryTransport } from "@modelcontextprotocol/client";
import { it, expect } from "vitest";
import { createServer } from "../../src/mcp/server.js";
import { Logger } from "../../src/logging.js";
import {
  configuration,
  connection,
  workspace,
  server,
  json,
  imageBytes,
} from "../fixtures/runtime.js";

it("executes every tool with schemas, resource bytes and bounded previews", async () =>
  workspace(async (root) => {
    const bytes = await imageBytes();
    await server(
      (req, res) => {
        json(
          res,
          req.method === "POST"
            ? { data: [{ b64_json: bytes.toString("base64") }] }
            : { data: [{ id: "image-model" }] },
        );
      },
      async (origin) => {
        const config = configuration(root, {
          local: connection("openai-images", {
            baseUrl: origin + "/v1",
            defaultModel: "image-model",
            edit: {
              mode: "multipart",
              encoding: "image",
              maxReferences: 1,
              masks: true,
              maskPolarity: "transparent-edit",
            },
          }),
        });
        const logger = new Logger(config.logging.directory, "ERROR"),
          app = createServer(config, logger),
          client = new Client({ name: "tool-contract", version: "1" });
        const [a, b] = InMemoryTransport.createLinkedPair();
        try {
          await Promise.all([app.server.connect(b), client.connect(a)]);
          const catalog = await client.listTools({}, { timeout: 5000 });
          const editing = catalog.tools.find(
            (tool) => tool.name === "edit_image",
          );
          expect(editing?.description).toContain("watermark");
          expect(editing?.description).toContain("edit_region");
          for (const [name, args] of [
            ["health_check", { probe: true }],
            ["list_connections", {}],
            ["list_models", {}],
            ["get_model_capabilities", { model: "image-model" }],
          ] as const) {
            const result = await client.callTool({ name, arguments: args });
            expect(result.isError).not.toBe(true);
            expect(result.structuredContent).toBeDefined();
          }
          const generated = await client.callTool({
            name: "generate_image",
            arguments: {
              prompt: "draw",
              request_id: "tools-generation",
              return_mode: "files_and_preview",
            },
          });
          expect(generated.isError).toBe(false);
          expect(generated.content.some((c) => c.type === "image")).toBe(true);
          const outputs = (
              generated.structuredContent as {
                outputs: { output_id: string }[];
              }
            ).outputs,
            id = outputs[0]!.output_id;
          const resource = await client.readResource({
            uri: `numera-image://outputs/${id}`,
          });
          expect(resource.contents[0]).toMatchObject({
            mimeType: "image/png",
            blob: bytes.toString("base64"),
          });
          const info = await client.callTool({
            name: "get_output_info",
            arguments: { output_id: id, preview: true },
          });
          expect(info.content.some((c) => c.type === "image")).toBe(true);
          const edit = await client.callTool({
            name: "edit_image",
            arguments: {
              prompt: "edit",
              request_id: "tools-edit",
              reference_images: [{ type: "output_id", output_id: id }],
            },
          });
          expect(edit.isError).toBe(false);
          expect(
            (
              await client.callTool({
                name: "get_job",
                arguments: { request_id: "tools-edit" },
              })
            ).isError,
          ).not.toBe(true);
          expect(
            (
              await client.callTool({
                name: "list_outputs",
                arguments: { limit: 1 },
              })
            ).structuredContent,
          ).toHaveProperty("next_offset", 1);
          expect(
            (
              await client.callTool({
                name: "cancel_job",
                arguments: { request_id: "tools-edit" },
              })
            ).structuredContent,
          ).toMatchObject({ refund_verified: false });
          expect(
            (
              await client.callTool({
                name: "generate_image",
                arguments: { prompt: "" },
              })
            ).isError,
          ).toBe(true);
          expect(
            (
              await client.callTool({
                name: "get_output_info",
                arguments: {
                  output_id: "00000000-0000-4000-8000-000000000000",
                },
              })
            ).isError,
          ).toBe(true);
        } finally {
          await client.close();
          await app.server.close();
          app.store.close();
          logger.close();
        }
      },
    );
  }));
