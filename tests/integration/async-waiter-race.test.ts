import { expect, it } from "vitest";
import { Store } from "../../src/jobs/store.js";
import { Generation } from "../../src/services/generation.js";
import { getJob } from "../../src/services/jobs.js";
import { Logger } from "../../src/logging.js";
import {
  configuration,
  connection,
  imageBytes,
  json,
  server,
  workspace,
} from "../fixtures/runtime.js";

it("fences the original Responses waiter when a refresher completes the existing job", async ({
  signal,
}) =>
  workspace(async (root) => {
    const bytes = await imageBytes();
    let posts = 0,
      polls = 0,
      release: (() => void) | undefined;
    let ready!: () => void;
    const waiting = new Promise<void>((resolve) => {
      ready = resolve;
    });
    const bounded = AbortSignal.any([signal, AbortSignal.timeout(3000)]);
    await server(
      (request, response) => {
        if (request.method === "POST") {
          posts++;
          json(response, { id: "owned", status: "in_progress" });
        } else {
          polls++;
          const reply = () =>
            json(response, {
              id: "owned",
              status: "completed",
              output: [
                {
                  type: "image_generation_call",
                  status: "completed",
                  result: bytes.toString("base64"),
                },
              ],
            });
          if (polls === 1) {
            let replied = false;
            release = () => {
              if (!replied) {
                replied = true;
                reply();
              }
            };
            ready();
          } else reply();
        }
      },
      async (origin) => {
        const config = configuration(root, {
          local: connection("openai-responses", {
            baseUrl: `${origin}/v1`,
            orchestrationModel: "text",
            defaultModel: "image",
          }),
        });
        const store = new Store(config.stateDir),
          logger = new Logger(config.logging.directory, "ERROR");
        const generation = new Generation(config, store, logger);
        let original: ReturnType<Generation["run"]> | undefined;
        try {
          original = generation.run(
            { prompt: "draw", request_id: "responses-race" },
            "generate",
            bounded,
          );
          void original.catch(ready);
          await new Promise<void>((resolve, reject) => {
            const abort = () => reject(bounded.reason);
            bounded.addEventListener("abort", abort, { once: true });
            void waiting.then(() => {
              bounded.removeEventListener("abort", abort);
              resolve();
            });
          });
          expect(release).toBeDefined();
          const recovered = await getJob(
            generation,
            "responses-race",
            true,
            bounded,
          );
          release!();
          await original;
          expect(recovered.status).toBe("completed");
          expect(store.get("responses-race").outputs).toHaveLength(1);
          expect(store.listOutputs()).toHaveLength(1);
          expect(posts).toBe(1);
          expect(polls).toBe(2);
        } finally {
          release?.();
          await original?.catch(() => {});
          store.close();
          logger.close();
        }
      },
    );
  }));
