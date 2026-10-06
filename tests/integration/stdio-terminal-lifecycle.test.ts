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
import { seedJobs, startHosts, job, tool } from "../fixtures/stdio-hosts.js";

const controls = [
  "queued",
  "in_progress",
  "401",
  "429",
  "500",
  "disconnect",
  "malformed",
  "wrong-id",
  "identity",
];

it.each(["openai-responses", "gemini-interactions"] as const)(
  "persists every %s terminal and polling control across two real hosts and two restarts",
  async (adapter) =>
    workspace(async (root) => {
      const bytes = await imageBytes();
      const states = [
        "failed",
        "cancelled",
        "incomplete",
        "completed",
        ...(adapter === "gemini-interactions" ? ["budget_exceeded"] : []),
      ];
      const modes = [...states, ...controls];
      const posts: string[] = [];
      const image =
        adapter === "openai-responses"
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
                  content: [{ type: "image", data: bytes.toString("base64") }],
                },
              ],
            };
      await server(
        (req, res) => {
          const mode = req.url!.split("/")[1]!;
          if (req.method === "POST") {
            posts.push(mode);
            json(res, { id: "probe", status: "completed", ...image });
          } else if (mode === "disconnect") res.destroy();
          else if (mode === "malformed") res.end("not-json");
          else
            json(
              res,
              {
                id: mode === "wrong-id" ? "other" : mode,
                status: states.includes(mode)
                  ? mode
                  : mode === "queued"
                    ? "queued"
                    : "in_progress",
                ...(["completed", "incomplete"].includes(mode) ? image : {}),
              },
              ["401", "429", "500"].includes(mode) ? Number(mode) : 200,
            );
        },
        async (origin) => {
          const config = configuration(
            root,
            Object.fromEntries(
              modes.map((mode) => [
                mode,
                connection(adapter, {
                  baseUrl: `${origin}/${mode}/v1`,
                  orchestrationModel: "text",
                  defaultModel: "image",
                  requestTimeoutMs: 700,
                  maxConcurrentRequests: 1,
                }),
              ]),
            ),
          );
          config.maxConcurrentRequests = 32;
          config.files.maxAggregateBytes = 4096;
          config.files.maxJournalBytes = 1024 * 1024;
          const kind =
            adapter === "openai-responses" ? "responses" : "interactions";
          await seedJobs(
            config,
            modes.map((mode) => ({
              id: mode,
              connection: mode,
              kind,
              ...(mode === "identity" ? { identity: "other-account" } : {}),
            })),
          );
          const outputIds = new Map<string, string>();
          const seenPids = new Set<number>();
          for (let round = 0; round < 3; round++) {
            const pair = await startHosts(root, config);
            try {
              for (const host of pair.hosts) {
                expect(seenPids.has(host.pid)).toBe(false);
                seenPids.add(host.pid);
              }
              for (const mode of modes) {
                await Promise.all(
                  pair.hosts.map((h) =>
                    tool(h.client, "get_job", {
                      request_id: mode,
                      refresh: true,
                    }),
                  ),
                );
                for (const host of pair.hosts) {
                  const r = await job(host.client, mode);
                  if (states.includes(mode)) {
                    const terminal =
                      mode === "budget_exceeded" ? "failed" : mode;
                    expect(r.generation_outcome).toBe(terminal);
                    expect(r.status).toBe(
                      mode === "completed"
                        ? "completed"
                        : mode === "incomplete"
                          ? "partial"
                          : mode === "cancelled"
                            ? "cancelled"
                            : "failed",
                    );
                    if (["completed", "incomplete"].includes(mode)) {
                      expect(r.outputs).toHaveLength(1);
                      expect(await readFile(r.outputs[0]!.path)).toEqual(bytes);
                      if (outputIds.has(mode))
                        expect(r.outputs[0]!.output_id).toBe(
                          outputIds.get(mode),
                        );
                      else outputIds.set(mode, r.outputs[0]!.output_id);
                    }
                  } else {
                    expect(r.status).toBe("running");
                    expect(r.upstream_terminal).toBeUndefined();
                  }
                  expect(r.upstream_job).toEqual({ id: mode, kind });
                }
                const before = posts.length;
                const probe = await tool(
                  pair.hosts[1]!.client,
                  "generate_image",
                  {
                    connection: mode,
                    prompt: "admission probe",
                    request_id: `probe-${mode}-${round}`,
                  },
                );
                expect(posts.length - before).toBe(
                  states.includes(mode) ? 1 : 0,
                );
                if (states.includes(mode)) expect(probe.isError).not.toBe(true);
                const replay = await tool(
                  pair.hosts[0]!.client,
                  "generate_image",
                  {
                    connection: mode,
                    prompt: "different fingerprint",
                    request_id: mode,
                  },
                );
                expect(replay.isError).toBe(true);
                expect(JSON.stringify(replay)).toContain("request_id_conflict");
                expect(posts.length - before).toBe(
                  states.includes(mode) ? 1 : 0,
                );
              }
            } finally {
              await pair.close();
            }
          }
          expect(seenPids.size).toBe(6);
          expect(posts).toHaveLength(states.length * 3);
        },
      );
    }),
  60000,
);

