import { fail } from "../errors.js";
import { commonFields } from "../capabilities.js";
import { jsonRequest, mimeData, type AdapterInput } from "./types.js";
export function openaiRequest({
  connection: c,
  request: r,
  model,
  references,
  mask,
  operation,
}: AdapterInput) {
  if (mask)
    fail("unsupported_operation", "Use a documented multipart mask route.");
  if (c.adapter === "chat-images") {
    if (!c.chatImageOutput)
      fail(
        "unsupported_operation",
        "Chat image output requires explicit opt-in.",
      );
    for (const key of commonFields)
      if (r[key] !== undefined && !["aspect_ratio", "image_size"].includes(key))
        fail(
          "unsupported_parameter",
          `Chat image adapter does not map ${key}.`,
        );
    return jsonRequest("chat/completions", "/v1", {
      model,
      messages: [
        {
          role: "user",
          content: [
            { type: "text", text: r.prompt },
            ...references.map((image) => ({
              type: "image_url",
              image_url: { url: mimeData(image) },
            })),
          ],
        },
      ],
      modalities: ["image", "text"],
      image_config: {
        ...(r.aspect_ratio ? { aspect_ratio: r.aspect_ratio } : {}),
        ...(r.image_size ? { image_size: r.image_size } : {}),
      },
      ...r.provider_options,
    });
  }
  if (!c.orchestrationModel)
    fail(
      "invalid_configuration",
      "Responses requires a separate orchestrationModel.",
    );
  const tool: Record<string, unknown> = {
    type: "image_generation",
    model,
    action: operation === "edit" ? "edit" : "generate",
  };
  for (const key of commonFields)
    if (r[key] !== undefined) {
      if (!["size", "quality", "output_format", "background"].includes(key))
        fail(
          "unsupported_parameter",
          `Responses image tool does not accept ${key}.`,
        );
      tool[key] = r[key];
    }
  return jsonRequest("responses", "/v1", {
    model: c.orchestrationModel,
    store: false,
    input: [
      {
        role: "user",
        content: [
          { type: "input_text", text: r.prompt },
          ...references.map((image) => ({
            type: "input_image",
            image_url: mimeData(image),
          })),
        ],
      },
    ],
    tools: [tool],
    tool_choice: { type: "image_generation" },
    ...r.provider_options,
  });
}
