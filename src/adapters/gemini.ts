import { commonFields } from "../capabilities.js";
import { fail } from "../errors.js";
import { jsonRequest, type AdapterInput } from "./types.js";
export function geminiRequest({
  connection: c,
  request: r,
  model,
  references,
  mask,
}: AdapterInput) {
  if (mask)
    fail(
      "unsupported_operation",
      "Native Gemini mask polarity is not implemented by this protocol.",
    );
  for (const key of commonFields)
    if (
      r[key] !== undefined &&
      !["aspect_ratio", "image_size", "output_format"].includes(key)
    )
      fail(
        "unsupported_parameter",
        `Native Gemini does not accept common ${key}.`,
      );
  if (c.adapter === "gemini") {
    if (r.output_format)
      fail(
        "unsupported_parameter",
        "generateContent does not guarantee requested output format.",
      );
    return jsonRequest(
      `models/${encodeURIComponent(model.replace(/^models\//, ""))}:generateContent`,
      "/v1beta",
      {
        contents: [
          {
            role: "user",
            parts: [
              { text: r.prompt },
              ...references.map((image) => ({
                inlineData: {
                  mimeType: image.mime,
                  data: image.bytes.toString("base64"),
                },
              })),
            ],
          },
        ],
        generationConfig: {
          responseModalities: ["TEXT", "IMAGE"],
          ...(r.aspect_ratio || r.image_size
            ? {
                imageConfig: {
                  ...(r.aspect_ratio ? { aspectRatio: r.aspect_ratio } : {}),
                  ...(r.image_size ? { imageSize: r.image_size } : {}),
                },
              }
            : {}),
          ...r.provider_options,
        },
      },
    );
  }
  return jsonRequest("interactions", "/v1beta", {
    model,
    store: false,
    input: [
      { type: "text", text: r.prompt },
      ...references.map((image) => ({
        type: "image",
        mime_type: image.mime,
        data: image.bytes.toString("base64"),
      })),
    ],
    response_format: {
      type: "image",
      ...(r.aspect_ratio ? { aspect_ratio: r.aspect_ratio } : {}),
      ...(r.image_size ? { image_size: r.image_size } : {}),
      ...(r.output_format ? { mime_type: `image/${r.output_format}` } : {}),
    },
    ...r.provider_options,
  });
}
