import { Client, InMemoryTransport } from "@modelcontextprotocol/client";
import { it, expect } from "vitest";
import { createServer } from "../../src/mcp/server.js";
import { readFile } from "node:fs/promises";
import { Generation } from "../../src/services/generation.js";
import { requestSchema } from "../../src/config/schema.js";
import { Logger } from "../../src/logging.js";
import { Store } from "../../src/jobs/store.js";
import { connectionIdentity } from "../../src/services/connection-identity.js";
import {
  configuration,
  connection,
  workspace,
  server,
  json,
  receipt,
  imageBytes,
} from "../fixtures/runtime.js";

it.each([
  ["openai-responses", "responses", "failed"],
  ["openai-responses", "responses", "cancelled"],
  ["gemini-interactions", "interactions", "failed"],
  ["openai-responses", "responses", "incomplete"],
] as const)(
  "persists authoritative %s %s %s through the public tool",
  async (adapter, kind, status) =>
    workspace(async (root) => {
      let gets = 0,
        posts = 0;
      await server(
        (req, res) => {
          if (req.method === "POST") posts++;
          else gets++;
          json(res, { id: "job", status });
        },
        async (origin) => {
          const config = configuration(root, {
            local: connection(adapter, {
              baseUrl: origin,
              defaultModel: "image-model",
            }),
          });
          const identity = await connectionIdentity(
            "local",
            config.connections.local!,
          );
          let store = new Store(config.stateDir);
          const old = store.prepare(receipt("old"), "hash", identity).receipt;
          store.admit("old", "local", 1, 1);
          old.status = "running";
          old.generation_outcome = "running";
          old.upstream_job = { id: "job", kind };
          store.update(old);
          store.release("old", old);
          for (let reopen = 0; reopen < 3; reopen++) {
            const logger = new Logger(config.logging.directory, "ERROR");
            const app = createServer(config, logger, store);
            const client = new Client({ name: "terminal-test", version: "1" });
            const [a, b] = InMemoryTransport.createLinkedPair();
            try {
              await Promise.all([app.server.connect(b), client.connect(a)]);
              for (let refresh = 0; refresh < 2; refresh++) {
                const result = await client.callTool(
                  {
                    name: "get_job",
                    arguments: { request_id: "old", refresh: true },
                  },
                  { timeout: 5000 },
                );
                expect(result.isError).not.toBe(true);
                expect(result.structuredContent).toMatchObject({
                  status: status === "incomplete" ? "failed" : status,
                  generation_outcome: status,
                  upstream_job: { id: "job", kind },
                });
              }
              const fresh = `fresh-${reopen}`;
              store.prepare(receipt(fresh), fresh, identity);
              expect(store.admit(fresh, "local", 1, 1)).toBe(true);
              const done = store.get(fresh);
              done.status = "failed";
              store.update(done);
              store.release(fresh, done);
              expect(
                store.prepare(receipt("old"), "hash", identity).fresh,
              ).toBe(false);
            } finally {
              await client.close();
              await app.server.close();
              logger.close();
              store.close();
            }
            if (reopen < 2) store = new Store(config.stateDir);
          }
          expect(gets).toBe(1);
          expect(posts).toBe(0);
        },
      );
    }),
);

it.each([
  "in_progress",
  "401",
  "429",
  "500",
  "disconnect",
  "malformed",
  "wrong-id",
  "identity",
])("retains live capacity for %s polling uncertainty", async (mode) =>
  workspace(async (root) => {
    let posts = 0,
      gets = 0;
    await server(
      (req, res) => {
        if (req.method === "POST") posts++;
        else gets++;
        if (mode === "disconnect") {
          res.destroy();
          return;
        }
        if (mode === "malformed") {
          res.end("not-json");
          return;
        }
        json(
          res,
          mode === "wrong-id"
            ? { id: "other", status: "failed" }
            : { id: "job", status: "in_progress" },
          mode === "401"
            ? 401
            : mode === "429"
              ? 429
              : mode === "500"
                ? 500
                : 200,
        );
      },
      async (origin) => {
        const config = configuration(root, {
          local: connection("openai-responses", {
            baseUrl: origin,
            defaultModel: "image-model",
          }),
        });
        const store = new Store(config.stateDir),
          logger = new Logger(config.logging.directory, "ERROR"),
          app = createServer(config, logger, store),
          client = new Client({ name: "poll-control", version: "1" });
        const [a, b] = InMemoryTransport.createLinkedPair();
        try {
          const identity = await connectionIdentity(
            "local",
            config.connections.local!,
          );
          const r = store.prepare(
            receipt("old"),
            "hash",
            mode === "identity" ? "other" : identity,
          ).receipt;
          store.admit("old", "local", 1, 1);
          r.status = "running";
          r.generation_outcome = "running";
          r.upstream_job = { id: "job", kind: "responses" };
          store.update(r);
          store.release("old", r);
          await Promise.all([app.server.connect(b), client.connect(a)]);
          await client.callTool(
            {
              name: "get_job",
              arguments: { request_id: "old", refresh: true },
            },
            { timeout: 5000 },
          );
          expect(store.get("old").upstream_terminal).toBeUndefined();
          expect(store.get("old").status).toBe("running");
          store.prepare(receipt("fresh"), "fresh", identity);
          expect(store.admit("fresh", "local", 1, 1)).toBe(false);
          expect(posts).toBe(0);
          expect(gets).toBe(mode === "identity" ? 0 : 1);
        } finally {
          await client.close();
          await app.server.close();
          store.close();
          logger.close();
        }
      },
    );
  }),
);

