import { it, expect } from "vitest";
import { Generation } from "../../src/services/generation.js";
import { Store } from "../../src/jobs/store.js";
import { Logger } from "../../src/logging.js";
import {
  workspace,
  configuration,
  connection,
  server,
  imageBytes,
  requestBody,
  json,
} from "../fixtures/runtime.js";

it(
  "requests a larger native edit and reports unchanged-size outputs as partial",
  () =>
    workspace(async (root) => {
      const original = await imageBytes(),
        larger = await imageBytes("png", true, 16, 12);
      let posts = 0;
      await server(
        async (req, res) => {
          posts++;
          const body = JSON.parse((await requestBody(req)).toString("utf8"));
          expect(req.url).toBe("/v1/images/edits");
          expect(body.image).toBe(
            `data:image/png;base64,${original.toString("base64")}`,
          );
          expect(body.prompt).toContain("Upscale the reference image to 16x12");
          expect(body.size).toBe("16x12");
          expect(body.upscale).toBeUndefined();
          json(res, {
            data: [
              {
                b64_json: (posts === 1 ? larger : original).toString("base64"),
              },
            ],
          });
        },
        async (origin) => {
          const config = configuration(root, {
            local: connection("openai-images", {
              baseUrl: `${origin}/v1`,
              defaultModel: "image-model",
              edit: { mode: "json", encoding: "image" },
            }),
          });
          const store = new Store(config.stateDir),
            logger = new Logger(config.logging.directory, "ERROR");
          try {
            const g = new Generation(config, store, logger);
            const input = {
              prompt: "Preserve the composition",
              upscale: true,
              size: "16x12",
              reference_images: [
                {
                  type: "data_url",
                  data_url: `data:image/png;base64,${original.toString("base64")}`,
                },
              ],
            };
            const first = await g.run(
              { ...input, request_id: "native-upscale" },
              "edit",
            );
            expect(first.status).toBe("completed");
            expect(first.outputs[0]).toMatchObject({ width: 16, height: 12 });
            const mismatch = await g.run(
              { ...input, request_id: "unchanged-output" },
              "edit",
            );
            expect(mismatch.status).toBe("partial");
            expect(mismatch.deviations).toContain(
              "Output 0 was not upscaled beyond the reference dimensions.",
            );
            for (const size of ["8x6", "4x3", "auto", "100000x100000"]) {
              await expect(
                g.run(
                  { ...input, size, request_id: `invalid-${size}` },
                  "edit",
                ),
              ).rejects.toMatchObject({ code: "invalid_input" });
            }
            expect(posts).toBe(2);
          } finally {
            store.close();
            logger.close();
          }
        },
      );
    }),
  10000,
);
