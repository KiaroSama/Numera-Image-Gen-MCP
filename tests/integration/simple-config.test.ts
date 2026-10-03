import { it, expect } from "vitest";
import { cp, mkdir, writeFile, readFile, readdir } from "node:fs/promises";
import { join, resolve } from "node:path";
import { Client } from "@modelcontextprotocol/client";
import { StdioClientTransport } from "@modelcontextprotocol/client/stdio";
import {
  workspace,
  server,
  json,
  requestBody,
  imageBytes,
} from "../fixtures/runtime.js";

it(
  "adjacent compact file starts from another CWD, shows names and forwards exact selected IDs privately",
  async () =>
    workspace(async (root) => {
      const install = join(root, "installation with spaces");
      await mkdir(install);
      await cp(resolve("dist"), join(install, "dist"), { recursive: true });
      const png = await imageBytes();
      const models: string[] = [];
      let gets = 0;
      const key = "synthetic-simple-private-key";
      await server(
        async (req, res) => {
          if (req.method === "GET") {
            gets++;
            json(res, { data: [] });
            return;
          }
          expect(req.headers.authorization).toBe(`Bearer ${key}`);
          const body = JSON.parse((await requestBody(req)).toString("utf8"));
          models.push(body.model);
          expect(req.url).toBe("/custom/api/v1/images/generations");
          expect(body.prompt).toBe("image 日本語");
          expect(body.size).toBe(
            body.model === "custom/exact-one" ? "1024x1024" : "1536x1024",
          );
          expect(body.quality).toBe(
            body.model === "custom/exact-one" ? "medium" : "high",
          );
          expect(body.output_format).toBe("png");
          json(res, { data: [{ b64_json: png.toString("base64") }] });
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
          const transport = new StdioClientTransport({
            command: process.execPath,
            args: [join(install, "dist/index.js")],
            cwd: root,
            env: { PATH: process.env.PATH ?? "" },
            stderr: "pipe",
          });
          const client = new Client({
            name: "simple-config-fixture",
            version: "1.0.0",
          });
          let stderr = "";
          transport.stderr?.on("data", (v: Buffer) => {
            stderr += v.toString("utf8");
          });
          try {
            await client.connect(transport);
            const call = async (
              name: string,
              args: Record<string, unknown> = {},
            ) => client.callTool({ name, arguments: args }, { timeout: 10000 });
            const listed = await call("list_models");
            expect(listed.isError).not.toBe(true);
            expect(listed.structuredContent).toMatchObject({
              models: [
                { id: "custom/exact-one", name: "First friendly name" },
                { id: "custom/exact-two", name: "Second 日本語" },
              ],
            });
            expect(gets).toBe(0);
            const a = await call("generate_image", {
              prompt: "image 日本語",
              request_id: "simple-first",
            });
            const b = await call("generate_image", {
              prompt: "image 日本語",
              model: "custom/exact-two",
              request_id: "simple-second",
              size: "1536x1024",
              quality: "high",
              output_format: "png",
            });
            expect(a.structuredContent).toMatchObject({
              status: "partial",
              requested_model: "custom/exact-one",
            });
            expect(b.structuredContent).toMatchObject({
              status: "partial",
              requested_model: "custom/exact-two",
            });
            expect(models).toEqual(["custom/exact-one", "custom/exact-two"]);
            const links = (
              a.structuredContent as { outputs: { path: string }[] }
            ).outputs;
            expect(await readFile(links[0]!.path)).toEqual(png);
            expect(JSON.stringify([listed, a, b]) + stderr).not.toContain(key);
            const duplicate = await call("generate_image", {
              prompt: "image 日本語",
              request_id: "simple-first",
            });
            expect(duplicate.structuredContent).toMatchObject({
              status: "partial",
            });
            expect(models).toHaveLength(2);
            const failed = await call("generate_image", {
              prompt: "private-error-prompt",
              request_id: "simple-first",
            });
            expect(failed.isError).toBe(true);
          } finally {
            await client.close();
            await transport.close();
          }
          const events: Record<string, unknown>[] = [];
          for (const file of await readdir(join(install, "logs"))) {
            const text = await readFile(join(install, "logs", file), "utf8");
            expect(text).not.toContain(key);
            expect(text).not.toContain("private-error-prompt");
            expect(text).not.toContain("image 日本語");
            events.push(
              ...text
                .trim()
                .split("\n")
                .filter(Boolean)
                .map((line) => JSON.parse(line)),
            );
          }
          const tools = events.filter((e) => e.component === "tool");
          expect(
            tools.some(
              (e) =>
                e.message === "Tool failed." &&
                e.code === "request_id_conflict",
            ),
          ).toBe(true);
          for (const done of tools.filter(
            (e) => e.message !== "Tool started.",
          )) {
            expect(done.duration_ms).toEqual(expect.any(Number));
            expect(done.run_id).toMatch(/^[a-f0-9-]{36}$/);
            expect(
              tools.some(
                (start) =>
                  start.call_id === done.call_id &&
                  start.run_id === done.run_id &&
                  start.message === "Tool started.",
              ),
            ).toBe(true);
          }
          expect(
            tools.some(
              (e) =>
                e.tool === "list_models" && e.message === "Tool completed.",
            ),
          ).toBe(true);
        },
      );
    }),
  20000,
);

it(
  "compact Responses sends one selected image tool with configured defaults and explicit overrides",
  async () =>
    workspace(async (root) => {
      const png = await imageBytes();
      let posts = 0;
      await server(
        async (req, res) => {
          posts++;
          expect(req.url).toBe("/deployment/v1/responses");
          const body = JSON.parse((await requestBody(req)).toString("utf8"));
          expect(body.model).toBe("opaque/orchestrator");
          expect(body.input[0].content).toEqual([
            { type: "input_text", text: "Responses 日本語" },
          ]);
          expect(body.tools).toEqual([
            {
              type: "image_generation",
              action: "generate",
              model: "opaque/image",
              output_format: "png",
              size: posts === 1 ? "1024x1024" : "1536x1024",
              quality: posts === 1 ? "medium" : "high",
            },
          ]);
          expect(body.tool_choice).toEqual({ type: "image_generation" });
          json(res, {
            output: [
              {
                type: "image_generation_call",
                status: "completed",
                result: png.toString("base64"),
              },
            ],
          });
        },
        async (origin) => {
          const file = join(root, "config.json");
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
          const transport = new StdioClientTransport({
            command: process.execPath,
            args: [resolve("dist/index.js"), "--config", file],
            env: { PATH: process.env.PATH ?? "" },
            stderr: "pipe",
          });
          const client = new Client({
            name: "responses-config-fixture",
            version: "1",
          });
          try {
            await client.connect(transport);
            transport.stderr?.on("data", () => {});
            for (const [id, args] of [
              ["defaults", {}],
              ["override", { size: "1536x1024", quality: "high" }],
            ] as const) {
              const result = await client.callTool(
                {
                  name: "generate_image",
                  arguments: {
                    prompt: "Responses 日本語",
                    request_id: id,
                    ...args,
                  },
                },
                { timeout: 10000 },
              );
              expect(result.isError).not.toBe(true);
              expect(result.structuredContent).toMatchObject({
                requested_model: "opaque/image",
                status: "partial",
              });
              const outputs = (
                result.structuredContent as { outputs: { path: string }[] }
              ).outputs;
              expect(await readFile(outputs[0]!.path)).toEqual(png);
            }
            expect(posts).toBe(2);
          } finally {
            await client.close();
            await transport.close();
          }
        },
      );
    }),
  20000,
);
