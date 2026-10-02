import { Client } from "@modelcontextprotocol/client";
import { StdioClientTransport } from "@modelcontextprotocol/client/stdio";
import { mkdtemp, mkdir, writeFile, rm } from "node:fs/promises";
import { resolve, join } from "node:path";
const root = resolve(".ci-work");
await mkdir(root, { recursive: true });
const temp = await mkdtemp(join(root, "smoke-"));
const file = join(temp, "config.json");
await writeFile(
  file,
  JSON.stringify({
    schemaVersion: 1,
    outputDir: join(temp, "outputs"),
    stateDir: join(temp, "state"),
    logging: { directory: join(temp, "logs") },
    connections: {},
  }),
  "utf8",
);
const transport = new StdioClientTransport({
  command: process.execPath,
  args: [resolve("dist/index.js")],
  env: {
    PATH: process.env.PATH ?? "",
    NUMERA_CONFIG: file,
    NUMERA_LOG_DIR: join(temp, "logs"),
  },
  stderr: "pipe",
});
const client = new Client({ name: "numera-smoke", version: "1.0.0" });
try {
  await client.connect(transport);
  const tools = await client.listTools();
  if (tools.tools.length !== 10) throw new Error("Expected 10 tools.");
  const result = await client.callTool(
    { name: "health_check", arguments: {} },
    { timeout: 10000 },
  );
  if (result.isError) throw new Error("Local health failed.");
  process.stderr.write(
    "INFO: MCP handshake, 10 tools and local health passed; no provider contacted.\n",
  );
} finally {
  await client.close();
  await transport.close();
  await rm(temp, { recursive: true, force: true });
}
