import { it, expect } from "vitest";
import { writeFile } from "node:fs/promises";
import { join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { cancelJob, getJob } from "../../src/services/jobs.js";
import { Generation } from "../../src/services/generation.js";
import { Store } from "../../src/jobs/store.js";
import { Logger } from "../../src/logging.js";
import {
  credentials,
  snapshotCredentials,
} from "../../src/config/credentials.js";
import { connectionIdentity } from "../../src/services/connection-identity.js";
import {
  configuration,
  connection,
  workspace,
  server,
  json,
  imageBytes,
  receipt,
} from "../fixtures/runtime.js";

it.each(["env", "file"] as const)(
  "pins %s credentials across queue waiting, but not across operations",
  async (kind) =>
    workspace(async (root) => {
      const key = "NUMERA_TEST_SNAPSHOT_KEY";
      const previous = process.env[key];
      const file = join(root, "fixture-key");
      const setKey = async (value: string) => {
        if (kind === "env") process.env[key] = value;
        else await writeFile(file, value, { encoding: "utf8", mode: 0o600 });
      };
      await setKey("synthetic-a");
      try {
        const bytes = await imageBytes();
        const accounts: boolean[] = [];
        await server(
          (req, res) => {
            accounts.push(req.headers.authorization === "Bearer synthetic-a");
            json(res, { data: [{ b64_json: bytes.toString("base64") }] });
          },
          async (origin) => {
            const c = connection("openai-images", {
              baseUrl: `${origin}/v1`,
              defaultModel: "image",
              requestTimeoutMs: 5000,
              auth: {
                type: "bearer",
                ...(kind === "env" ? { secretEnv: key } : { secretFile: file }),
              },
            });
            const config = configuration(root, { local: c });
            config.maxConcurrentRequests = 1;
            const store = new Store(config.stateDir);
            const logger = new Logger(config.logging.directory, "ERROR");
            let pending: ReturnType<Generation["run"]> | undefined;
            const controller = new AbortController();
            try {
              const blocker = store.prepare(
                receipt("blocker"),
                "fixture",
                "fixture",
              ).receipt;
              store.admit("blocker", "local", 1, 1);
              blocker.status = "running";
              blocker.upstream_job = { id: "blocker", kind: "responses" };
              store.update(blocker);
              store.release("blocker", blocker);
              const generation = new Generation(config, store, logger);
              pending = generation.run(
                { prompt: "fixture", request_id: "queued" },
                "generate",
                controller.signal,
              );
              void pending.catch(() => {});
              let prepared = false;
              const deadline = Date.now() + 2000;
              while (Date.now() < deadline) {
                try {
                  prepared = store.get("queued").status === "prepared";
                } catch {
                  /* Await the actual queue boundary. */
                }
                if (prepared) break;
                await delay(10);
              }
              expect(prepared).toBe(true);
              const originalIdentity = store.identity("queued");
              await setKey("synthetic-b");
              store.terminal("blocker", "cancelled", { code: "fixture" });
              const first = await pending;
              const next = await generation.run(
                { prompt: "fixture", request_id: "next" },
                "generate",
              );
              expect(accounts).toEqual([true, false]);
              expect(first.status).toBe("completed");
              expect(next.status).toBe("completed");
              expect(store.identity("queued")).toBe(originalIdentity);
              expect(store.identity("next")).not.toBe(originalIdentity);
              const persisted = JSON.stringify([first, next, config]);
              expect(persisted).not.toContain("synthetic-a");
              expect(persisted).not.toContain("synthetic-b");
            } finally {
              controller.abort();
              await pending?.catch(() => {});
              store.close();
              logger.close();
            }
          },
        );
      } finally {
        if (previous === undefined) delete process.env[key];
        else process.env[key] = previous;
      }
    }),
);

it("keeps secret snapshots ephemeral and returns independent header copies", async () => {
  const c = connection("openai-images", {
    auth: { type: "bearer", apiKey: "synthetic" },
  });
  const before = await connectionIdentity("local", c);
  const pinned = await snapshotCredentials(c);
  const headers = await credentials(pinned);
  headers.Authorization = "modified";
  expect(await credentials(pinned)).toEqual({
    Authorization: "Bearer synthetic",
  });
  expect(await connectionIdentity("local", pinned)).toBe(before);
  expect(Object.keys(pinned)).toEqual(Object.keys(c));
  expect(JSON.stringify(pinned)).toBe(JSON.stringify(c));
});

it("keeps the cancellation account through nested completed-job recovery", async () =>
  workspace(async (root) => {
    const key = "NUMERA_TEST_CANCEL_SNAPSHOT_KEY",
      previous = process.env[key];
    process.env[key] = "synthetic-a";
    try {
      const bytes = await imageBytes();
      const accounts: boolean[] = [];
      let submissions = 0;
      await server(
        (req, res) => {
          accounts.push(req.headers.authorization === "Bearer synthetic-a");
          if (req.url === "/api/jobs/owned/cancel")
            json(res, { cancelled: true });
          else if (req.url === "/api/jobs/owned") {
            process.env[key] = "synthetic-b";
            json(res, { id: "owned", status: "completed" });
          } else if (req.url === "/history/owned")
            json(res, {
              owned: {
                status: { completed: true, status_str: "success" },
                outputs: {
                  save: {
                    images: [
                      {
                        filename: "original.png",
                        type: "output",
                        subfolder: "",
                      },
                    ],
                  },
                },
              },
            });
          else if (req.url?.startsWith("/view")) {
            res.writeHead(200, { "Content-Type": "image/png" });
            res.end(bytes);
          } else {
            submissions++;
            json(res, {}, 404);
          }
        },
        async (origin) => {
          const c = connection("comfyui", {
            baseUrl: origin,
            baseUrlMode: "origin",
            auth: { type: "bearer", secretEnv: key },
            workflow: {
              graph: { save: { class_type: "Save", inputs: {} } },
              bindings: {},
              outputNodes: ["save"],
            },
          });
          const config = configuration(root, { local: c });
          const store = new Store(config.stateDir),
            logger = new Logger(config.logging.directory, "ERROR");
          const generation = new Generation(config, store, logger);
          try {
            const r = store.prepare(
              receipt("cancel-recover"),
              "hash",
              await connectionIdentity("local", c),
            ).receipt;
            store.admit(r.request_id, "local", 1, 1);
            r.status = "running";
            r.upstream_job = { id: "owned", kind: "comfyui" };
            store.update(r);
            store.release(r.request_id, r);
            const cancelled = await cancelJob(config, generation, r.request_id);
            expect(cancelled.refund_verified).toBe(false);
            const result = await getJob(generation, r.request_id);
            expect(result.status).toBe("completed");
            expect(result.outputs).toHaveLength(1);
            expect(accounts).toEqual([true, true, true, true]);
            expect(submissions).toBe(0);
            expect(JSON.stringify(result)).not.toContain("synthetic-a");
            expect(await credentials(c)).toEqual({
              Authorization: "Bearer synthetic-b",
            });
          } finally {
            store.close();
            logger.close();
          }
        },
      );
    } finally {
      if (previous === undefined) delete process.env[key];
      else process.env[key] = previous;
    }
  }));
