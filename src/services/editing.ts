import sharp from "sharp";
import type { Config, Connection, ImageRequest } from "../config/schema.js";
import type { Operation } from "../adapters/types.js";
import { inspectImage, type Image } from "../files/images.js";
import { fail } from "../errors.js";

export function editingRequest(
  request: ImageRequest,
  operation: Operation,
): ImageRequest {
  if (
    (request.edit_region || request.target_language) &&
    (operation !== "edit" || !request.reference_images.length)
  )
    fail(
      "invalid_input",
      "Region editing and in-image translation require edit_image with a reference image.",
    );
  if (request.edit_region && request.mask)
    fail("invalid_input", "Choose edit_region or an explicit mask, not both.");
  if (!request.target_language) return request;
  return {
    ...request,
    prompt: `${request.prompt}\n\nTranslate the text ${request.edit_region ? "inside the selected region" : "in the reference image"} into ${request.target_language}. Replace the original text inside the image; preserve panel layout, artwork, speech bubbles and text styling. Return the edited image, not a separate text translation.`,
  };
}

export async function regionMask(
  request: ImageRequest,
  source: Image,
  connection: Connection,
  config: Config,
): Promise<Image> {
  const region = request.edit_region!,
    polarity = connection.edit?.maskPolarity;
  if (!polarity)
    fail(
      "unsupported_operation",
      "Configure documented mask polarity before region editing.",
    );
  if (
    region.x + region.width > source.width ||
    region.y + region.height > source.height
  )
    fail(
      "invalid_input",
      "Edit region must fit within the first reference image dimensions.",
    );
  const length = source.width * source.height * 4;
  if (length > config.files.maxAggregateBytes)
    fail("invalid_input", "Region mask exceeds the working image byte limit.");
  const outside =
    polarity === "black-edit" ? 255 : polarity === "white-edit" ? 0 : 255;
  const inside = polarity === "black-edit" ? 0 : 255;
  const pixels = Buffer.alloc(length, outside);
  for (let i = 3; i < length; i += 4) pixels[i] = 255;
  for (let y = region.y; y < region.y + region.height; y++) {
    for (let x = region.x; x < region.x + region.width; x++) {
      const offset = (y * source.width + x) * 4;
      pixels.fill(inside, offset, offset + 3);
      pixels[offset + 3] = polarity === "transparent-edit" ? 0 : 255;
    }
  }
  const bytes = await sharp(pixels, {
    raw: { width: source.width, height: source.height, channels: 4 },
  })
    .png()
    .toBuffer();
  return inspectImage(bytes, config, config.files.maxInputBytes);
}
