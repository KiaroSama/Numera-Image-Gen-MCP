import { expect, it } from "vitest";
import {
  configuration,
  connection,
  imageBytes,
  json,
  receipt,
  server,
  workspace,
} from "../fixtures/runtime.js";
import { Store } from "../../src/jobs/store.js";
import { Generation } from "../../src/services/generation.js";
import { getJob } from "../../src/services/jobs.js";
import { connectionIdentity } from "../../src/services/connection-identity.js";
import { Logger } from "../../src/logging.js";
import { readFile } from "node:fs/promises";

it("retains the first completed Comfy image when the second download fails", async ({
  signal,
}) =>
  workspace(async (root) => {
    const bytes = await imageBytes();
    let posts = 0,
      downloads = 0;
    await server(
      (request, response) => {
        if (request.method === "POST") posts++;
        const url = new URL(request.url!, "http://fixture");
        if (url.pathname === "/history/owned")
          json(response, {
            owned: {
              status: { completed: true },
              outputs: {
                save: {
                  images: [
                    { filename: "one.png", type: "output" },
                    { filename: "two.png", type: "output" },
                  ],
                },
              },
            },
          });
        else if (url.pathname === "/view") {
          downloads++;
          if (url.searchParams.get("filename") === "two.png")
            json(response, {}, 500);
          else {
            response.writeHead(200, { "Content-Type": "image/png" });
            response.end(bytes);
          }
        } else json(response, {}, 404);
      },
      async (origin) => {
        const c = connection("comfyui", {
          baseUrl: origin,
          baseUrlMode: "origin",
          workflow: {
            graph: { save: { class_type: "Save", inputs: {} } },
            bindings: {},
            outputNodes: ["save"],
          },
        });
        const config = configuration(root, { local: c }),
          store = new Store(config.stateDir),
          logger = new Logger(config.logging.directory, "ERROR");
        try {
          const initial = {
            ...receipt("comfy-partial"),
            status: "running",
            generation_outcome: "running",
            upstream_job: { id: "owned", kind: "comfyui" },
            output_requirements: { count: 2 },
          };
          store.prepare(initial, "hash", await connectionIdentity("local", c));
          store.release(initial.request_id);
          const first = await getJob(
            new Generation(config, store, logger),
            initial.request_id,
            true,
            signal,
          );
          expect(first.outputs).toEqual([]);
          expect(store.pendingResults(initial.request_id)).toHaveLength(1);
          expect(store.pendingResults(initial.request_id)[0]!.bytes).toEqual(
            bytes,
          );
          const recovered = await getJob(
            new Generation({ ...config, connections: {} }, store, logger),
            initial.request_id,
            true,
            signal,
          );
          expect(recovered.status).toBe("partial");
          expect(recovered.outputs).toHaveLength(1);
          expect(await readFile(recovered.outputs[0]!.path)).toEqual(bytes);
          expect(downloads).toBe(2);
          expect(posts).toBe(0);
        } finally {
          store.close();
          logger.close();
        }
      },
    );
  }));
