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
          expect(body.prompt).toBe("image 日本語");
          json(res, { data: [{ b64_json: png.toString("base64") }] });
        },
        async (origin) => {
          await writeFile(
            join(install, "config.local.json"),
            JSON.stringify({
              api_endpoint: `${origin}/v1`,
              api_key: key,
              models: [
                { id: "custom/exact-one", name: "First friendly name" },
                { id: "custom/exact-two", name: "Second 日本語" },
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
            });
            expect(a.structuredContent).toMatchObject({
              status: "completed",
              requested_model: "custom/exact-one",
            });
            expect(b.structuredContent).toMatchObject({
              status: "completed",
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
              status: "completed",
            });
            expect(models).toHaveLength(2);
          } finally {
            await client.close();
            await transport.close();
          }
          for (const file of await readdir(join(install, "logs")))
            expect(
              await readFile(join(install, "logs", file), "utf8"),
            ).not.toContain(key);
        },
      );
    }),
  20000,
);
