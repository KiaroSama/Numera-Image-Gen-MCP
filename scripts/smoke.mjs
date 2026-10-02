import { mkdtemp, mkdir, writeFile, rm } from "node:fs/promises";
import { join } from "node:path";
import { maintenance, projectRoot } from "./logging.mjs";
await maintenance("smoke", async (log) => {
  const { Client } = await import("@modelcontextprotocol/client");
  const { StdioClientTransport } =
    await import("@modelcontextprotocol/client/stdio");
  const root = join(projectRoot, ".ci-work");
  await mkdir(root, { recursive: true });
  const temp = await mkdtemp(join(root, "smoke-"));
  let client, transport;
  try {
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
    transport = new StdioClientTransport({
      command: process.execPath,
      args: [join(projectRoot, "dist/index.js")],
      env: {
        PATH: process.env.PATH ?? "",
        NUMERA_CONFIG: file,
        NUMERA_LOG_DIR: join(temp, "logs"),
      },
      stderr: "pipe",
    });
    client = new Client({ name: "numera-smoke", version: "1.0.0" });
    await client.connect(transport);
    transport.stderr?.resume();
    const tools = await client.listTools({}, { timeout: 10000 });
    if (tools.tools.length !== 10) throw new Error("Expected 10 tools.");
    const result = await client.callTool(
      { name: "health_check", arguments: {} },
      { timeout: 10000 },
    );
    if (result.isError) throw new Error("Local health failed.");
    log.emit(
      "INFO",
      "MCP handshake, 10 tools and local health passed; no provider contacted.",
    );
  } finally {
    try {
      await client?.close();
    } finally {
      try {
        await transport?.close();
      } finally {
        await rm(temp, { recursive: true, force: true });
      }
    }
  }
});
