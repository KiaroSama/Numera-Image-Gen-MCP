import { Client, InMemoryTransport } from "@modelcontextprotocol/client";
import { it, expect } from "vitest";
import { createServer } from "../../src/mcp/server.js";
import { Logger } from "../../src/logging.js";
import { configuration, workspace } from "../fixtures/runtime.js";
it.each(["legacy", "auto", { pin: "2026-07-28" }] as const)(
  "negotiates supported protocol era %j without provider contact",
  async (mode) =>
    workspace(async (root) => {
      const config = configuration(root, {}),
        log = new Logger(config.logging.directory, "ERROR"),
        app = createServer(config, log),
        client = new Client(
          { name: "era-fixture", version: "1" },
          { versionNegotiation: { mode } },
        );
      const [a, b] = InMemoryTransport.createLinkedPair();
      try {
        await app.server.connect(b);
        if (typeof mode === "object") {
          await expect(client.connect(a)).rejects.toMatchObject({
            code: "ERA_NEGOTIATION_FAILED",
          });
        } else {
          await client.connect(a);
          expect((await client.listTools()).tools).toHaveLength(10);
          expect(client.getProtocolEra()).toBe("legacy");
        }
      } finally {
        await client.close();
        await app.server.close();
        app.store.close();
        log.close();
      }
    }),
);
