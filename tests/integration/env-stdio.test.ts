import { it, expect } from "vitest";
import { cp, mkdir, writeFile, readFile, readdir } from "node:fs/promises";
import { join, resolve } from "node:path";
import { Client } from "@modelcontextprotocol/client";
import { StdioClientTransport } from "@modelcontextprotocol/client/stdio";
import { workspace } from "../fixtures/runtime.js";

it(
  "a running built stdio server reloads adjacent env from another CWD without revealing its key",
  async () =>
    workspace(async (root) => {
      const install = join(root, "installation with spaces");
      await mkdir(install);
      await cp(resolve("dist"), join(install, "dist"), { recursive: true });
      const file = join(install, ".env"),
        key = "synthetic-env-private-key";
      const config = (id: string, name: string) =>
        `API_ENDPOINT=https://api.example/v1\nAPI_KEY=${key}\nMODEL_1_ID=${id}\nMODEL_1_NAME="${name}"\n`;
      await writeFile(file, config("first/model", "First 日本語"), "utf8");
      const transport = new StdioClientTransport({
        command: process.execPath,
        args: [join(install, "dist/index.js")],
        cwd: root,
        env: { PATH: process.env.PATH ?? "" },
        stderr: "pipe",
      });
      const client = new Client({ name: "env-stdio-fixture", version: "1" });
      let stderr = "";
      transport.stderr?.on("data", (value: Buffer) => {
        stderr += value.toString("utf8");
      });
      try {
        await client.connect(transport);
        const list = () =>
          client.callTool(
            { name: "list_models", arguments: { refresh: false } },
            { timeout: 10000 },
          );
        const first = await list();
        expect(first.structuredContent).toMatchObject({
          models: [{ id: "first/model", name: "First 日本語" }],
        });
        await writeFile(file, config("second/model", "Second فارسی"), "utf8");
        const second = await list();
        expect(second.structuredContent).toMatchObject({
          models: [{ id: "second/model", name: "Second فارسی" }],
        });
        await writeFile(file, "API_KEY=\n", "utf8");
        expect((await list()).structuredContent).toMatchObject({
          models: [{ id: "second/model" }],
        });
        expect(JSON.stringify([first, second]) + stderr).not.toContain(key);
      } finally {
        await client.close();
        await transport.close();
      }
      const logs = await readdir(join(install, "logs"));
      for (const log of logs)
        expect(
          await readFile(join(install, "logs", log), "utf8"),
        ).not.toContain(key);
    }),
  20000,
);
