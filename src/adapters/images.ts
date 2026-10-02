import { FormData } from "undici";
import { fail } from "../errors.js";
import { commonFields } from "../capabilities.js";
import { jsonRequest, mimeData, type AdapterInput } from "./types.js";
export function imagesRequest(input: AdapterInput) {
  const {
    connection: c,
    request: r,
    model,
    operation,
    references,
    mask,
  } = input;
  const fields: Record<string, unknown> = {
    model,
    prompt: r.prompt,
    n: r.count,
    ...r.provider_options,
  };
  const allowed =
    c.modelOverrides[model]?.supportedParameters ??
    (c.gateway === "omniroute" && model.startsWith("antigravity/")
      ? ["aspect_ratio", "image_size", "size"]
      : c.gateway === "omniroute" && model.startsWith("codex/")
        ? ["size", "quality"]
        : c.gateway === "9router" && model.startsWith("antigravity/")
          ? ["size"]
          : c.gateway === "omniroute"
            ? ["size", "quality"]
            : ["size", "quality", "output_format", "background"]);
  for (const key of commonFields)
    if (r[key] !== undefined) {
      if (!allowed.includes(key))
        fail(
          "unsupported_parameter",
          `Parameter ${key} is unsupported by this route.`,
        );
      fields[key] = r[key];
    }
  if (
    c.gateway === "omniroute" &&
    model.startsWith("antigravity/") &&
    r.image_size &&
    !["1K", "2K", "4K"].includes(r.image_size)
  )
    fail(
      "unsupported_parameter",
      "OmniRoute Antigravity tier must be 1K, 2K or 4K; gateway clamping is not accepted.",
    );
  if (!references.length && operation === "generate")
    return jsonRequest("images/generations", "/v1", fields);
  if (!references.length)
    fail("invalid_input", "Editing requires source images.");
  if (c.gateway === "9router") {
    if (!model.startsWith("codex/") && !c.edit)
      fail(
        "unsupported_operation",
        "9router reference contract is not verified for this model.",
      );
    return jsonRequest("images/generations", "/v1", {
      ...fields,
      ...(references.length === 1
        ? { image: mimeData(references[0]!) }
        : { images: references.map(mimeData) }),
    });
  }
  const edit =
    c.edit ??
    (c.gateway === "omniroute"
      ? {
          mode: "multipart",
          encoding: "image",
          maxReferences: model.startsWith("codex/") ? 8 : 1,
          masks: false,
        }
      : {
          mode: "multipart",
          encoding: "image",
          maxReferences: 1,
          masks: false,
        });
  if (edit.mode === "multipart") {
    const form = new FormData();
    for (const [k, v] of Object.entries(fields))
      if (v !== undefined)
        form.append(k, typeof v === "string" ? v : JSON.stringify(v));
    for (const [i, image] of references.entries())
      form.append(
        edit.encoding === "image[]" ? "image[]" : "image",
        new Blob([new Uint8Array(image.bytes)], { type: image.mime }),
        `reference-${i}.${image.extension}`,
      );
    if (mask)
      form.append(
        "mask",
        new Blob([new Uint8Array(mask.bytes)], { type: mask.mime }),
        "mask.png",
      );
    return {
      operation: "images/edits",
      prefix: "/v1",
      body: form,
      headers: {},
    };
  }
  const encoded = references.map(mimeData);
  const ref =
    edit.encoding === "image"
      ? encoded[0]
      : edit.encoding === "input_references"
        ? encoded.map((url) => ({ type: "image_url", image_url: { url } }))
        : encoded;
  return jsonRequest(
    edit.mode === "generation" ? "images/generations" : "images/edits",
    "/v1",
    {
      ...fields,
      [edit.encoding]: ref,
      ...(mask ? { mask: mimeData(mask) } : {}),
    },
  );
}
