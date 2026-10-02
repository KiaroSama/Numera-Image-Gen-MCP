import type { Connection, ImageRequest } from "../config/schema.js";
import { commonFields } from "../capabilities.js";
import { apiJson } from "../http/client.js";
import { record, array, fail } from "../errors.js";

function accepts(descriptor: unknown, value: unknown): boolean {
  if (!descriptor || typeof descriptor !== "object") return false;
  const d = record(descriptor);
  if (d.type === "enum") return array(d.values).includes(value);
  if (d.type === "range")
    return (
      typeof value === "number" &&
      Number.isInteger(value) &&
      value >= Number(d.min) &&
      value <= Number(d.max)
    );
  return d.type === "boolean";
}
export async function validateDescriptors(
  c: Connection,
  model: string,
  request: ImageRequest,
  signal?: AbortSignal,
): Promise<{ request: ImageRequest; warnings: string[] }> {
  if (c.adapter !== "openrouter-images")
    return {
      request,
      warnings: c.modelOverrides[model]?.evidence
        ? []
        : [
            "Model/account parameter support is unknown unless documented in configured capability evidence.",
          ],
    };
  for (const key of Object.keys(request.provider_options))
    if (!["provider", "stream", "output_compression"].includes(key))
      fail(
        "unsupported_parameter",
        "Dedicated Image API requires provider-specific settings under provider.options.",
      );
  let raw: Record<string, unknown>;
  try {
    raw = record(
      await apiJson(
        c,
        `images/models/${model.split("/").map(encodeURIComponent).join("/")}/endpoints`,
        "/api/v1",
        {},
        signal,
      ),
    );
  } catch {
    return {
      request,
      warnings: [
        "Endpoint capability discovery failed; explicit model availability and parameter forwarding remain unknown. No generation fallback will occur.",
      ],
    };
  }
  const endpoints = array(raw.endpoints).map(record);
  if (!endpoints.length)
    return {
      request,
      warnings: ["Endpoint catalog is empty; model support is unknown."],
    };
  const requirements: Record<string, unknown> = {};
  for (const key of commonFields)
    if (request[key] !== undefined)
      requirements[key === "image_size" ? "resolution" : key] = request[key];
  if (request.count > 1) requirements.n = request.count;
  if (request.reference_images.length)
    requirements.input_references = request.reference_images.length;
  for (const key of ["stream", "output_compression"])
    if (request.provider_options[key] !== undefined)
      requirements[key] = request.provider_options[key];
  const routing = request.provider_options.provider
    ? record(request.provider_options.provider)
    : {};
  const selected = array(routing.only).map(String);
  const compatible = endpoints.filter((e) => {
    const tag = String(e.provider_tag ?? e.provider_slug ?? "");
    if (selected.length && !selected.includes(tag)) return false;
    const params = record(e.supported_parameters ?? {});
    return Object.entries(requirements).every(([key, value]) =>
      accepts(params[key], value),
    );
  });
  if (!compatible.length)
    fail(
      "unsupported_parameter",
      "No discovered endpoint supports every explicit image requirement; nothing was submitted.",
    );
  const tag = compatible[0]!.provider_tag ?? compatible[0]!.provider_slug;
  if (tag === null || tag === undefined) {
    if (compatible.length !== endpoints.length)
      fail(
        "capability_unknown",
        "Cannot pin a compatible endpoint from this catalog.",
      );
    return { request, warnings: [] };
  }
  const endpoint = compatible[0]!,
    passthrough = record(routing.options ?? {});
  for (const [provider, options] of Object.entries(passthrough)) {
    if (provider !== endpoint.provider_slug)
      fail(
        "unsupported_parameter",
        "Passthrough options name a different provider than selected endpoint.",
      );
    const allowed = array(endpoint.allowed_passthrough_parameters);
    if (Object.keys(record(options)).some((key) => !allowed.includes(key)))
      fail(
        "unsupported_parameter",
        "Endpoint does not allow an explicit passthrough parameter.",
      );
  }
  return {
    request: {
      ...request,
      provider_options: {
        ...request.provider_options,
        provider: { ...routing, only: [String(tag)], allow_fallbacks: false },
      },
    },
    warnings: [],
  };
}
