import assert from "node:assert/strict";
import {
  chmod,
  copyFile,
  mkdir,
  readFile,
  readdir,
  stat,
  writeFile,
} from "node:fs/promises";
import { basename, isAbsolute, join } from "node:path";

// Frozen integration/env-stdio.test.ts through one actual native SDK process.
export async function envAcceptance({
  root,
  workspace,
  binary,
  start,
  closeHost,
  bounded,
}) {
  assert.equal(
    process.env.GITHUB_ACTIONS,
    "true",
    "native dotenv acceptance is hosted only",
  );
  for (const path of [root, workspace, binary]) assert.ok(isAbsolute(path));
  assert.equal(start.supportsLaunchOverrides, true);
  const directory = join(workspace, "env-acceptance");
  const install = join(directory, "installation with spaces");
  await mkdir(join(install, "bin"), { recursive: true });
  const command = join(install, "bin", basename(binary));
  await copyFile(binary, command);
  if (process.platform !== "win32") await chmod(command, 0o755);
  const file = join(install, ".env");
  const key = "synthetic-env-private-key";
  const config = (id, name) =>
    `API_ENDPOINT=https://api.example/v1\nAPI_KEY=${key}\nMODEL_1_ID=${id}\nMODEL_1_NAME="${name}"\n`;
  await writeFile(file, config("first/model", "First 日本語"), "utf8");
  let stderr = "";
  let overflow = false;
  const launch = {
    command,
    args: ["bridge"],
    cwd: directory,
    clientInfo: { name: "env-stdio-fixture", version: "1" },
  };
  let host;
  try {
    host = await start(
      undefined,
      "auto",
      {},
      {
        ...launch,
        onStderr(chunk) {
          const next = chunk.toString("utf8");
          if (
            Buffer.byteLength(stderr, "utf8") +
              Buffer.byteLength(next, "utf8") >
            1048576
          )
            overflow = true;
          else stderr += next;
        },
      },
    );
    assert.deepEqual(host.launch, launch);
    const list = () =>
      bounded(
        host.client.callTool(
          { name: "list_models", arguments: { refresh: false } },
          { timeout: 10000 },
        ),
        11000,
      );
    const first = await list();
    assert.notEqual(first.isError, true);
    assert.deepEqual(
      first.structuredContent.models.map(({ id, name }) => ({ id, name })),
      [{ id: "first/model", name: "First 日本語" }],
    );
    await writeFile(file, config("second/model", "Second فارسی"), "utf8");
    const second = await list();
    assert.notEqual(second.isError, true);
    assert.deepEqual(
      second.structuredContent.models.map(({ id, name }) => ({ id, name })),
      [{ id: "second/model", name: "Second فارسی" }],
    );
    await writeFile(file, "API_KEY=\n", "utf8");
    const invalid = await list();
    assert.notEqual(invalid.isError, true);
    assert.deepEqual(
      invalid.structuredContent.models.map(({ id }) => id),
      ["second/model"],
    );
    assert.equal(
      (JSON.stringify([first, second]) + stderr).includes(key),
      false,
    );
  } finally {
    if (host) await closeHost(host);
  }
  assert.equal(overflow, false, "full dotenv session stderr exceeds one MiB");
  assert.equal(stderr.includes(key), false);
  const logs = await readdir(join(install, "logs"));
  assert.ok(logs.length > 0, "actual native file logs required");
  for (const name of logs) {
    const log = join(install, "logs", name);
    assert.ok((await stat(log)).size <= 1048576);
    assert.equal((await readFile(log, "utf8")).includes(key), false);
  }
  process.stderr.write(
    JSON.stringify({
      component: "native-env-acceptance",
      message:
        "Exact adjacent dotenv reload retained last valid models privately.",
    }) + "\n",
  );
}
