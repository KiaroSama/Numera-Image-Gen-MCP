import { it, expect } from "vitest";
import { validateWorkflow, submitComfy } from "../../src/adapters/comfyui.js";
import { imagesRequest } from "../../src/adapters/images.js";
import { capabilities, effectiveRequest } from "../../src/capabilities.js";
import { requestSchema } from "../../src/config/schema.js";
import { inspectImage } from "../../src/files/images.js";
import type { AdapterInput } from "../../src/adapters/types.js";
import {
  configuration,
  connection,
  workspace,
  server,
  json,
  imageBytes,
  requestBody,
} from "../fixtures/runtime.js";

async function input(root: string, origin: string): Promise<AdapterInput> {
  const config = configuration(root);
  return {
    connection: connection("comfyui", {
      baseUrl: origin,
      workflow: {
        graph: {
          "1": {
            class_type: "Fixture",
            inputs: { prompt: "", image: "", image2: "", mask: "" },
          },
        },
        bindings: {
          prompt: { node: "1", input: "prompt" },
          image: { node: "1", input: "image" },
          image_2: { node: "1", input: "image2" },
        },
        outputNodes: ["1"],
        requireGpu: false,
      },
    }),
    request: requestSchema.parse({ prompt: "fixture" }),
    references: [
      await inspectImage(await imageBytes("png"), config),
      await inspectImage(await imageBytes("jpeg", false), config),
    ],
    operation: "edit",
    model: "fixture",
  };
}

it.each([
  "second",
  "mask",
  "node",
  "slot",
  "collision",
  "changed-after-preflight",
])("rejects invalid Comfy %s binding before every upload", async (mode) =>
  workspace(async (root) => {
    let uploads = 0;
    await server(
      async (req, res) => {
        await requestBody(req);
        uploads++;
        json(res, { name: "fixture.png", subfolder: "", prompt_id: "owned" });
      },
      async (origin) => {
        const request = await input(root, origin);
        const workflow = request.connection.workflow!;
        const prepared =
          mode === "changed-after-preflight"
            ? validateWorkflow(request)
            : undefined;
        if (mode === "second" || mode === "changed-after-preflight")
          delete workflow.bindings.image_2;
        if (mode === "mask") request.mask = request.references[0];
        if (mode === "node")
          workflow.bindings.image_2 = { node: "missing", input: "image" };
        if (mode === "slot")
          workflow.bindings.image_2 = { node: "1", input: "imagge" };
        if (mode === "collision")
          workflow.bindings.image_2 = { ...workflow.bindings.image! };
        await expect(
          (async () => {
            await submitComfy(
              request,
              prepared ?? validateWorkflow(request),
              AbortSignal.timeout(2000),
            );
          })(),
        ).rejects.toThrow();
        expect(uploads).toBe(0);
      },
    );
  }),
);

it("preserves all accepted Comfy references and their distinct workflow targets", async () =>
  workspace(async (root) => {
    const uploads: Buffer[] = [];
    let prompt: Record<string, { inputs: Record<string, unknown> }> | undefined;
    await server(
      async (req, res) => {
        const body = await requestBody(req);
        if (req.url === "/upload/image") {
          uploads.push(body);
          json(res, {
            name: `reference-${uploads.length}`,
            subfolder: "owned",
          });
        } else {
          prompt = JSON.parse(body.toString("utf8")).prompt;
          json(res, { prompt_id: "owned", node_errors: {} });
        }
      },
      async (origin) => {
        const request = await input(root, origin);
        const result = await submitComfy(
          request,
          validateWorkflow(request),
          AbortSignal.timeout(2000),
        );
        expect(result.id).toBe("owned");
        expect(uploads).toHaveLength(2);
        expect(uploads[0]!.includes(request.references[0]!.bytes)).toBe(true);
        expect(uploads[1]!.includes(request.references[1]!.bytes)).toBe(true);
        expect(request.references[0]!.sha256).not.toBe(
          request.references[1]!.sha256,
        );
        expect(prompt?.["1"]?.inputs).toMatchObject({
          image: "owned/reference-1",
          image2: "owned/reference-2",
        });
      },
    );
  }));

it.each(["omniroute", "9router"] as const)(
  "treats Codex aliases equivalently without rewriting %s wire IDs",
  async (gateway) =>
    workspace(async (root) => {
      const image = await inspectImage(await imageBytes(), configuration(root));
      const c = connection("openai-images", { gateway });
      const request = requestSchema.parse({
        prompt: "fixture",
        reference_images: [
          {
            type: "data_url",
            data_url: "data:image/png;base64," + image.bytes.toString("base64"),
          },
        ],
      });
      for (const prefix of ["codex", "cx"]) {
        const model = `${prefix}/fixture`;
        expect(capabilities(c, model).gateway_forwarding.image_to_image).toBe(
          "supported",
        );
        const result = imagesRequest({
          connection: c,
          request: effectiveRequest(c, request, model),
          references: [image],
          model,
          operation: "edit",
        });
        if (typeof result.body === "string")
          expect(JSON.parse(result.body).model).toBe(model);
        else expect(result.body.get("model")).toBe(model);
      }
      expect(
        capabilities(c, "cxx/fixture").gateway_forwarding.image_to_image,
      ).toBe("unknown");
      if (gateway === "omniroute") {
        for (const prefix of ["codex", "cx"]) {
          const model = `${prefix}/fixture`;
          c.modelOverrides[model] = {
            defaults: {},
            capabilities: {},
            maxCount: 2,
          };
          expect(() =>
            effectiveRequest(
              c,
              requestSchema.parse({ prompt: "fixture", count: 2 }),
              model,
            ),
          ).toThrow("fans out paid requests");
        }
      }
    }),
);
