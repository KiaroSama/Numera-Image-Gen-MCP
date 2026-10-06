import { Client, InMemoryTransport } from "@modelcontextprotocol/client";
import { it, expect } from "vitest";
import { readFile } from "node:fs/promises";
import { createServer } from "../../src/mcp/server.js";
import { Logger } from "../../src/logging.js";
import { Store, type Receipt } from "../../src/jobs/store.js";
import { connectionIdentity } from "../../src/services/connection-identity.js";
import type { Config } from "../../src/config/schema.js";
import {
  configuration,
  connection,
  workspace,
  server,
  json,
  imageBytes,
  receipt,
} from "../fixtures/runtime.js";

async function host(
  config: Config,
  run: (client: Client, store: Store) => Promise<void>,
) {
  const logger = new Logger(config.logging.directory, "ERROR");
  const app = createServer(config, logger);
  const client = new Client({ name: "transport-audit", version: "1" });
  const [a, b] = InMemoryTransport.createLinkedPair();
  try {
    await Promise.all([app.server.connect(b), client.connect(a)]);
    await run(client, app.store);
  } finally {
    app.abortActive();
    await client.close();
    await app.server.close();
    app.store.close();
    logger.close();
  }
}

it.each(["failed", "incomplete"] as const)(
  "preserves authoritative %s SSE through HTTP and the public generation tool",
  async (state) =>
    workspace(async (root) => {
      const bytes = await imageBytes();
      let posts = 0;
      await server(
        (req, res) => {
          if (req.method === "POST") posts++;
          const data = {
            response: {
              id: "owned",
              status: state,
              output:
                state === "incomplete"
                  ? [
                      {
                        type: "image_generation_call",
                        status: "completed",
                        result: bytes.toString("base64"),
                      },
                    ]
                  : [],
            },
          };
          res.writeHead(200, { "Content-Type": "text/event-stream" });
          const frame = `event: response.${state}\ndata: ${JSON.stringify(data)}\n\n`;
          res.write(frame.slice(0, 19));
          res.end(frame.slice(19));
        },
        async (origin) => {
          const config = configuration(root, {
            local: connection("openai-responses", {
              baseUrl: `${origin}/v1`,
              orchestrationModel: "text",
              defaultModel: "image",
            }),
          });
          await host(config, async (client, store) => {
            const call = {
              name: "generate_image",
              arguments: { prompt: "fixture", request_id: "sse" },
            };
            const first = await client.callTool(call, { timeout: 5000 });
            const result = first.structuredContent as unknown as Receipt;
            expect(result.generation_outcome).toBe(state);
            expect(result.status).toBe(
              state === "failed" ? "failed" : "partial",
            );
            if (state === "incomplete") {
              expect(result.outputs).toHaveLength(1);
              expect(await readFile(result.outputs[0]!.path)).toEqual(bytes);
            }
            await client.callTool(call, { timeout: 5000 });
            expect(posts).toBe(1);
            store.prepare(receipt("next"), "next", "fixture");
            expect(store.admit("next", "local", 1, 1)).toBe(true);
          });
        },
      );
    }),
);

it.each([
  "error",
  "malformed-terminal",
  "empty-terminal-id",
  "blank-terminal-id",
])("keeps %s SSE uncertain without a new submission", async (mode) =>
  workspace(async (root) => {
    let posts = 0;
    await server(
      (req, res) => {
        if (req.method === "POST") posts++;
        res.writeHead(200, { "Content-Type": "text/event-stream" });
        res.end(
          mode === "error"
            ? 'event: error\ndata: {"error":"synthetic"}\n\n'
            : `event: response.failed\ndata: ${JSON.stringify({ response: { status: "failed", ...(mode === "empty-terminal-id" ? { id: "" } : mode === "blank-terminal-id" ? { id: " " } : {}) } })}\n\n`,
        );
      },
      async (origin) => {
        const config = configuration(root, {
          local: connection("openai-responses", {
            baseUrl: `${origin}/v1`,
            orchestrationModel: "text",
            defaultModel: "image",
          }),
        });
        await host(config, async (client) => {
          const result = await client.callTool(
            {
              name: "generate_image",
              arguments: {
                prompt: "fixture",
                request_id: "uncertain",
              },
            },
            { timeout: 5000 },
          );
          expect(result.structuredContent).toMatchObject({
            status: "outcome_unknown",
            outputs: [],
          });
          expect(posts).toBe(1);
        });
      },
    );
  }),
);

for (const adapter of ["openai-responses", "gemini-interactions"] as const)
  for (const returned of ["other", undefined, "owned"] as const)
    it(`${adapter} validates completed job identity ${returned ?? "missing"} for waiter and refresh`, async () =>
      workspace(async (root) => {
        const bytes = await imageBytes();
        let posts = 0;
        await server(
          (req, res) => {
            if (req.method === "POST") {
              posts++;
              json(res, { id: "owned", status: "in_progress" });
            } else
              json(res, {
                ...(returned ? { id: returned } : {}),
                status: "completed",
                ...(adapter === "openai-responses"
                  ? {
                      output: [
                        {
                          type: "image_generation_call",
                          status: "completed",
                          result: bytes.toString("base64"),
                        },
                      ],
                    }
                  : {
                      steps: [
                        {
                          type: "model_output",
                          content: [
                            { type: "image", data: bytes.toString("base64") },
                          ],
                        },
                      ],
                    }),
              });
          },
          async (origin) => {
            const c = connection(adapter, {
              baseUrl: `${origin}/v1`,
              defaultModel: "image",
              orchestrationModel: "text",
            });
            const config = configuration(root, { local: c });
            await host(config, async (client, store) => {
              const waiting = await client.callTool(
                {
                  name: "generate_image",
                  arguments: {
                    prompt: "fixture",
                    request_id: "waiting",
                  },
                },
                { timeout: 5000 },
              );
              const first = waiting.structuredContent as unknown as Receipt;
              const saved = store.prepare(
                receipt("detached"),
                "fixture",
                await connectionIdentity("local", c),
              ).receipt;
              store.admit("detached", "local", 4, 4);
              saved.status = "running";
              saved.generation_outcome = "running";
              saved.adapter = adapter;
              saved.upstream_job = {
                id: "owned",
                kind:
                  adapter === "openai-responses" ? "responses" : "interactions",
              };
              store.update(saved);
              store.release("detached", saved);
              const refreshed = await client.callTool(
                {
                  name: "get_job",
                  arguments: {
                    request_id: "detached",
                    refresh: true,
                  },
                },
                { timeout: 5000 },
              );
              const second = refreshed.structuredContent as unknown as Receipt;
              for (const result of [first, second]) {
                if (returned === "owned") {
                  expect(result.outputs).toHaveLength(1);
                  expect(await readFile(result.outputs[0]!.path)).toEqual(
                    bytes,
                  );
                } else {
                  expect(result.status).toBe("running");
                  expect(result.outputs).toEqual([]);
                  expect(result.errors).toEqual(
                    expect.arrayContaining([
                      expect.objectContaining({ code: "invalid_response" }),
                    ]),
                  );
                }
              }
              expect(posts).toBe(1);
            });
          },
        );
      }));
