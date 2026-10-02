import {
  readFile,
  writeFile,
  mkdir,
  rename,
  open,
  rm,
  lstat,
  realpath,
} from "node:fs/promises";
import { dirname, basename, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { randomUUID } from "node:crypto";
import { constants } from "node:fs";
import { maintenance } from "./logging.mjs";

function refuse(message, code = "INVALID_HOST_CONFIG") {
  throw Object.assign(new Error(message), { code });
}
function mergeToml(text, action, node, entry, config) {
  const lines = text.match(/[^\n]*\n|[^\n]+$/g) ?? [];
  const sections = [];
  const headers = new Set();
  let section = "";
  let ownedKeys = new Set();
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i].trim();
    if (!line || line.startsWith("#")) continue;
    // ponytail: lossless bare-table edits only; use a CST parser if richer host TOML must be editable.
    if (line.includes('"""') || line.includes("'''"))
      refuse(
        "Multiline TOML is not safely editable; update the owned entry manually.",
        "AMBIGUOUS_TOML",
      );
    if (line.startsWith("[")) {
      const header =
        /^\[([A-Za-z0-9_-]+(?:\s*\.\s*[A-Za-z0-9_-]+)*)\]\s*(?:#.*)?$/.exec(
          line,
        );
      if (!header)
        refuse(
          "Quoted or array TOML tables are not safely editable.",
          "AMBIGUOUS_TOML",
        );
      section = header[1].replace(/\s/g, "");
      if (headers.has(section))
        refuse(
          "Duplicate TOML tables are not safely editable.",
          "AMBIGUOUS_TOML",
        );
      headers.add(section);
      sections.push({ start: i, name: section });
      ownedKeys = new Set();
      continue;
    }
    const key = /^([A-Za-z0-9_-]+(?:\s*\.\s*[A-Za-z0-9_-]+)*)\s*=/
      .exec(line)?.[1]
      ?.replace(/\s/g, "");
    if (!key)
      refuse(
        "Quoted or unrecognized TOML keys are not safely editable.",
        "AMBIGUOUS_TOML",
      );
    if (
      (!section && /^mcp_servers(?:\.|$)/.test(key)) ||
      (section === "mcp_servers" && /^numera-image-gen(?:\.|$)/.test(key))
    )
      refuse(
        "Inline or dotted MCP definitions are not safely editable.",
        "AMBIGUOUS_TOML",
      );
    if (/^mcp_servers\.numera-image-gen(?:\.|$)/.test(section)) {
      if (ownedKeys.has(key))
        refuse(
          "Duplicate owned TOML keys are not safely editable.",
          "AMBIGUOUS_TOML",
        );
      ownedKeys.add(key);
    }
    // A multiline array/string could make its following lines appear to be table headers.
    let quote = "",
      escaped = false,
      depth = 0;
    for (const char of line.slice(line.indexOf("=") + 1)) {
      if (quote) {
        if (escaped) escaped = false;
        else if (char === "\\" && quote === '"') escaped = true;
        else if (char === quote) quote = "";
      } else if (char === "#") break;
      else if (char === '"' || char === "'") quote = char;
      else if (char === "[" || char === "{") depth++;
      else if (char === "]" || char === "}") depth--;
      if (depth < 0) refuse("Malformed TOML value.", "AMBIGUOUS_TOML");
    }
    if (quote || depth)
      refuse(
        "Multiline or malformed TOML values are not safely editable.",
        "AMBIGUOUS_TOML",
      );
  }
  const owned = sections.filter(({ name }) =>
    /^mcp_servers\.numera-image-gen(?:\.|$)/.test(name),
  );
  if (owned.length && !headers.has("mcp_servers.numera-image-gen"))
    refuse(
      "Owned TOML subtables without an explicit parent are not safely editable.",
      "AMBIGUOUS_TOML",
    );
  for (let i = sections.length - 1; i >= 0; i--) {
    if (owned.includes(sections[i])) {
      const end = sections[i + 1]?.start ?? lines.length;
      lines.splice(sections[i].start, end - sections[i].start);
    }
  }
  const retained = lines.join("");
  if (action === "Remove") return retained;
  const nl = text.includes("\r\n") ? "\r\n" : "\n";
  const q = JSON.stringify;
  return (
    retained +
    (retained && !retained.endsWith("\n") ? nl : "") +
    [
      "[mcp_servers.numera-image-gen]",
      `command = ${q(node)}`,
      `args = [${q(entry)}]`,
      "startup_timeout_sec = 10",
      "tool_timeout_sec = 360",
      `env = { NUMERA_CONFIG = ${q(config)} }`,
      "",
    ].join(nl)
  );
}
export function mergeRegistration(text, client, action, node, entry, config) {
  if (client === "Codex") return mergeToml(text, action, node, entry, config);
  let parsed;
  try {
    parsed = text.trim() ? JSON.parse(text) : {};
  } catch {
    refuse("Host configuration must be valid JSON.");
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed))
    refuse("Host configuration must be an object.");
  if (parsed.mcpServers === undefined) {
    if (action === "Remove") return text;
    parsed.mcpServers = {};
  }
  if (
    !parsed.mcpServers ||
    typeof parsed.mcpServers !== "object" ||
    Array.isArray(parsed.mcpServers)
  )
    refuse("mcpServers must be an object.");
  if (action === "Remove") {
    if (!Object.hasOwn(parsed.mcpServers, "numera-image-gen")) return text;
    delete parsed.mcpServers["numera-image-gen"];
  } else
    parsed.mcpServers["numera-image-gen"] = {
      ...(client === "ClaudeProject" ? { type: "stdio" } : {}),
      command: node,
      args: [entry],
      env: { NUMERA_CONFIG: config },
    };
  return JSON.stringify(parsed, null, 2) + "\n";
}
async function snapshot(file) {
  let handle;
  const identity = (stat) =>
    [stat.dev, stat.ino, stat.size, stat.mtimeNs, stat.ctimeNs].join(":");
  try {
    const before = await lstat(file, { bigint: true });
    if (!before.isFile() || before.nlink !== 1n)
      refuse(
        "Host target must be a regular, unlinked file.",
        "UNSAFE_HOST_PATH",
      );
    handle = await open(file, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
    const stat = await handle.stat({ bigint: true });
    if (identity(before) !== identity(stat))
      refuse("Host target changed while opening.", "HOST_CHANGED");
    const bytes = await handle.readFile();
    const current = await lstat(file, { bigint: true });
    if (
      identity(stat) !== identity(current) ||
      identity(stat) !== identity(await handle.stat({ bigint: true }))
    )
      refuse("Host target changed while reading.", "HOST_CHANGED");
    const text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    return {
      bytes,
      text,
      identity: identity(stat),
      mode: Number(stat.mode & 0o777n),
    };
  } catch (error) {
    if (error.code === "ENOENT" && !handle)
      return { bytes: Buffer.alloc(0), text: "", identity: null, mode: 0o600 };
    throw error;
  } finally {
    await handle?.close();
  }
}
export async function register(args, log) {
  const [client, action, requestedFile, node, entry, config, option] = args;
  if (
    !["ClaudeProject", "ClaudeDesktop", "Codex"].includes(client) ||
    !["Register", "Remove"].includes(action) ||
    !requestedFile ||
    !isAbsolute(requestedFile) ||
    (option !== undefined && option !== "--dry-run") ||
    args.length > 7 ||
    (action === "Register" &&
      [node, entry, config].some((value) => !value || !isAbsolute(value)))
  )
    refuse("Invalid explicit registration request.", "INVALID_REQUEST");
  const dryRun = option === "--dry-run";
  const [major, minor] = process.versions.node.split(".").map(Number);
  if (major !== 24 || minor < 21)
    refuse("Node.js >=24.21.0 <25 is required.", "UNSUPPORTED_RUNTIME");
  if (dryRun) {
    const before = await snapshot(requestedFile);
    mergeRegistration(before.text, client, action, node, entry, config);
    log?.emit(
      "INFO",
      "Registration dry-run validated; no host files changed.",
      { client, action },
    );
    return;
  }
  if (action === "Remove" && !(await snapshot(requestedFile)).identity) return;
  await mkdir(dirname(requestedFile), { recursive: true });
  const file = join(
    await realpath(dirname(requestedFile)),
    basename(requestedFile),
  );
  const lock = `${file}.numera-lock`;
  let handle, temp;
  try {
    try {
      handle = await open(lock, "wx", 0o600);
    } catch (error) {
      if (error.code === "EEXIST")
        refuse(
          "Registration is locked by another request; retry after it finishes.",
          "REGISTRATION_LOCKED",
        );
      throw error;
    }
    await handle.writeFile(
      JSON.stringify({ pid: process.pid, started: new Date().toISOString() }) +
        "\n",
      "utf8",
    );
    const before = await snapshot(file);
    const after = mergeRegistration(
      before.text,
      client,
      action,
      node,
      entry,
      config,
    );
    if (after === before.text) {
      log?.emit("INFO", "Owned registration already matches; no changes.", {
        client,
        action,
      });
      return;
    }
    temp = `${file}.numera-${randomUUID()}.tmp`;
    await writeFile(temp, after, {
      encoding: "utf8",
      flag: "wx",
      mode: before.mode,
    });
    if (before.identity)
      await writeFile(`${file}.numera-backup-${randomUUID()}`, before.bytes, {
        flag: "wx",
        mode: 0o600,
      });
    const current = await snapshot(file);
    if (
      current.identity !== before.identity ||
      !current.bytes.equals(before.bytes)
    )
      refuse(
        "Host configuration changed concurrently; no overwrite performed.",
        "HOST_CHANGED",
      );
    await rename(temp, file);
    log?.emit(
      "INFO",
      "Only Numera registration updated; unrelated settings preserved.",
      { client, action },
    );
  } finally {
    try {
      if (temp) await rm(temp, { force: true });
    } finally {
      if (handle) {
        const owned = await handle.stat({ bigint: true });
        await handle.close();
        let current;
        try {
          current = await lstat(lock, { bigint: true });
        } catch (error) {
          if (error.code !== "ENOENT") throw error;
        }
        if (current && current.dev === owned.dev && current.ino === owned.ino)
          await rm(lock);
      }
    }
  }
}
if (
  process.argv[1] &&
  resolve(fileURLToPath(import.meta.url)) === resolve(process.argv[1])
)
  await maintenance("register", (log) => register(process.argv.slice(2), log));
