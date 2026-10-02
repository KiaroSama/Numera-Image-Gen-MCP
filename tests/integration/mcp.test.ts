import { createServer as httpServer } from "node:http";
import { mkdtemp, mkdir, writeFile, readFile, rm } from "node:fs/promises";
import { resolve, join } from "node:path";
import { Client } from "@modelcontextprotocol/client";
import { StdioClientTransport } from "@modelcontextprotocol/client/stdio";
import sharp from "sharp";
import { describe, it, expect } from "vitest";

describe("built stdio image bridge", () => {
  it("discovers ten tools, saves exact bytes and deduplicates two host processes", async () => {
    await mkdir(resolve(".ci-work"), { recursive: true });
    const root = await mkdtemp(resolve(".ci-work/mcp-"));
    const png = await sharp({
      create: {
        width: 8,
        height: 6,
        channels: 4,
        background: { r: 20, g: 40, b: 60, alpha: 0.5 },
      },
    })
      .png()
      .toBuffer();
    let submissions = 0;
    const bodies: unknown[] = [];
    const provider = httpServer(async (req, res) => {
      const chunks: Buffer[] = [];
      for await (const chunk of req) chunks.push(Buffer.from(chunk));
      if (req.method === "POST") {
        submissions++;
        bodies.push(JSON.parse(Buffer.concat(chunks).toString("utf8")));
        res.setHeader("Content-Type", "application/json");
        res.end(
          JSON.stringify({ data: [{ b64_json: png.toString("base64") }] }),
        );
      } else {
        res.setHeader("Content-Type", "application/json");
        res.end(JSON.stringify({ data: [{ id: "family/arbitrary" }] }));
      }
    });
    await new Promise<void>((r) => provider.listen(0, "127.0.0.1", r));
    const address = provider.address() as { port: number };
    const config = join(root, "config.json");
    await writeFile(
      config,
      JSON.stringify({
        schemaVersion: 1,
        defaultConnection: "fixture",
        outputDir: join(root, "outputs"),
        stateDir: join(root, "state"),
        logging: { directory: join(root, "logs") },
        connections: {
          fixture: {
            adapter: "openai-images",
            baseUrl: `http://127.0.0.1:${address.port}/v1`,
            auth: { type: "none" },
            defaultModel: "family/arbitrary",
          },
        },
      }),
      "utf8",
    );
    const clients: Client[] = [];
    const transports: StdioClientTransport[] = [];
    try {
      for (let i = 0; i < 2; i++) {
        const transport = new StdioClientTransport({
          command: process.execPath,
          args: [resolve("dist/index.js")],
          env: {
            PATH: process.env.PATH ?? "",
            NUMERA_CONFIG: config,
            NUMERA_LOG_DIR: join(root, "logs"),
          },
          stderr: "pipe",
        });
        const client = new Client({ name: `host-${i}`, version: "1.0.0" });
        clients.push(client);
        transports.push(transport);
        await client.connect(transport);
      }
      const tools = await clients[0]!.listTools();
      expect(tools.tools).toHaveLength(10);
      const args = {
        prompt: "تصویر بدون بازنویسی 123",
        request_id: "same-logical-id",
      };
      const results = await Promise.all(
        clients.map((client) =>
          client.callTool(
            { name: "generate_image", arguments: args },
            { timeout: 10000 },
          ),
        ),
      );
      expect(submissions).toBe(1);
      expect(bodies[0]).toMatchObject({
        model: "family/arbitrary",
        prompt: args.prompt,
        n: 1,
      });
      const job = await clients[0]!.callTool({
        name: "get_job",
        arguments: { request_id: args.request_id },
      });
      const receipt = job.structuredContent as {
        status: string;
        outputs: { path: string; width: number; height: number }[];
      };
      expect(receipt.status).toBe("completed");
      expect(receipt.outputs[0]).toMatchObject({ width: 8, height: 6 });
      expect(await readFile(receipt.outputs[0]!.path)).toEqual(png);
      const conflict = await clients[1]!.callTool({
        name: "generate_image",
        arguments: { ...args, prompt: "different" },
      });
      expect(conflict.isError).toBe(true);
      expect(JSON.stringify(conflict)).toContain("request_id_conflict");
      expect(submissions).toBe(1);
      expect(results.some((r) => !r.isError)).toBe(true);
    } finally {
      for (const client of clients) await client.close();
      for (const transport of transports) await transport.close();
      provider.closeAllConnections();
      await new Promise<void>((r) => provider.close(() => r()));
      await rm(root, { recursive: true, force: true });
    }
  }, 15000);
});
