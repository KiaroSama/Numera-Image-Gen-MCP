import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { resolve, join } from "node:path";
import { runBounded } from "./bounded.mjs";
import { maintenance, projectRoot } from "./logging.mjs";
await maintenance("package-check", async (log) => {
  const { Client } = await import("@modelcontextprotocol/client");
  const { StdioClientTransport } =
    await import("@modelcontextprotocol/client/stdio");
  const npm = resolve(
    process.env.npm_execpath ??
      join(projectRoot, "node_modules/npm/bin/npm-cli.js"),
  );
  async function run(args, cwd) {
    const result = await runBounded(process.execPath, [npm, ...args], {
      wall: 60000,
      idle: 40000,
      cwd,
      log,
      capture: true,
    });
    if (result.code)
      throw Object.assign(new Error("npm package operation failed."), {
        code: "PACKAGE_COMMAND_FAILED",
      });
    return result.stdout;
  }
  await mkdir(join(projectRoot, ".ci-work"), { recursive: true });
  const root = await mkdtemp(
    join(projectRoot, ".ci-work/package path with spaces-"),
  );
  let client, transport;
  try {
    const manifest = JSON.parse(
      await run(["pack", "--json", "--pack-destination", root], projectRoot),
    );
    const packed = Array.isArray(manifest)
      ? manifest[0]
      : Object.values(manifest)[0];
    for (const file of packed.files) {
      if (
        /(?:^|\/)(?:\.ai|\.specify|specs|\.claude|\.ignoreme|secrets\.md|state|outputs|logs|node_modules)(?:\/|$)|(?:^|\/)\.env(?:$|\.)/.test(
          file.path,
        )
      )
        throw Object.assign(new Error("Private artifact in package."), {
          code: "PRIVATE_PACKAGE_ARTIFACT",
        });
    }
    await writeFile(
      join(root, "package.json"),
      '{"private":true,"type":"module"}\n',
      "utf8",
    );
    await run(
      [
        "install",
        "--omit=dev",
        "--no-audit",
        "--no-fund",
        join(root, packed.filename),
      ],
      root,
    );
    const config = join(root, "config.json");
    await writeFile(
      config,
      JSON.stringify({
        schemaVersion: 1,
        outputDir: join(root, "outputs"),
        stateDir: join(root, "state"),
        logging: { directory: join(root, "logs") },
        connections: {},
      }),
      "utf8",
    );
    const bin = join(root, "node_modules/numera-image-gen-mcp/dist/index.js");
    await readFile(bin);
    transport = new StdioClientTransport({
      command: process.execPath,
      args: [bin],
      env: {
        PATH: process.env.PATH ?? "",
        NUMERA_CONFIG: config,
        NUMERA_LOG_DIR: join(root, "logs"),
      },
      stderr: "pipe",
    });
    client = new Client({ name: "packed-check", version: "1" });
    await client.connect(transport);
    transport.stderr?.resume();
    const result = await client.listTools({}, { timeout: 10000 });
    if (result.tools.length !== 10)
      throw new Error("Packed tool catalog mismatch.");
    log.emit(
      "INFO",
      "Clean production-only packed installation and MCP tool catalog passed.",
    );
  } finally {
    try {
      await client?.close();
    } finally {
      try {
        await transport?.close();
      } finally {
        await rm(root, { recursive: true, force: true });
      }
    }
  }
});
