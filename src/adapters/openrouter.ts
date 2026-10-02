import { commonFields } from "../capabilities.js";
import { jsonRequest, mimeData, type AdapterInput } from "./types.js";
import { fail } from "../errors.js";
export function openrouterRequest({
  request: r,
  model,
  references,
  mask,
}: AdapterInput) {
  if (mask)
    fail(
      "unsupported_operation",
      "Dedicated OpenRouter image API does not define a universal mask contract.",
    );
  const fields: Record<string, unknown> = {
    model,
    prompt: r.prompt,
    n: r.count,
    input_references: references.map((image) => ({
      type: "image_url",
      image_url: { url: mimeData(image) },
    })),
    provider: { allow_fallbacks: false },
  };
  for (const key of commonFields)
    if (r[key] !== undefined) {
      if (key === "negative_prompt")
        fail(
          "unsupported_parameter",
          "Use explicitly documented provider passthrough for negative prompt.",
        );
      fields[key === "image_size" ? "resolution" : key] = r[key];
    }
  for (const [k, v] of Object.entries(r.provider_options)) fields[k] = v;
  if (fields.provider && typeof fields.provider === "object")
    (fields.provider as Record<string, unknown>).allow_fallbacks = false;
  return jsonRequest("images", "/api/v1", fields);
}
