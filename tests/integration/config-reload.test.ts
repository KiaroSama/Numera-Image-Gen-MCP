import { Client, InMemoryTransport } from "@modelcontextprotocol/client";
import { writeFile, readFile, readdir } from "node:fs/promises";
import { join } from "node:path";
import { it, expect } from "vitest";
import { loadConfig } from "../../src/config/load.js";
import { ConfigReload } from "../../src/config/reload.js";
import { createServer } from "../../src/mcp/server.js";
import { Logger } from "../../src/logging.js";
import {
  workspace,
  server,
  json,
  configuration,
  connection,
  receipt,
  imageBytes,
  requestBody,
} from "../fixtures/runtime.js";
import { connectionIdentity } from "../../src/services/connection-identity.js";

it(
  "refreshes saved env models in an alive SDK client, retaining last good config on invalid saves",
  async () =>
    workspace(async (root) => {
      let requests = 0;
      await server(
        (_req, res) => {
          requests++;
          json(res, { data: [] });
        },
        async (origin) => {
          const file = join(root, ".env");
          const envText = (id: string, name: string) =>
            `API_ENDPOINT=${origin}/v1\nAPI_KEY=synthetic-test-key\nMODEL_1_ID=${id}\nMODEL_1_NAME="${name}"\nMODEL_1_SIZE=\n`;
          await writeFile(file, envText("first", "First"), "utf8");
          const args = ["--config", file];
          const config = await loadConfig(args, {});
          const logger = new Logger(config.logging.directory);
          const reload = new ConfigReload(config, file, args, {}, logger);
          const app = createServer(config, logger, undefined, reload);
          const client = new Client({ name: "reload-fixture", version: "1" });
          const [a, b] = InMemoryTransport.createLinkedPair();
          try {
            await Promise.all([app.server.connect(b), client.connect(a)]);
            const models = async () =>
              (
                await client.callTool(
                  {
                    name: "list_models",
                    arguments: {},
                  },
                  { timeout: 5000 },
                )
              ).structuredContent;
            expect(await models()).toMatchObject({
              models: [{ id: "first", name: "First" }],
            });
            await writeFile(file, envText("second", "تصویر دوم"), "utf8");
            expect(await models()).toMatchObject({
              models: [{ id: "second", name: "تصویر دوم" }],
            });
            await writeFile(
              file,
              envText("third", "Third") + "MODEL_1_ENABLED=maybe\n",
              "utf8",
            );
            expect(await models()).toMatchObject({
              models: [{ id: "second", name: "تصویر دوم" }],
            });
            expect(await models()).toMatchObject({
              models: [{ id: "second", name: "تصویر دوم" }],
            });
            expect(requests).toBe(0);
            const logs = (
              await Promise.all(
                (await readdir(config.logging.directory)).map((name) =>
                  readFile(join(config.logging.directory, name), "utf8"),
                ),
              )
            ).join("\n");
            expect(logs.match(/Saved configuration rejected/g)).toHaveLength(1);
            expect(logs).not.toContain("synthetic-test-key");
          } finally {
            await client.close();
            await app.server.close();
            app.store.close();
            logger.close();
          }
        },
      );
    }),
  20000,
);

