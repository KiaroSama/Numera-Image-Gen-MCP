import type { Config, Connection } from "../config/schema.js";

// Google AI for Developers, CC BY 4.0; checked 2026-10-03. Native pixel sizes are discrete, not calculated ratios.
const flashSizes: Record<string, string[]> = {
  "1:1": ["512x512", "1024x1024", "2048x2048", "4096x4096"],
  "1:4": ["256x1024", "512x2048", "1024x4096", "2048x8192"],
  "1:8": ["192x1536", "384x3072", "768x6144", "1536x12288"],
  "2:3": ["424x632", "848x1264", "1696x2528", "3392x5056"],
  "3:2": ["632x424", "1264x848", "2528x1696", "5056x3392"],
  "3:4": ["448x600", "896x1200", "1792x2400", "3584x4800"],
  "4:1": ["1024x256", "2048x512", "4096x1024", "8192x2048"],
  "4:3": ["600x448", "1200x896", "2400x1792", "4800x3584"],
  "4:5": ["464x576", "928x1152", "1856x2304", "3712x4608"],
  "5:4": ["576x464", "1152x928", "2304x1856", "4608x3712"],
  "8:1": ["1536x192", "3072x384", "6144x768", "12288x1536"],
  "9:16": ["384x688", "768x1376", "1536x2752", "3072x5504"],
  "16:9": ["688x384", "1376x768", "2752x1536", "5504x3072"],
  "21:9": ["792x168", "1584x672", "3168x1344", "6336x2688"],
};
const tiers = ["0.5K", "1K", "2K", "4K"];
const source =
  "https://ai.google.dev/gemini-api/docs/image-generation#aspect-ratios-and-image-size";

export function imageDimensions(
  connection: Connection,
  model: string,
  files: Config["files"],
) {
  const knownFlash = [
    "gemini-3.1-flash-image",
    "models/gemini-3.1-flash-image",
    "antigravity/gemini-3.1-flash-image",
  ].includes(model);
  const nativeGemini =
    connection.adapter === "gemini" ||
    connection.adapter === "gemini-interactions";
  const omni =
    connection.gateway === "omniroute" && model.startsWith("antigravity/");
  const requested = connection.modelOverrides[model]?.supportedParameters;
  const accepted = requested
    ? requested.filter((key) =>
        ["size", "aspect_ratio", "image_size"].includes(key),
      )
    : connection.adapter === "openrouter-images"
      ? ["size", "aspect_ratio", "image_size"]
      : nativeGemini || connection.adapter === "chat-images"
        ? ["aspect_ratio", "image_size"]
        : omni
          ? ["size", "aspect_ratio", "image_size"]
          : connection.adapter === "comfyui"
            ? []
            : ["size"];
  return {
    preset_aspect_ratios: Object.keys(flashSizes),
    presets_are_model_support: false,
    model_support: {
      status: knownFlash ? "documented" : "unknown",
      aspect_ratios: knownFlash ? Object.keys(flashSizes) : null,
      resolution_tiers: knownFlash ? tiers : null,
      pixel_sizes: knownFlash
        ? Object.entries(flashSizes).flatMap(([aspect_ratio, sizes]) =>
            sizes.map((size, i) => {
              const [width, height] = size.split("x").map(Number);
              return {
                aspect_ratio,
                resolution: tiers[i]!,
                width: width!,
                height: height!,
              };
            }),
          )
        : null,
      custom_dimensions: {
        status: knownFlash ? "unsupported" : "unknown",
        explanation: knownFlash
          ? "Select a documented aspect ratio and native tier; arbitrary pixel dimensions are not a native contract."
          : "No verified model-specific custom pixel-size contract is available.",
      },
      evidence: knownFlash
        ? [{ source, checked_at: "2026-10-03", kind: "official_documentation" }]
        : [],
    },
    route_support: {
      accepted_parameters: accepted,
      resolution_tiers: omni
        ? ["1K", "2K", "4K"]
        : knownFlash && nativeGemini
          ? tiers
          : null,
      max_pixels: files.maxPixels,
      custom_dimensions: "unknown",
      explanation: omni
        ? "Gateway size maps aspect ratio, not arbitrary pixels. 0.5K is not forwarded as a distinct tier. Use aspect_ratio/image_size."
        : "Accepted request fields do not establish model/account support. Generic Images does not accept undeclared aspect_ratio/image_size fields.",
    },
    account_verified: false,
    output_dimensions_must_be_verified: true,
  };
}
