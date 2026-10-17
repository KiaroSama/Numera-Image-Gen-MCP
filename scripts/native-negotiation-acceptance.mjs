import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

// Frozen integration/negotiation.test.ts at the actual native stdio boundary.
export async function negotiationAcceptance({
  workspace,
  configSchema,
  start,
  closeHost,
  bounded,
}) {
  assert.equal(
    process.env.GITHUB_ACTIONS,
    "true",
    "native negotiation acceptance is hosted only",
  );
  assert.equal(start.supportsLaunchOverrides, true);
  const directory = join(workspace, "negotiation-acceptance");
  await mkdir(directory);
  for (const mode of ["legacy", "auto", { pin: "2026-07-28" }]) {
    const caseRoot = join(directory, typeof mode === "string" ? mode : "pin");
    await mkdir(caseRoot);
    const config = configSchema.parse({
      schemaVersion: 1,
      outputDir: join(caseRoot, "outputs"),
      stateDir: join(caseRoot, "state"),
      logging: { directory: join(caseRoot, "logs"), level: "ERROR" },
      files: { allowedInputRoots: [join(caseRoot, "inputs")] },
      connections: {},
    });
    const file = join(caseRoot, "config.json");
    await writeFile(file, JSON.stringify(config), "utf8");
    const overrides = { clientInfo: { name: "era-fixture", version: "1" } };
    let host;
    try {
      if (typeof mode === "object") {
        await assert.rejects(start(file, mode, {}, overrides), {
          code: "ERA_NEGOTIATION_FAILED",
        });
      } else {
        host = await start(file, mode, {}, overrides);
        assert.equal((await bounded(host.client.listTools())).tools.length, 10);
        assert.equal(host.client.getProtocolEra(), "legacy");
      }
    } finally {
      if (host) await closeHost(host);
    }
  }
  process.stderr.write(
    JSON.stringify({
      component: "native-negotiation-acceptance",
      message:
        "Three exact protocol-era scenarios passed without a provider connection.",
      cases: 3,
    }) + "\n",
  );
}