it(
  "polls and cancels existing handles only against their original retained connection",
  () =>
    workspace(async (root) => {
      const paths: string[] = [];
      await server(
        (req, res) => {
          paths.push(req.url!);
          json(res, req.method === "POST" ? { cancelled: true } : {});
        },
        async (origin) => {
          const file = join(root, "config.json");
          const first = configuration(root, {
            local: connection("comfyui", {
              baseUrl: origin,
              baseUrlMode: "origin",
            }),
          });
          await writeFile(file, JSON.stringify(first), "utf8");
          const logger = new Logger(first.logging.directory, "ERROR");
          const reload = new ConfigReload(
            first,
            file,
            ["--config", file],
            {},
            logger,
          );
          const app = createServer(first, logger, undefined, reload);
          const client = new Client({ name: "recovery-fixture", version: "1" });
          const [a, b] = InMemoryTransport.createLinkedPair();
          const old = {
            ...receipt("existing", "local"),
            adapter: "comfyui",
            status: "running",
            upstream_job: { kind: "comfyui", id: "original-handle" },
          };
          app.store.prepare(
            old,
            "fixture-fingerprint",
            await connectionIdentity("local", first.connections.local!),
          );
          app.store.prepare(
            { ...old, request_id: "unknown" },
            "fixture-fingerprint",
            "unavailable-account",
          );
          try {
            await Promise.all([app.server.connect(b), client.connect(a)]);
            await writeFile(
              file,
              JSON.stringify({
                ...first,
                connections: {
                  local: {
                    ...first.connections.local,
                    baseUrl: origin + "/changed",
                    defaultModel: "new-model",
                  },
                },
              }),
              "utf8",
            );
            const poll = await client.callTool(
              {
                name: "get_job",
                arguments: { request_id: "existing", refresh: true },
              },
              { timeout: 5000 },
            );
            expect(poll.isError).not.toBe(true);
            const cancel = await client.callTool(
              { name: "cancel_job", arguments: { request_id: "existing" } },
              { timeout: 5000 },
            );
            expect(cancel.structuredContent).toMatchObject({
              upstream_requested: true,
              upstream_cancelled: true,
            });
            expect(paths).toEqual([
              "/history/original-handle",
              "/api/jobs/original-handle/cancel",
            ]);
            const unknown = await client.callTool(
              {
                name: "get_job",
                arguments: { request_id: "unknown", refresh: true },
              },
              { timeout: 5000 },
            );
            expect(unknown.isError).toBe(true);
            expect(unknown.structuredContent).toMatchObject({
              error: { code: "permission_denied" },
            });
            expect(paths).toHaveLength(2);
            await writeFile(
              file,
              JSON.stringify({
                ...first,
                connections: {
                  local: {
                    ...first.connections.local,
                    paths: { history: "changed-history" },
                  },
                },
              }),
              "utf8",
            );
            const ambiguous = await client.callTool(
              {
                name: "get_job",
                arguments: { request_id: "existing", refresh: true },
              },
              { timeout: 5000 },
            );
            expect(ambiguous.isError).toBe(true);
            expect(ambiguous.structuredContent).toMatchObject({
              error: { code: "permission_denied" },
            });
            expect(paths).toHaveLength(2);
          } finally {
            await client.close();
            await app.server.close();
            app.store.close();
            logger.close();
          }
        },
      );
    }),
  20000,
);

it(
  "keeps an in-flight generation on its captured config while the next tool sees saved models",
  () =>
    workspace(async (root) => {
      const bytes = await imageBytes();
      let submitted!: () => void, release!: () => void;
      const intent = new Promise<void>((resolve) => {
        submitted = resolve;
      });
      const held = new Promise<void>((resolve) => {
        release = resolve;
      });
      const bodies: unknown[] = [];
      await server(
        async (req, res) => {
          bodies.push(JSON.parse((await requestBody(req)).toString("utf8")));
          submitted();
          await held;
          json(res, { data: [{ b64_json: bytes.toString("base64") }] });
        },
        async (origin) => {
          const file = join(root, "config.json");
          const first = configuration(root, {
            local: connection("openai-images", {
              baseUrl: origin + "/v1",
              defaultModel: "old",
              configuredModels: [{ id: "old", name: "Old" }],
              requestTimeoutMs: 5000,
            }),
          });
          await writeFile(file, JSON.stringify(first), "utf8");
          const logger = new Logger(first.logging.directory, "ERROR");
          const app = createServer(
            first,
            logger,
            undefined,
            new ConfigReload(first, file, ["--config", file], {}, logger),
          );
          const client = new Client({ name: "snapshot-fixture", version: "1" });
          const [a, b] = InMemoryTransport.createLinkedPair();
          let pending: ReturnType<Client["callTool"]> | undefined;
          try {
            await Promise.all([app.server.connect(b), client.connect(a)]);
            pending = client.callTool(
              {
                name: "generate_image",
                arguments: { prompt: "fixture", request_id: "captured-config" },
              },
              { timeout: 7000 },
            );
            await Promise.race([
              intent,
              pending.then(() => {
                throw new Error(
                  "Generation returned before fixture submission.",
                );
              }),
            ]);
            await writeFile(
              file,
              JSON.stringify({
                ...first,
                connections: {
                  local: {
                    ...first.connections.local,
                    baseUrl: origin + "/new/v1",
                    defaultModel: "new",
                    configuredModels: [{ id: "new", name: "New" }],
                  },
                },
              }),
              "utf8",
            );
            const listed = await client.callTool(
              { name: "list_models", arguments: {} },
              { timeout: 5000 },
            );
            expect(listed.structuredContent).toMatchObject({
              models: [{ id: "new", name: "New" }],
            });
            release();
            const result = await pending;
            expect(result.structuredContent).toMatchObject({
              status: "completed",
              requested_model: "old",
              outputs: [{ bytes: bytes.length }],
            });
            expect(bodies).toEqual([{ model: "old", prompt: "fixture", n: 1 }]);
          } finally {
            release();
            await pending?.catch(() => {});
            app.abortActive();
            await client.close();
            await app.server.close();
            app.store.close();
            logger.close();
          }
        },
      );
    }),
  20000,
);
