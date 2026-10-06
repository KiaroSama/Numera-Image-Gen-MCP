import { it, expect } from "vitest";
import { readFile } from "node:fs/promises";
import {
  configuration,
  connection,
  imageBytes,
  json,
  server,
  workspace,
} from "../fixtures/runtime.js";
import {
  bounded,
  gate,
  job,
  startHosts,
  tool,
} from "../fixtures/stdio-hosts.js";

it.each(["failed", "cancelled", "incomplete", "completed"] as const)(
  "fences a real Responses waiter when the other stdio host confirms %s",
  async (state) =>
    workspace(async (root) => {
      const bytes = await imageBytes();
      const polled = gate(),
        release = gate();
      let posts = 0,
        polls = 0;
      await server(
        async (req, res) => {
          if (req.method === "POST") {
            posts++;
            json(res, { id: "owned", status: "in_progress" });
          } else {
            polls++;
            if (polls === 1) {
              polled.resolve();
              await bounded(release.promise);
            }
            json(res, {
              id: "owned",
              status: state,
              ...(["completed", "incomplete"].includes(state)
                ? {
                    output: [
                      {
                        type: "image_generation_call",
                        status: "completed",
                        result: bytes.toString("base64"),
                      },
                    ],
                  }
                : {}),
            });
          }
        },
        async (origin) => {
          const config = configuration(root, {
            local: connection("openai-responses", {
              baseUrl: `${origin}/v1`,
              orchestrationModel: "text",
              defaultModel: "image",
              requestTimeoutMs: 5000,
            }),
          });
          const pair = await startHosts(root, config);
          let original: ReturnType<typeof tool> | undefined;
          try {
            original = tool(pair.hosts[0]!.client, "generate_image", {
              prompt: "draw",
              request_id: "race",
            });
            void original.catch(() => {});
            await bounded(polled.promise);
            const terminal = await job(pair.hosts[1]!.client, "race", true);
            expect(terminal.generation_outcome).toBe(state);
            release.resolve();
            await original;
            const persisted = await job(pair.hosts[0]!.client, "race", true);
            expect(persisted.generation_outcome).toBe(state);
            expect(persisted.outputs).toEqual(terminal.outputs);
            if (terminal.outputs.length)
              expect(await readFile(terminal.outputs[0]!.path)).toEqual(bytes);
            expect(posts).toBe(1);
            expect(polls).toBe(2);
          } finally {
            release.resolve();
            await original?.catch(() => {});
            await pair.close();
          }
          for (let restart = 0; restart < 2; restart++) {
            const reopened = await startHosts(root, config);
            try {
              for (const host of reopened.hosts)
                expect(
                  (await job(host.client, "race", true)).generation_outcome,
                ).toBe(state);
            } finally {
              await reopened.close();
            }
          }
          expect(posts).toBe(1);
        },
      );
    }),
  30000,
);

it.each(["cancelled", "failed", "completed"] as const)(
  "coordinates Comfy %s with an actual waiting host and preserves completed originals",
  async (state) =>
    workspace(async (root) => {
      const bytes = await imageBytes(),
        observed = gate(),
        release = gate();
      let posts = 0,
        cancellations = 0,
        downloads = 0;
      await server(
        async (req, res) => {
          if (req.url === "/object_info")
            json(res, {
              Text: { input: { required: { text: ["STRING"] } } },
              Save: {
                output_node: true,
                input: { required: { text: ["STRING"] } },
              },
            });
          else if (req.url === "/system_stats")
            json(res, { devices: [{ type: "cuda" }] });
          else if (req.url === "/prompt") {
            posts++;
            json(res, { prompt_id: "owned" });
          } else if (req.url === "/history/owned") {
            if (state !== "completed") {
              observed.resolve();
              await bounded(release.promise);
              json(res, {});
            } else
              json(res, {
                owned: {
                  status: { completed: true, status_str: "success" },
                  outputs: {
                    save: {
                      images: [
                        {
                          filename: "final.png",
                          subfolder: "",
                          type: "output",
                        },
                      ],
                    },
                  },
                },
              });
          } else if (req.url?.startsWith("/view")) {
            downloads++;
            if (downloads === 1) {
              observed.resolve();
              await bounded(release.promise);
            }
            res.writeHead(200, { "Content-Type": "image/png" });
            res.end(bytes);
          } else if (req.url === "/api/jobs/owned/cancel") {
            cancellations++;
            json(res, { cancelled: true });
          } else if (req.url === "/api/jobs/owned")
            json(res, {
              id: "owned",
              status: state === "completed" ? "cancelled" : state,
            });
          else json(res, {}, 404);
        },
        async (origin) => {
          const config = configuration(root, {
            local: connection("comfyui", {
              baseUrl: origin,
              baseUrlMode: "origin",
              defaultModel: "workflow",
              requestTimeoutMs: 5000,
              workflow: {
                graph: {
                  text: { class_type: "Text", inputs: { text: "" } },
                  save: { class_type: "Save", inputs: { text: "output" } },
                },
                bindings: { prompt: { node: "text", input: "text" } },
                outputNodes: ["save"],
              },
            }),
          });
          let pair = await startHosts(root, config);
          let original: ReturnType<typeof tool> | undefined;
          try {
            original = tool(pair.hosts[0]!.client, "generate_image", {
              prompt: "draw",
              request_id: "race",
            });
            void original.catch(() => {});
            await bounded(observed.promise);
            const cancelled = await tool(pair.hosts[1]!.client, "cancel_job", {
              request_id: "race",
            });
            expect(cancelled.isError).not.toBe(true);
            expect(cancelled.structuredContent).toMatchObject({
              refund_verified: false,
              upstream_cancelled: state === "cancelled",
            });
            await original;
            release.resolve();
            const r = await job(pair.hosts[1]!.client, "race");
            expect(r.generation_outcome).toBe(state);
            if (state !== "completed") expect(r.status).toBe(state);
          } finally {
            release.resolve();
            await original?.catch(() => {});
            await pair.close();
          }
          let outputId: string | undefined;
          for (let restart = 0; restart < 2; restart++) {
            pair = await startHosts(root, config);
            try {
              for (const host of pair.hosts) {
                const r = await job(host.client, "race", true);
                expect(r.generation_outcome).toBe(state);
                expect(r.status).toBe(state);
                if (state === "completed") {
                  expect(r.outputs).toHaveLength(1);
                  expect(await readFile(r.outputs[0]!.path)).toEqual(bytes);
                  if (outputId) expect(r.outputs[0]!.output_id).toBe(outputId);
                  outputId = r.outputs[0]!.output_id;
                }
              }
            } finally {
              await pair.close();
            }
          }
          expect(posts).toBe(1);
          expect(cancellations).toBe(1);
          expect(downloads).toBe(state === "completed" ? 2 : 0);
        },
      );
    }),
  30000,
);
