import { it, expect } from "vitest";
import { Generation } from "../../src/services/generation.js";
import { Store } from "../../src/jobs/store.js";
import { Logger } from "../../src/logging.js";
import { getJob } from "../../src/services/jobs.js";
import {
  configuration,
  connection,
  workspace,
  server,
  json,
  imageBytes,
  requestBody,
} from "../fixtures/runtime.js";
it("keeps detached job recoverable and saves final output once after status lookup", async ({
  signal,
}) =>
  workspace(async (root) => {
    const bytes = await imageBytes();
    let posts = 0;
    await server(
      async (req, res) => {
        const path = new URL(req.url!, "http://fixture").pathname;
        if (path === "/object_info")
          json(res, {
            Text: { input: { required: { text: ["STRING"] } } },
            Save: {
              output_node: true,
              input: { required: { text: ["STRING"] } },
            },
          });
        else if (path === "/system_stats")
          json(res, { devices: [{ type: "cuda" }] });
        else if (path === "/prompt") {
          posts++;
          const body = JSON.parse((await requestBody(req)).toString("utf8"));
          expect(body.prompt.text.inputs.text).toBe("exact prompt");
          json(res, { prompt_id: "job" });
        } else if (path === "/history/job")
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
        else if (path === "/view") {
          res.writeHead(200, { "Content-Type": "image/png" });
          res.end(bytes);
        } else json(res, {}, 404);
      },
      async (origin) => {
        const c = connection("comfyui", {
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
        });
        const config = configuration(root, { local: c }),
          store = new Store(config.stateDir),
          log = new Logger(config.logging.directory, "ERROR"),
          generation = new Generation(config, store, log);
        try {
          const first = await generation.run(
            { prompt: "exact prompt", request_id: "detached", wait: false },
            "generate",
            signal,
          );
          expect(first.status).toBe("running");
          expect(posts).toBe(1);
          const done = await getJob(generation, "detached", true, signal);
          expect(done.status).toBe("completed");
          expect(done.outputs).toHaveLength(1);
          expect(
            (await getJob(generation, "detached", true, signal)).outputs,
          ).toHaveLength(1);
          expect(posts).toBe(1);
        } finally {
          store.close();
          log.close();
        }
      },
    );
  }));
