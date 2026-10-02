import { readFile } from "node:fs/promises";
import { homedir } from "node:os";
import { isAbsolute, join } from "node:path";
import { parseArgs } from "node:util";
import { configSchema, type Config, type Connection } from "./schema.js";
import { fail } from "../errors.js";
import { resolveOperationUrl } from "../http/url.js";
export function userRoot(env: NodeJS.ProcessEnv = process.env) {
  return process.platform === "win32"
    ? join(env.LOCALAPPDATA ?? homedir(), "Numera", "ImageGen")
    : join(
        env.XDG_CONFIG_HOME ?? join(homedir(), ".config"),
        "numera-image-gen",
      );
}
export async function loadConfig(
  args: string[] = process.argv.slice(2),
  env: NodeJS.ProcessEnv = process.env,
): Promise<Config> {
  const { values } = parseArgs({
    args,
    options: {
      config: { type: "string" },
      "default-connection": { type: "string" },
      "output-dir": { type: "string" },
      "state-dir": { type: "string" },
      "log-dir": { type: "string" },
      "log-level": { type: "string" },
      "request-timeout-ms": { type: "string" },
      "max-concurrent-requests": { type: "string" },
      "return-mode": { type: "string" },
      "openai-compat": { type: "boolean" },
    },
    strict: true,
  });
  const root = userRoot(env),
    file = values.config ?? env.NUMERA_CONFIG ?? join(root, "config.json");
  if (!isAbsolute(file))
    fail("invalid_configuration", "NUMERA_CONFIG must be absolute.");
  let raw: Record<string, unknown>;
  if (values["openai-compat"])
    raw = {
      schemaVersion: 1,
      defaultConnection: "openai-compat",
      outputDir: join(root, "outputs"),
      stateDir: join(root, "state"),
      logging: { directory: join(root, "logs") },
      connections: {
        "openai-compat": {
          adapter: "openai-images",
          baseUrl: env.OPENAI_BASE_URL ?? "https://api.openai.com/v1",
          auth: { type: "bearer", secretEnv: "OPENAI_API_KEY" },
          defaultModel: env.OPENAI_MODEL,
        },
      },
    };
  else {
    try {
      raw = JSON.parse(await readFile(file, "utf8")) as Record<string, unknown>;
    } catch {
      return fail(
        "invalid_configuration",
        "Cannot read configuration. Set an absolute NUMERA_CONFIG to valid JSON.",
      );
    }
  }
  const get = (flag: string, key: string) =>
    values[flag as keyof typeof values] ?? env[key];
  for (const [field, flag, key] of [
    ["defaultConnection", "default-connection", "NUMERA_DEFAULT_CONNECTION"],
    ["outputDir", "output-dir", "NUMERA_OUTPUT_DIR"],
    ["stateDir", "state-dir", "NUMERA_STATE_DIR"],
    ["returnMode", "return-mode", "NUMERA_RETURN_MODE"],
  ]) {
    const v = get(flag!, key!);
    if (v !== undefined) raw[field!] = v;
  }
  const logging = (raw.logging as Record<string, unknown>) ?? {};
  for (const [field, flag, key] of [
    ["directory", "log-dir", "NUMERA_LOG_DIR"],
    ["level", "log-level", "NUMERA_LOG_LEVEL"],
  ]) {
    const v = get(flag!, key!);
    if (v !== undefined) logging[field!] = v;
  }
  raw.logging = logging;
  const global = get(
    "max-concurrent-requests",
    "NUMERA_MAX_CONCURRENT_REQUESTS",
  );
  if (global !== undefined) raw.maxConcurrentRequests = Number(global);
  const parsed = configSchema.safeParse(raw);
  if (!parsed.success)
    fail(
      "invalid_configuration",
      "Configuration does not match schemaVersion 1. Check the generated JSON Schema.",
    );
  const config = parsed.data;
  const timeout = get("request-timeout-ms", "NUMERA_REQUEST_TIMEOUT_MS");
  for (const connection of Object.values(config.connections)) {
    if (timeout !== undefined) {
      const n = Number(timeout);
      if (!Number.isInteger(n) || n < 1 || n > 3600000)
        fail("invalid_configuration", "Invalid request timeout.");
      connection.requestTimeoutMs = n;
    }
    if (connection.proxy) {
      const proxy = new URL(connection.proxy.url);
      if (
        proxy.username ||
        proxy.password ||
        proxy.search ||
        proxy.hash ||
        !["http:", "https:"].includes(proxy.protocol)
      )
        fail(
          "invalid_configuration",
          "Proxy URL must not contain credentials, query or fragments.",
        );
    }
    resolveOperationUrl(
      connection.baseUrl,
      connection.baseUrlMode,
      "models",
      "/v1",
      connection.query,
    );
    if (
      connection.auth.origin &&
      new URL(connection.auth.origin).origin !==
        new URL(connection.baseUrl).origin
    )
      fail("invalid_configuration", "Credential destination origin mismatch.");
    if (
      connection.auth.type !== "none" &&
      [
        connection.auth.secretEnv,
        connection.auth.secretFile,
        connection.auth.secretDpapiFile,
      ].filter(Boolean).length !== 1
    )
      fail(
        "invalid_configuration",
        "Select exactly one secret source per authenticated connection.",
      );
    if (
      connection.auth.type === "header" &&
      (!connection.auth.header ||
        !/^[a-zA-Z0-9-]+$/.test(connection.auth.header) ||
        /^(host|content-length|cookie)$/i.test(connection.auth.header))
    )
      fail("invalid_configuration", "Invalid authentication header.");
  }
  for (const path of [
    config.outputDir,
    config.stateDir,
    config.logging.directory,
    ...config.files.allowedInputRoots,
  ])
    if (!isAbsolute(path))
      fail(
        "invalid_configuration",
        "Storage and input roots must be absolute paths.",
      );
  if (config.defaultConnection && !config.connections[config.defaultConnection])
    fail("invalid_configuration", "Default connection does not exist.");
  return config;
}
export function selectConnection(
  config: Config,
  name?: string,
): [string, Connection] {
  const id = name ?? config.defaultConnection;
  if (!id || !config.connections[id]?.enabled)
    fail("invalid_input", "Select an enabled configured connection.");
  return [id, config.connections[id]!];
}
