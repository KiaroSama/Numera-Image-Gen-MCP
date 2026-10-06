import type { Connection, ImageRequest } from "./config/schema.js";
import { fail } from "./errors.js";
import { isCodexModel } from "./gateway-model.js";
import { validateEditContract } from "./edit-contract.js";
import { validateImageDimensions } from "./services/image-dimensions.js";
export const commonFields = [
  "size",
  "aspect_ratio",
  "image_size",
  "quality",
  "output_format",
  "background",
  "seed",
  "negative_prompt",
] as const;
const reserved = new Set([
  "model",
  "prompt",
  "n",
  "count",
  "image",
  "images",
  "image_url",
  "image_urls",
  "input_references",
  "contents",
  "input",
  "messages",
  "tools",
  "mask",
  "url",
  "baseUrl",
  "headers",
  "auth",
  "authorization",
  "request_id",
  "__proto__",
  "prototype",
  "constructor",
]);
export function safeOptions(
  value: Record<string, unknown>,
  allowed?: string[],
  depth = 0,
): Record<string, unknown> {
  if (depth > 6 || Object.keys(value).length > 50)
    fail("invalid_input", "Provider options exceed structure limits.");
  for (const [key, v] of Object.entries(value)) {
    if (
      reserved.has(key) ||
      /token|secret|api.?key|password|upload|endpoint/i.test(key) ||
      (allowed && !allowed.includes(key))
    )
      fail("unsupported_parameter", `Provider option ${key} is not allowed.`);
    if (v && typeof v === "object") {
      if (Array.isArray(v)) {
        if (v.length > 32 || v.some((x) => x && typeof x === "object"))
          fail("invalid_input", "Nested option arrays are unsupported.");
      } else safeOptions(v as Record<string, unknown>, undefined, depth + 1);
    }
  }
  return value;
}
export function capabilities(connection: Connection, model: string) {
  const native = [
    "gemini",
    "gemini-interactions",
    "openai-responses",
    "openrouter-images",
    "chat-images",
  ].includes(connection.adapter);
  let refs = native ? "supported" : connection.edit ? "supported" : "unknown";
  let masks = connection.edit?.masks ? "supported" : "unsupported";
  const limitations: string[] = [];
  if (connection.gateway && model.startsWith("antigravity/")) {
    refs = "unsupported";
    limitations.push(
      "Examined gateway Antigravity route forwards text only; reference editing is disabled until explicitly verified by a corrected gateway version.",
    );
  }
  if (connection.gateway === "omniroute") {
    masks = "unsupported";
    if (isCodexModel(model)) refs = "supported";
  }
  if (connection.gateway === "9router") {
    masks = "unsupported";
    if (isCodexModel(model)) refs = "supported";
  }
  if (native) masks = "unsupported";
  if (connection.adapter === "comfyui") {
    refs = connection.workflow?.bindings.image ? "supported" : "unsupported";
    masks =
      connection.workflow?.bindings.mask && connection.edit?.maskPolarity
        ? "supported"
        : "unsupported";
  }
  const override = connection.modelOverrides[model];
  return {
    model: { text_to_image: "unknown", image_to_image: "unknown" },
    gateway_forwarding: {
      text_to_image: "supported",
      image_to_image: refs,
      masks,
      ...override?.capabilities,
    },
    account_availability: "unknown",
    max_references:
      override?.maxReferences ??
      (connection.gateway === "omniroute" && isCodexModel(model)
        ? 8
        : connection.gateway === "9router"
          ? 1
          : native
            ? 14
            : (connection.edit?.maxReferences ?? 1)),
    max_count: override?.maxCount ?? 1,
    parameters: override?.supportedParameters ?? null,
    evidence: [
      {
        kind: override ? "configured" : "documented",
        gateway_version: connection.gatewayVersion ?? null,
        source:
          connection.gateway === "9router"
            ? "https://github.com/decolua/9router"
            : connection.gateway === "omniroute"
              ? "https://github.com/diegosouzapw/OmniRoute"
              : null,
      },
    ],
    limitations,
  };
}
export function effectiveRequest(
  connection: Connection,
  request: ImageRequest,
  model: string,
): ImageRequest {
  const merged = {
    ...connection.defaults,
    ...connection.modelOverrides[model]?.defaults,
    ...request,
  };
  for (const key of commonFields)
    if (request[key] === undefined) {
      const value =
        connection.modelOverrides[model]?.defaults[key] ??
        connection.defaults[key];
      if (value !== undefined) (merged as Record<string, unknown>)[key] = value;
    }
  const options = safeOptions(
    { ...connection.extensions, ...request.provider_options },
    connection.providerOptionKeys,
  );
  for (const key of commonFields)
    if (merged[key] !== undefined && options[key] !== undefined)
      fail("invalid_input", `Conflicting common/provider parameter ${key}.`);
  merged.provider_options = options;
  if (merged.background === "transparent" && merged.output_format === "jpeg")
    fail("unsupported_parameter", "Transparent background cannot use JPEG.");
  if (
    (connection.allowedModels && !connection.allowedModels.includes(model)) ||
    connection.deniedModels.includes(model)
  )
    fail("model_unavailable", "Model is denied by connection policy.");
  validateEditContract(connection.edit, merged.reference_images.length);
  validateImageDimensions(connection, model, merged);
  const cap = capabilities(connection, model);
  if (
    request.count > 1 &&
    [
      "gemini",
      "gemini-interactions",
      "openai-responses",
      "chat-images",
    ].includes(connection.adapter)
  )
    fail(
      "unsupported_parameter",
      "This native adapter has no documented batch field; count must be 1.",
    );
  if (request.count > cap.max_count)
    fail(
      "unsupported_parameter",
      "Native batch count is not verified. Configure documented maxCount; multiple submissions are never automatic.",
    );
  if (request.reference_images.length > cap.max_references)
    fail("unsupported_operation", "Too many references for selected route.");
  if (
    request.reference_images.length &&
    cap.gateway_forwarding.image_to_image !== "supported"
  )
    fail(
      "unsupported_operation",
      "Reference input forwarding is not verified for this route.",
    );
  if (
    (request.mask || request.edit_region) &&
    cap.gateway_forwarding.masks !== "supported"
  )
    fail(
      "unsupported_operation",
      "Mask forwarding is unsupported for this route.",
    );
  if (
    (request.mask || request.edit_region) &&
    !connection.edit?.maskPolarity &&
    connection.adapter !== "comfyui"
  )
    fail(
      "unsupported_operation",
      "Configure documented mask polarity before submitting.",
    );
  if (
    connection.gateway === "omniroute" &&
    isCodexModel(model) &&
    request.count > 1
  )
    fail(
      "unsupported_parameter",
      "OmniRoute Codex count fans out paid requests; count must be 1.",
    );
  return merged;
}
