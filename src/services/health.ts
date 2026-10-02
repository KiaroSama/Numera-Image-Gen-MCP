import { mkdir, access } from "node:fs/promises";
import { constants } from "node:fs";
import type { Config } from "../config/schema.js";
import { credentials } from "../config/credentials.js";
import { apiJson } from "../http/client.js";
import { connectionPrefix } from "./discovery.js";
import { safeError } from "../errors.js";
export async function listConnections(config: Config) {
  return Promise.all(
    Object.entries(config.connections).map(async ([name, c]) => {
      let ready = true;
      try {
        await credentials(c);
      } catch {
        ready = false;
      }
      return {
        name,
        adapter: c.adapter,
        gateway: c.gateway ?? null,
        enabled: c.enabled,
        credentials_ready: ready,
        default_model: c.defaultModel ?? null,
      };
    }),
  );
}
export async function health(
  config: Config,
  probe = false,
  signal?: AbortSignal,
) {
  const storage: Record<string, boolean> = {};
  for (const [key, path] of Object.entries({
    outputs: config.outputDir,
    state: config.stateDir,
    logs: config.logging.directory,
  })) {
    try {
      await mkdir(path, { recursive: true, mode: 0o700 });
      await access(path, constants.W_OK);
      storage[key] = true;
    } catch {
      storage[key] = false;
    }
  }
  const connections = await Promise.all(
    Object.entries(config.connections).map(async ([name, c]) => {
      let reachable: boolean | null = null,
        authentication_checked = false,
        error: unknown = null;
      if (probe && c.enabled) {
        try {
          await apiJson(
            c,
            c.adapter === "comfyui" ? "system_stats" : "models",
            connectionPrefix(c),
            {},
            signal,
          );
          reachable = true;
          authentication_checked = c.auth.type !== "none";
        } catch (e) {
          reachable = false;
          error = safeError(e);
        }
      }
      return {
        name,
        reachable,
        authentication_checked,
        generation_verified: false,
        error,
      };
    }),
  );
  return {
    configuration_valid: true,
    runtime: process.version,
    storage,
    connections,
    generation_verified: false,
  };
}