it.each([
  "confirmed",
  "dispatched",
  "false",
  "missing",
  "ambiguous",
  "completed",
])("coordinates targeted cancellation %s through public tools", async (mode) =>
  workspace(async (root) => {
    let cancellations = 0,
      generations = 0;
    await server(
      (req, res) => {
        if (req.method === "POST") {
          if (req.url?.endsWith("/cancel")) {
            cancellations++;
            if (mode === "ambiguous") {
              res.destroy();
              return;
            }
            json(
              res,
              mode === "missing" ? {} : { cancelled: mode !== "false" },
            );
          } else {
            generations++;
            json(res, {});
          }
        } else
          json(res, {
            id: "job",
            status: mode === "confirmed" ? "cancelled" : "in_progress",
          });
      },
      async (origin) => {
        const config = configuration(root, {
          local: connection("comfyui", {
            baseUrl: origin,
            baseUrlMode: "origin",
          }),
        });
        const identity = await connectionIdentity(
          "local",
          config.connections.local!,
        );
        let store = new Store(config.stateDir);
        const r = store.prepare(receipt("old"), "hash", identity).receipt;
        store.admit("old", "local", 1, 1);
        r.status = mode === "completed" ? "completed" : "running";
        r.generation_outcome = mode === "completed" ? "completed" : "running";
        r.upstream_job = { id: "job", kind: "comfyui" };
        store.update(r);
        store.release("old", r);
        const logger = new Logger(config.logging.directory, "ERROR"),
          app = createServer(config, logger, store),
          client = new Client({ name: "cancel-contract", version: "1" });
        const [a, b] = InMemoryTransport.createLinkedPair();
        try {
          await Promise.all([app.server.connect(b), client.connect(a)]);
          for (let n = 0; n < 2; n++) {
            const result = await client.callTool(
              { name: "cancel_job", arguments: { request_id: "old" } },
              { timeout: 5000 },
            );
            if (mode !== "ambiguous")
              expect(result.structuredContent).toMatchObject({
                refund_verified: false,
                upstream_cancelled:
                  mode === "confirmed"
                    ? true
                    : mode === "missing"
                      ? null
                      : false,
              });
          }
        } finally {
          await client.close();
          await app.server.close();
          logger.close();
          store.close();
        }
        for (let n = 0; n < 2; n++) {
          store = new Store(config.stateDir);
          try {
            expect(store.get("old").status).toBe(
              mode === "confirmed"
                ? "cancelled"
                : mode === "completed"
                  ? "completed"
                  : "running",
            );
            store.prepare(receipt(`fresh-${n}`), `fresh-${n}`, identity);
            expect(store.admit(`fresh-${n}`, "local", 1, 1)).toBe(
              ["confirmed", "completed"].includes(mode),
            );
            if (["confirmed", "completed"].includes(mode)) {
              const end = store.get(`fresh-${n}`);
              end.status = "failed";
              store.update(end);
              store.release(end.request_id, end);
            }
          } finally {
            store.close();
          }
        }
        expect(generations).toBe(0);
        expect(cancellations).toBe(
          mode === "completed" ? 0 : mode === "confirmed" ? 1 : 2,
        );
      },
    );
  }),
);

it("terminal partial originals survive local recovery without restoring generation capacity", async () =>
  workspace(async (root) => {
    const config = configuration(root),
      store = new Store(config.stateDir),
      logger = new Logger(config.logging.directory, "ERROR"),
      generation = new Generation(config, store, logger),
      bytes = await imageBytes();
    try {
      const r = store.prepare(receipt("partial"), "hash", "identity").receipt;
      store.admit("partial", "local", 1, 1);
      r.status = "running";
      r.upstream_job = { id: "job", kind: "responses" };
      store.update(r);
      const signal = AbortSignal.abort();
      const first = await generation.finish(
        r,
        {
          images: [{ bytes }],
          upstreamModel: null,
          upstreamId: "job",
          warnings: [],
          terminal: "failed",
        },
        requestSchema.parse({ prompt: "fixture", count: 1 }),
        signal,
      );
      expect(first.generation_outcome).toBe("failed");
      expect(store.pendingResults("partial")[0]?.bytes).toEqual(bytes);
      store.prepare(receipt("fresh"), "fresh", "identity");
      expect(store.admit("fresh", "local", 1, 1)).toBe(true);
      const end = store.get("fresh");
      end.status = "failed";
      store.update(end);
      store.release("fresh", end);
      const recovered = await generation.recoverLocal("partial");
      expect(recovered.generation_outcome).toBe("failed");
      expect(recovered.status).toBe("partial");
      expect(await readFile(recovered.outputs[0]!.path)).toEqual(bytes);
      expect(store.owns("partial")).toBe(false);
    } finally {
      store.close();
      logger.close();
    }
  }));
