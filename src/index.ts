#!/usr/bin/env node
import { StdioServerTransport } from "@modelcontextprotocol/server/stdio";
import { configFile, loadConfig, userRoot } from "./config/load.js";
import { ConfigReload } from "./config/reload.js";
import { readConfigText } from "./config/env.js";
import { Logger } from "./logging.js";
import { createServer } from "./mcp/server.js";
import { safeError } from "./errors.js";
import { join } from "node:path";
async function main() {
  let logger = new Logger(
    process.env.NUMERA_LOG_DIR ?? join(userRoot(), "logs"),
  );
  try {
    const args = process.argv.slice(2),
      env = { ...process.env };
    const file = await configFile(args, env);
    const pinnedArgs = file ? [...args, "--config", file] : args;
    const config = await loadConfig(
      pinnedArgs,
      env,
      file ? await readConfigText(file) : undefined,
    );
    logger.close();
    logger = new Logger(
      config.logging.directory,
      config.logging.level,
      config.logging.retentionDays,
    );
    const reload = file
      ? new ConfigReload(config, file, pinnedArgs, env, logger)
      : undefined;
    const { server, store, abortActive } = createServer(
      config,
      logger,
      undefined,
      reload,
    );
    let closed = false;
    const shutdown = async () => {
      if (closed) return;
      closed = true;
      abortActive();
      await server.close();
      store.close();
      logger.log("INFO", "MCP", "Server stopped.");
      logger.close();
    };
    for (const signal of ["SIGINT", "SIGTERM"] as const)
      process.once(signal, () => {
        void shutdown();
      });
    const transport = new StdioServerTransport();
    transport.onerror = () =>
      logger.log("ERROR", "MCP", "Protocol transport error.");
    transport.onclose = () => {
      void shutdown();
    };
    await server.connect(transport);
    logger.log("INFO", "MCP", "Stdio server ready.", {
      runtime: process.version,
    });
  } catch (e) {
    logger.log("ERROR", "startup", "Server cannot start.", {
      error: safeError(e),
    });
    logger.close();
    process.exitCode = 1;
  }
}
void main();
