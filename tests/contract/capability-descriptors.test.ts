import { it, expect } from "vitest";
import { requestSchema } from "../../src/config/schema.js";
import { validateDescriptors } from "../../src/services/model-policy.js";
import { connection, server, json } from "../fixtures/runtime.js";

it("rejects endpoint unsupported/invalid parameters and pins only a compatible endpoint", async () => {
  await server(
    (req, res) =>
      json(res, {
        endpoints: [
          {
            provider_tag: "small",
            supported_parameters: {
              resolution: { type: "enum", values: ["1K"] },
            },
          },
          {
            provider_tag: "large",
            supported_parameters: {
              resolution: { type: "enum", values: ["2K"] },
              n: { type: "range", min: 1, max: 2 },
            },
          },
        ],
      }),
    async (origin) => {
      const c = connection("openrouter-images", {
        baseUrl: origin + "/api/v1",
      });
      const validated = await validateDescriptors(
        c,
        "family/model",
        requestSchema.parse({ prompt: "draw", image_size: "2K", count: 2 }),
      );
      expect(validated.request.provider_options).toMatchObject({
        provider: { only: ["large"], allow_fallbacks: false },
      });
      await expect(
        validateDescriptors(
          c,
          "family/model",
          requestSchema.parse({ prompt: "draw", quality: "high" }),
        ),
      ).rejects.toMatchObject({ code: "unsupported_parameter" });
    },
  );
});
it("catalog failure allows a selected known protocol model but emits an unknown warning", async () => {
  await server(
    (req, res) => json(res, { error: "fixture" }, 503),
    async (origin) => {
      const result = await validateDescriptors(
        connection("openrouter-images", { baseUrl: origin + "/api/v1" }),
        "chosen",
        requestSchema.parse({ prompt: "draw" }),
      );
      expect(result.warnings.join(" ")).toContain("unknown");
      expect(result.request.model).toBeUndefined();
    },
  );
});
