import { imagesRequest } from "./images.js";
import { geminiRequest } from "./gemini.js";
import { openaiRequest } from "./openai.js";
import { openrouterRequest } from "./openrouter.js";
import type { AdapterInput, Prepared } from "./types.js";
import { fail } from "../errors.js";
export { normalize, normalizeResponse } from "./normalize.js";
export function buildRequest(input: AdapterInput): Prepared {
  switch (input.connection.adapter) {
    case "openai-images":
      return imagesRequest(input);
    case "gemini":
    case "gemini-interactions":
      return geminiRequest(input);
    case "openai-responses":
    case "chat-images":
      return openaiRequest(input);
    case "openrouter-images":
      return openrouterRequest(input);
    case "comfyui":
      return fail(
        "invalid_configuration",
        "ComfyUI requires workflow execution, not a prompt-only request.",
      );
  }
}
