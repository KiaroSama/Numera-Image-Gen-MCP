import { spawnSync } from "node:child_process";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { resolve, join } from "node:path";
import { Client } from "@modelcontextprotocol/client";
import { StdioClientTransport } from "@modelcontextprotocol/client/stdio";
const npm = resolve(
  process.env.npm_execpath ?? "node_modules/npm/bin/npm-cli.js",
);
function run(args, cwd) {
  const r = spawnSync(process.execPath, [npm, ...args], {
    cwd,
    windowsHide: true,
    encoding: "utf8",
    timeout: 60000,
    maxBuffer: 2 * 1024 * 1024,
  });
  if (r.status !== 0) throw new Error(`npm ${args[0]} failed: ${r.stderr}`);
  return r.stdout;
}
await mkdir(".ci-work", { recursive: true });
const root = await mkdtemp(resolve(".ci-work/package path with spaces-"));
let client, transport;
try {
  const manifest = JSON.parse(
    run(["pack", "--json", "--pack-destination", root], process.cwd()),
  );
  const packed = Array.isArray(manifest)
    ? manifest[0]
    : Object.values(manifest)[0];
  for (const f of packed.files) {
    if (
      /(?:^|\/)(?:\.ai|\.specify|specs|\.claude|\.ignoreme|secrets\.md|state|outputs|logs|node_modules)(?:\/|$)|(?:^|\/)\.env(?:$|\.)/.test(
        f.path,
      )
    )
      throw new Error(`Private artifact in package: ${f.path}`);
  }
  await writeFile(
    join(root, "package.json"),
    '{"private":true,"type":"module"}\n',
    "utf8",
  );
  run(
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
  const result = await client.listTools();
  if (result.tools.length !== 10)
    throw new Error("Packed tool catalog mismatch.");
  process.stderr.write(
    "INFO: Clean production-only packed installation and MCP tool catalog passed.\n",
  );
} finally {
  await client?.close();
  await transport?.close();
  await rm(root, { recursive: true, force: true });
}
