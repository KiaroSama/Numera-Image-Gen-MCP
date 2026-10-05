import type { Config, Connection, ImageRequest } from "../config/schema.js";
import { fail } from "../errors.js";

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
const proSizes = Object.fromEntries(
  Object.entries(flashSizes)
    .filter(([ratio]) => !["1:4", "1:8", "4:1", "8:1"].includes(ratio))
    .map(([ratio, sizes]) => [ratio, sizes.slice(1)]),
);
const source =
  "https://ai.google.dev/gemini-api/docs/image-generation#aspect-ratios-and-image-size";

export function imageDimensions(
  connection: Connection,
  model: string,
  files: Pick<Config["files"], "maxPixels">,
) {
  const knownFlash = [
    "gemini-3.1-flash-image",
    "models/gemini-3.1-flash-image",
    "antigravity/gemini-3.1-flash-image",
  ].includes(model);
  const knownPro = [
    "gemini-3-pro-image",
    "gemini-3-pro-image-preview",
    "models/gemini-3-pro-image",
    "models/gemini-3-pro-image-preview",
    "antigravity/gemini-3-pro-image",
    "antigravity/gemini-3-pro-image-preview",
  ].includes(model);
  const sizes = knownFlash ? flashSizes : knownPro ? proSizes : undefined;
  const modelTiers = knownPro ? tiers.slice(1) : tiers;
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
      status: sizes ? "documented" : "unknown",
      aspect_ratios: sizes ? Object.keys(sizes) : null,
      resolution_tiers: sizes ? modelTiers : null,
      pixel_sizes: sizes
        ? Object.entries(sizes).flatMap(([aspect_ratio, pixels]) =>
            pixels.map((size, i) => {
              const [width, height] = size.split("x").map(Number);
              return {
                aspect_ratio,
                resolution: modelTiers[i]!,
                width: width!,
                height: height!,
              };
            }),
          )
        : null,
      custom_dimensions: {
        status: sizes ? "unsupported" : "unknown",
        explanation: sizes
          ? "Select a documented aspect ratio and native tier; arbitrary pixel dimensions are not a native contract."
          : "No verified model-specific custom pixel-size contract is available.",
      },
      evidence: sizes
        ? [{ source, checked_at: "2026-10-05", kind: "official_documentation" }]
        : [],
    },
    route_support: {
      accepted_parameters: accepted,
      resolution_tiers: omni
        ? ["1K", "2K", "4K"]
        : sizes && nativeGemini
          ? modelTiers
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

export function dimensionEvidence(connection: Connection, model: string) {
  const dimensions = imageDimensions(connection, model, {
    maxPixels: 64000000,
  });
  const pixels = dimensions.model_support.pixel_sizes;
  const engine = model.replace(/^(?:cx|codex)\//, "");
  const gpt = [
    "gpt-image-2",
    "gpt-image-2.5-sunburst",
    "gpt-image-2.5-flare",
  ].includes(engine);
  return {
    kind: pixels ? "discrete" : gpt ? "custom" : "unknown",
    source: pixels
      ? source
      : gpt
        ? "https://developers.openai.com/api/docs/guides/image-generation#customize-image-output"
        : null,
    checked_at: pixels || gpt ? "2026-10-05" : null,
    max_edge: pixels
      ? Math.max(...pixels.flatMap((p) => [p.width, p.height]))
      : gpt
        ? 3840
        : null,
    max_pixels: pixels
      ? Math.max(...pixels.map((p) => p.width * p.height))
      : gpt
        ? 8294400
        : null,
    min_pixels: gpt ? 655360 : null,
    dimension_multiple: gpt ? 16 : null,
    pixel_sizes: pixels,
    account_verified: false,
    forwarding_guaranteed: false,
  };
}
export function validateImageDimensions(
  connection: Connection,
  model: string,
  request: ImageRequest,
) {
  const evidence = dimensionEvidence(connection, model);
  if (evidence.pixel_sizes) {
    if (
      request.aspect_ratio &&
      !evidence.pixel_sizes.some(
        (size) => size.aspect_ratio === request.aspect_ratio,
      )
    )
      fail(
        "unsupported_parameter",
        "Requested aspect ratio is outside the documented model presets.",
      );
    if (
      request.image_size &&
      !evidence.pixel_sizes.some(
        (size) => size.resolution === request.image_size,
      )
    )
      fail(
        "unsupported_parameter",
        "Requested resolution tier is outside the documented model presets.",
      );
  }
  if (
    !request.size ||
    !/^\d+x\d+$/.test(request.size) ||
    evidence.kind !== "custom"
  )
    return;
  const [width, height] = request.size.split("x").map(Number) as [
    number,
    number,
  ];
  if (
    width > evidence.max_edge! ||
    height > evidence.max_edge! ||
    width * height > evidence.max_pixels! ||
    width * height < evidence.min_pixels! ||
    width % 16 ||
    height % 16 ||
    width / height < 1 / 3 ||
    width / height > 3
  )
    fail(
      "unsupported_parameter",
      "Requested dimensions exceed the documented image-model limits; a gateway cannot guarantee larger native output.",
    );
}
