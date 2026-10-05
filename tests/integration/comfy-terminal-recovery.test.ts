import { it, expect } from "vitest";
import { Generation } from "../../src/services/generation.js";
import { getJob } from "../../src/services/jobs.js";
import { Store } from "../../src/jobs/store.js";
import { Logger } from "../../src/logging.js";
import {
  configuration,
  connection,
  workspace,
  server,
  json,
  imageBytes,
} from "../fixtures/runtime.js";

it("recovers completed Comfy originals after the first download fails without another submission", async ({
  signal,
}) =>
  workspace(async (root) => {
    const bytes = await imageBytes();
    let posts = 0,
      downloads = 0;
    await server(
      (req, res) => {
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
          json(res, { prompt_id: "job" });
        } else if (req.url === "/history/job")
          json(res, {
            job: {
              status: { completed: true, status_str: "success" },
              outputs: {
                save: {
                  images: [
                    { filename: "final.png", subfolder: "", type: "output" },
                  ],
                },
              },
            },
          });
        else if (req.url?.startsWith("/view")) {
          downloads++;
          if (downloads === 1) json(res, {}, 500);
          else {
            res.writeHead(200, { "Content-Type": "image/png" });
            res.end(bytes);
          }
        } else json(res, {}, 404);
      },
      async (origin) => {
        const config = configuration(root, {
          local: connection("comfyui", {
            baseUrl: origin,
            baseUrlMode: "origin",
            defaultModel: "workflow",
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
        const store = new Store(config.stateDir),
          logger = new Logger(config.logging.directory, "ERROR"),
          generation = new Generation(config, store, logger);
        try {
          const first = await generation.run(
            { prompt: "fixture", request_id: "download-failure" },
            "generate",
            signal,
          );
          expect(first).toMatchObject({
            status: "running",
            generation_outcome: "completed",
            outputs: [],
          });
          const done = await getJob(
            generation,
            "download-failure",
            true,
            signal,
          );
          expect(done.outputs).toHaveLength(1);
          expect(done.generation_outcome).toBe("completed");
          expect(posts).toBe(1);
          expect(downloads).toBe(2);
        } finally {
          store.close();
          logger.close();
        }
      },
    );
  }));
