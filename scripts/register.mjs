import { readFile, writeFile, mkdir, rename, copyFile } from "node:fs/promises";
import { dirname, isAbsolute, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { randomUUID } from "node:crypto";
export function mergeRegistration(text, client, action, node, entry, config) {
  if (client === "Codex") {
    const lines = text.split(/\r?\n/);
    const start = lines.findIndex((l) =>
      /^\s*\[mcp_servers\.numera-image-gen\]\s*(?:#.*)?$/.test(l),
    );
    if (start >= 0) {
      let end = start + 1;
      while (
        end < lines.length &&
        !/^\s*\[(?!mcp_servers\.numera-image-gen[.\]])/.test(lines[end])
      )
        end++;
      lines.splice(start, end - start);
    }
    if (action === "Register") {
      const q = JSON.stringify;
      lines.push(
        "",
        "[mcp_servers.numera-image-gen]",
        `command = ${q(node)}`,
        `args = [${q(entry)}]`,
        "startup_timeout_sec = 10",
        "tool_timeout_sec = 360",
        "env = { NUMERA_CONFIG = " + q(config) + " }",
      );
    }
    return lines.join("\n");
  }
  const parsed = text.trim() ? JSON.parse(text) : {};
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed))
    throw new Error("Host configuration must be an object.");
  parsed.mcpServers ??= {};
  if (typeof parsed.mcpServers !== "object" || Array.isArray(parsed.mcpServers))
    throw new Error("mcpServers must be an object.");
  if (action === "Remove") delete parsed.mcpServers["numera-image-gen"];
  else
    parsed.mcpServers["numera-image-gen"] = {
      ...(client === "ClaudeProject" ? { type: "stdio" } : {}),
      command: node,
      args: [entry],
      env: { NUMERA_CONFIG: config },
    };
  return JSON.stringify(parsed, null, 2) + "\n";
}
export async function register(args) {
  const [client, action, file, node, entry, config] = args;
  if (
    !["ClaudeProject", "ClaudeDesktop", "Codex"].includes(client) ||
    !["Register", "Remove"].includes(action) ||
    !isAbsolute(file)
  )
    throw new Error("Invalid explicit registration request.");
  let before = "";
  try {
    before = await readFile(file, "utf8");
  } catch (e) {
    if (e.code !== "ENOENT") throw e;
  }
  const after = mergeRegistration(before, client, action, node, entry, config);
  await mkdir(dirname(file), { recursive: true });
  if (before) await copyFile(file, `${file}.numera-backup-${randomUUID()}`);
  const temp = `${file}.numera-${randomUUID()}.tmp`;
  await writeFile(temp, after, { encoding: "utf8", flag: "wx", mode: 0o600 });
  let current = "";
  try {
    current = await readFile(file, "utf8");
  } catch (e) {
    if (e.code !== "ENOENT") throw e;
  }
  if (current !== before)
    throw new Error(
      "Host configuration changed concurrently; preserved original and temporary result. No overwrite.",
    );
  await rename(temp, file);
  process.stderr.write(
    "INFO: Only Numera registration updated; existing host settings preserved.\n",
  );
}
if (
  process.argv[1] &&
  resolve(fileURLToPath(import.meta.url)) === resolve(process.argv[1])
)
  await register(process.argv.slice(2));
