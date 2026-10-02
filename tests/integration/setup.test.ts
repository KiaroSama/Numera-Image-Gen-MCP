import { it, expect } from "vitest";
import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { workspace } from "../fixtures/runtime.js";

it("registers and removes only Numera JSON/TOML entries while retaining other settings", async () => {
  // @ts-expect-error JavaScript maintenance script is exercised as the actual public entrypoint.
  const { mergeRegistration } = await import("../../scripts/register.mjs");
  const json = JSON.stringify({
    model: "unchanged",
    mcpServers: { existing: { command: "existing" } },
  });
  const registered = JSON.parse(
    mergeRegistration(
      json,
      "ClaudeProject",
      "Register",
      "node",
      "entry",
      "config",
    ),
  );
  expect(registered).toMatchObject({
    model: "unchanged",
    mcpServers: {
      existing: { command: "existing" },
      "numera-image-gen": { command: "node" },
    },
  });
  expect(
    JSON.parse(
      mergeRegistration(
        JSON.stringify(registered),
        "ClaudeProject",
        "Remove",
        "node",
        "entry",
        "config",
      ),
    ),
  ).toEqual(JSON.parse(json));
  const toml =
    '# retained\nmodel = "unchanged"\n[mcp_servers.other]\ncommand = "existing"\n';
  const result = mergeRegistration(
    toml,
    "Codex",
    "Register",
    "node",
    "entry",
    "config",
  );
  expect(result).toContain(toml.trim());
  expect(result.match(/\[mcp_servers.numera-image-gen\]/g)).toHaveLength(1);
  expect(
    mergeRegistration(
      result,
      "Codex",
      "Register",
      "node",
      "entry",
      "config",
    ).match(/\[mcp_servers.numera-image-gen\]/g),
  ).toHaveLength(1);
  expect(
    mergeRegistration(result, "Codex", "Remove", "node", "entry", "config"),
  ).toContain("[mcp_servers.other]");
});
it("actual registration function backs up and preserves unrelated JSON entries", async () =>
  workspace(async (root) => {
    // @ts-expect-error JavaScript maintenance script is exercised as the actual public entrypoint.
    const { register } = await import("../../scripts/register.mjs");
    const file = join(root, "client.json");
    await writeFile(
      file,
      '{"model":"same","mcpServers":{"other":{"command":"stay"}}}',
      "utf8",
    );
    await register([
      "ClaudeDesktop",
      "Register",
      file,
      "node",
      "entry",
      "config",
    ]);
    const result = JSON.parse(await readFile(file, "utf8"));
    expect(result.model).toBe("same");
    expect(result.mcpServers.other.command).toBe("stay");
    await register([
      "ClaudeDesktop",
      "Remove",
      file,
      "node",
      "entry",
      "config",
    ]);
    expect(JSON.parse(await readFile(file, "utf8")).mcpServers).toEqual({
      other: { command: "stay" },
    });
  }));
