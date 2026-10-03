import { expect, it } from "vitest";
import sharp from "sharp";
import { Generation } from "../../src/services/generation.js";
import { Store } from "../../src/jobs/store.js";
import { Logger } from "../../src/logging.js";
import {
  workspace,
  server,
  configuration,
  connection,
  imageBytes,
  requestBody,
  json,
} from "../fixtures/runtime.js";

it.each(["transparent-edit", "white-edit", "black-edit"] as const)(
  "edits a comic region with %s and translates inside the image without changing source bytes",
  async (polarity) =>
    workspace(async (root) => {
      const source = await imageBytes();
      let posts = 0;
      await server(
        async (req, res) => {
          posts++;
          expect(req.url).toBe("/v1/images/edits");
          const body = JSON.parse((await requestBody(req)).toString("utf8"));
          expect(body.image).toBe(
            `data:image/png;base64,${source.toString("base64")}`,
          );
          expect(body.prompt).toContain("Preserve the comic panels 日本語");
          expect(body.prompt).toContain(
            "Translate the text inside the selected region into Persian",
          );
          const mask = Buffer.from(body.mask.split(",")[1], "base64");
          const { data, info } = await sharp(mask)
            .raw()
            .toBuffer({ resolveWithObject: true });
          expect([info.width, info.height, info.channels]).toEqual([8, 6, 4]);
          expect(data[3]).toBe(255);
          const alpha = polarity === "transparent-edit" ? 0 : 255;
          expect(data[(1 * 8 + 2) * 4 + 3]).toBe(alpha);
          expect(data[(2 * 8 + 4) * 4 + 3]).toBe(alpha);
          expect(data[(2 * 8 + 5) * 4 + 3]).toBe(255);
          if (polarity !== "transparent-edit") {
            expect(data[0]).toBe(polarity === "white-edit" ? 0 : 255);
            expect(data[(1 * 8 + 2) * 4]).toBe(
              polarity === "white-edit" ? 255 : 0,
            );
          }
          json(res, { data: [{ b64_json: source.toString("base64") }] });
        },
        async (origin) => {
          const config = configuration(root, {
            local: connection("openai-images", {
              baseUrl: `${origin}/v1`,
              defaultModel: "comic-model",
              edit: {
                mode: "json",
                encoding: "image",
                masks: true,
                maskPolarity: polarity,
              },
            }),
          });
          const store = new Store(config.stateDir),
            logger = new Logger(config.logging.directory);
          try {
            const generation = new Generation(config, store, logger);
            const input = {
              prompt: "Preserve the comic panels 日本語",
              request_id: "comic-region",
              target_language: "Persian",
              reference_images: [
                {
                  type: "data_url",
                  data_url: `data:image/png;base64,${source.toString("base64")}`,
                },
              ],
              edit_region: { x: 2, y: 1, width: 3, height: 2 },
            };
            const result = await generation.run(input, "edit");
            expect(result.status).toBe("completed");
            expect(result.outputs[0]).toMatchObject({ width: 8, height: 6 });
            await expect(
              generation.run(
                {
                  ...input,
                  request_id: "outside",
                  edit_region: { x: 7, y: 0, width: 2, height: 1 },
                },
                "edit",
              ),
            ).rejects.toMatchObject({ code: "invalid_input" });
            await expect(
              generation.run(
                {
                  ...input,
                  request_id: "conflict",
                  mask: input.reference_images[0],
                },
                "edit",
              ),
            ).rejects.toMatchObject({ code: "invalid_input" });
            await expect(
              generation.run(
                { ...input, request_id: "generate-region" },
                "generate",
              ),
            ).rejects.toMatchObject({ code: "invalid_input" });
            expect(posts).toBe(1);
          } finally {
            store.close();
            logger.close();
          }
        },
      );
    }),
  10000,
);
