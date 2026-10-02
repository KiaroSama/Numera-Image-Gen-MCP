import { it, expect } from "vitest";
import { submitComfy, validateWorkflow } from "../../src/adapters/comfyui.js";
import { requestSchema } from "../../src/config/schema.js";
import { capabilities, effectiveRequest } from "../../src/capabilities.js";
import { connection, server, json } from "../fixtures/runtime.js";
it("surfaces rejected node IDs after successful partial enqueue and supports explicit mask forwarding evidence", async ({
  signal,
}) => {
  await server(
    (req, res) =>
      json(res, {
        prompt_id: "accepted",
        node_errors: { bad: { errors: ["private detail"] } },
      }),
    async (origin) => {
      const c = connection("comfyui", {
        baseUrl: origin,
        workflow: {
          graph: { text: { class_type: "Text", inputs: { text: "" } } },
          bindings: {
            prompt: { node: "text", input: "text" },
            image: { node: "text", input: "image" },
            mask: { node: "text", input: "mask" },
          },
          outputNodes: ["text"],
        },
        edit: {
          mode: "json",
          encoding: "image",
          masks: true,
          maskPolarity: "transparent-edit",
        },
      });
      const request = requestSchema.parse({ prompt: "draw" }),
        input = {
          connection: c,
          request,
          model: "workflow",
          operation: "generate" as const,
          references: [],
        };
      const result = await submitComfy(input, validateWorkflow(input), signal);
      expect(result.id).toBe("accepted");
      expect(result.warnings).toEqual([
        "ComfyUI accepted only valid workflow branches; rejected node IDs: bad",
      ]);
      expect(result.warnings.join(" ")).not.toContain("private detail");
      expect(capabilities(c, "workflow").gateway_forwarding.masks).toBe(
        "supported",
      );
      expect(
        effectiveRequest(
          c,
          requestSchema.parse({
            prompt: "edit",
            reference_images: [{ type: "path", path: "/input.png" }],
            mask: { type: "path", path: "/mask.png" },
          }),
          "workflow",
        ).mask,
      ).toBeDefined();
    },
  );
});
