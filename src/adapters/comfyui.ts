import { FormData } from "undici";
import { randomUUID } from "node:crypto";
import { setTimeout as delay } from "node:timers/promises";
import { apiJson, apiRequest } from "../http/client.js";
import { array, record, fail } from "../errors.js";
import type { AdapterInput, Normalized } from "./types.js";
import type { Connection } from "../config/schema.js";
import { validateGraph } from "./workflow-validation.js";
export function validateWorkflow(input: AdapterInput) {
  const workflow = input.connection.workflow;
  if (!workflow)
    fail(
      "invalid_configuration",
      "Configure an API-format ComfyUI workflow and explicit bindings.",
    );
  const graph = structuredClone(workflow.graph);
  if (!Object.keys(graph).length || Object.keys(graph).length > 300)
    fail("invalid_configuration", "Invalid bounded ComfyUI graph.");
  for (const [id, value] of Object.entries(graph)) {
    const node = record(value);
    if (typeof node.class_type !== "string" || !node.inputs)
      fail("invalid_configuration", `Node ${id} is not API format.`);
    record(node.inputs);
  }
  const bind = (key: string, value: unknown, required = false) => {
    const target = workflow.bindings[key];
    if (!target) {
      if (required)
        fail(
          "unsupported_parameter",
          `ComfyUI workflow has no ${key} binding.`,
        );
      return;
    }
    const node = graph[target.node];
    if (!node)
      fail(
        "invalid_configuration",
        `Workflow binding ${key} references an absent node.`,
      );
    record(record(node).inputs)[target.input] = value;
  };
  bind("prompt", input.request.prompt, true);
  for (const key of [
    "seed",
    "negative_prompt",
    "size",
    "aspect_ratio",
    "image_size",
    "quality",
    "output_format",
    "background",
  ] as const)
    if (input.request[key] !== undefined) bind(key, input.request[key], true);
  if (input.request.count > 1) bind("count", input.request.count, true);
  for (const [key, value] of Object.entries(input.request.provider_options))
    bind(key, value, true);
  for (const node of workflow.outputNodes)
    if (!graph[node])
      fail(
        "invalid_configuration",
        "Output binding references an absent node.",
      );
  return { graph, workflow, bind };
}
export async function prepareComfy(input: AdapterInput, signal: AbortSignal) {
  const prepared = validateWorkflow(input),
    c = input.connection;
  const info = record(await apiJson(c, "object_info", "", {}, signal));
  validateGraph(prepared.graph, prepared.workflow, info);
  for (const value of Object.values(prepared.graph)) {
    const node = record(value);
    if (!info[String(node.class_type)])
      fail(
        "unsupported_operation",
        "Workflow requires a node not installed on this backend.",
      );
  }
  if (prepared.workflow.requireGpu) {
    const stats = record(await apiJson(c, "system_stats", "", {}, signal));
    const devices = array(stats.devices);
    if (!devices.length || record(devices[0]).type === "cpu")
      fail(
        "unsupported_operation",
        "Configured ComfyUI reports no primary GPU; CPU inference is not authorized.",
      );
  }
  return prepared;
}
export async function submitComfy(
  input: AdapterInput,
  prepared: Awaited<ReturnType<typeof prepareComfy>>,
  signal: AbortSignal,
): Promise<{ id: string; kind: string; warnings: string[] }> {
  const c = input.connection;
  for (const [index, image] of input.references.entries()) {
    const binding = index === 0 ? "image" : `image_${index + 1}`;
    if (!prepared.workflow.bindings[binding])
      fail(
        "unsupported_operation",
        `Workflow lacks ${binding} reference binding.`,
      );
    const form = new FormData();
    form.append(
      "image",
      new Blob([new Uint8Array(image.bytes)], { type: image.mime }),
      `${randomUUID()}.${image.extension}`,
    );
    form.append("type", "input");
    const upload = record(
      await apiJson(
        c,
        "upload/image",
        "",
        { method: "POST", body: form },
        signal,
      ),
    );
    if (typeof upload.name !== "string" || typeof upload.subfolder !== "string")
      fail(
        "invalid_response",
        "ComfyUI upload returned invalid file identity.",
        "upload",
      );
    prepared.bind(
      binding,
      (upload.subfolder ? upload.subfolder + "/" : "") + upload.name,
      true,
    );
  }
  if (input.mask) {
    if (!prepared.workflow.bindings.mask)
      fail("unsupported_operation", "Workflow has no explicit mask input.");
    const form = new FormData();
    form.append(
      "image",
      new Blob([new Uint8Array(input.mask.bytes)], { type: input.mask.mime }),
      `${randomUUID()}.png`,
    );
    const upload = record(
      await apiJson(
        c,
        "upload/image",
        "",
        { method: "POST", body: form },
        signal,
      ),
    );
    if (typeof upload.name !== "string" || typeof upload.subfolder !== "string")
      fail(
        "invalid_response",
        "ComfyUI mask upload returned invalid file identity.",
        "upload",
      );
    prepared.bind(
      "mask",
      (upload.subfolder ? upload.subfolder + "/" : "") + upload.name,
      true,
    );
  }
  const result = record(
    await apiJson(
      c,
      "prompt",
      "",
      {
        method: "POST",
        body: JSON.stringify({
          prompt: prepared.graph,
          client_id: randomUUID(),
        }),
        headers: { "Content-Type": "application/json" },
      },
      signal,
    ),
  );
  if (typeof result.prompt_id !== "string" || !result.prompt_id)
    fail(
      "outcome_unknown",
      "ComfyUI submission has no recoverable prompt ID.",
      "submission",
    );
  return {
    id: result.prompt_id,
    kind: "comfyui",
    warnings: Object.keys(record(result.node_errors ?? {})).length
      ? [
          "ComfyUI accepted only valid workflow branches; rejected node IDs: " +
            Object.keys(record(result.node_errors)).join(", "),
        ]
      : [],
  };
}
export async function comfyStatus(
  c: Connection,
  id: string,
  signal: AbortSignal,
): Promise<Normalized | undefined> {
  const history = record(
    await apiJson(c, `history/${encodeURIComponent(id)}`, "", {}, signal),
  );
  if (!history[id]) return;
  const entry = record(history[id]),
    status = record(entry.status);
  if (status.status_str === "error")
    fail("provider_rejection", "ComfyUI workflow failed.", "generation");
  if (status.completed !== true) return;
  const images: Normalized["images"] = [];
  for (const node of c.workflow!.outputNodes) {
    const output = record(record(entry.outputs)[node] ?? {});
    for (const item of array(output.images)) {
      const file = record(item);
      if (
        typeof file.filename !== "string" ||
        !["output", "temp"].includes(String(file.type))
      )
        continue;
      const response = await apiRequest(
        c,
        "view",
        "",
        {
          query: {
            filename: file.filename,
            subfolder: String(file.subfolder ?? ""),
            type: String(file.type),
          },
        },
        signal,
      );
      images.push({ bytes: response.bytes });
    }
  }
  if (!images.length)
    fail(
      "no_image_returned",
      "ComfyUI finished without owned final output images.",
      "response",
    );
  return { images, upstreamModel: null, upstreamId: id, warnings: [] };
}
export async function waitComfy(
  c: Connection,
  id: string,
  signal: AbortSignal,
) {
  while (!signal.aborted) {
    const result = await comfyStatus(c, id, signal);
    if (result) return result;
    await delay(500, undefined, { signal });
  }
  signal.throwIfAborted();
  throw new Error("Aborted");
}