it(
  "coordinates all Comfy cancellation acknowledgements across two real hosts and two restarts",
  async () =>
    workspace(async (root) => {
      const modes = [
        "cancelled",
        "failed",
        "dispatched",
        "false",
        "missing",
        "ambiguous",
        "wrong-id",
        "completed",
      ];
      const bytes = await imageBytes();
      let cancellations = 0,
        generations = 0;
      await server(
        (req, res) => {
          const mode = new URL(req.url!, "http://fixture").searchParams.get(
            "fixture",
          )!;
          if (req.method === "POST") {
            if (
              new URL(req.url!, "http://fixture").pathname.endsWith("/cancel")
            ) {
              cancellations++;
              if (mode === "ambiguous") res.destroy();
              else
                json(
                  res,
                  mode === "missing" ? {} : { cancelled: mode !== "false" },
                );
            } else {
              generations++;
              json(res, { prompt_id: "probe" });
            }
          } else if (req.url!.includes("/object_info"))
            json(res, {
              Text: { input: { required: { text: ["STRING"] } } },
              Save: {
                output_node: true,
                input: { required: { text: ["STRING"] } },
              },
            });
          else if (req.url!.includes("/system_stats"))
            json(res, { devices: [{ type: "cuda" }] });
          else if (req.url!.includes("/history/probe"))
            json(res, {
              probe: {
                status: { completed: true, status_str: "success" },
                outputs: {
                  save: {
                    images: [
                      { filename: "probe.png", subfolder: "", type: "output" },
                    ],
                  },
                },
              },
            });
          else if (req.url!.includes("/view")) {
            res.writeHead(200, { "Content-Type": "image/png" });
            res.end(bytes);
          } else if (req.url!.includes("/history/")) json(res, {});
          else
            json(res, {
              id: mode === "wrong-id" ? "other" : mode,
              status: ["cancelled", "failed"].includes(mode)
                ? mode
                : "in_progress",
            });
        },
        async (origin) => {
          const config = configuration(
            root,
            Object.fromEntries(
              modes.map((mode) => [
                mode,
                connection("comfyui", {
                  baseUrl: origin,
                  baseUrlMode: "origin",
                  defaultModel: "workflow",
                  requestTimeoutMs: 700,
                  workflow: {
                    graph: {
                      text: { class_type: "Text", inputs: { text: "" } },
                      save: { class_type: "Save", inputs: { text: "output" } },
                    },
                    bindings: { prompt: { node: "text", input: "text" } },
                    outputNodes: ["save"],
                  },
                }),
              ]),
            ),
          );
          config.maxConcurrentRequests = 32;
          // Native Comfy routes are origin-relative; the query key identifies each fixture connection.
          for (const mode of modes)
            config.connections[mode]!.query = { fixture: mode };
          await seedJobs(
            config,
            modes.map((mode) => ({
              id: mode,
              connection: mode,
              kind: "comfyui",
            })),
          );
          // Completed receipts model a result already committed before cancellation.
          const { Store } = await import("../../src/jobs/store.js");
          const store = new Store(config.stateDir);
          try {
            expect(store.claimFinalization("completed")).toBe(true);
            const done = store.get("completed");
            done.status = "completed";
            done.generation_outcome = "completed";
            store.update(done);
            store.release("completed", done);
          } finally {
            store.close();
          }
          for (let round = 0; round < 3; round++) {
            const pair = await startHosts(root, config);
            try {
              for (const mode of modes) {
                const result = await tool(pair.hosts[0]!.client, "cancel_job", {
                  request_id: mode,
                });
                if (!["ambiguous", "wrong-id"].includes(mode)) {
                  expect(result.isError).not.toBe(true);
                  expect(result.structuredContent).toMatchObject({
                    refund_verified: false,
                    upstream_cancelled:
                      mode === "cancelled"
                        ? true
                        : ["missing", "completed"].includes(mode) ||
                            (mode === "failed" && round > 0)
                          ? null
                          : false,
                  });
                } else expect(result.isError).toBe(true);
                for (const host of pair.hosts) {
                  const r = await job(host.client, mode, true);
                  expect(r.status).toBe(
                    ["cancelled", "failed", "completed"].includes(mode)
                      ? mode
                      : "running",
                  );
                }
                const before = generations;
                await tool(pair.hosts[1]!.client, "generate_image", {
                  connection: mode,
                  prompt: "capacity",
                  request_id: `capacity-${mode}-${round}`,
                });
                expect(generations - before).toBe(
                  ["cancelled", "failed", "completed"].includes(mode) ? 1 : 0,
                );
              }
            } finally {
              await pair.close();
            }
          }
          expect(generations).toBe(9);
          expect(cancellations).toBe(2 + 5 * 3);
        },
      );
    }),
  60000,
);
