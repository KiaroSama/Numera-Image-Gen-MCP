#!/usr/bin/env node
import { StdioServerTransport } from "@modelcontextprotocol/server/stdio";
import { loadConfig, userRoot } from "./config/load.js";
import { Logger } from "./logging.js";
import { createServer } from "./mcp/server.js";
import { safeError } from "./errors.js";
import { join } from "node:path";
async function main() {
  let logger = new Logger(
    process.env.NUMERA_LOG_DIR ?? join(userRoot(), "logs"),
  );
  try {
    const config = await loadConfig();
    logger.close();
    logger = new Logger(
      config.logging.directory,
      config.logging.level,
      config.logging.retentionDays,
    );
    const { server, store, generation } = createServer(config, logger);
    let closed = false;
    const shutdown = async () => {
      if (closed) return;
      closed = true;
      for (const c of generation.active.values()) c.abort();
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
