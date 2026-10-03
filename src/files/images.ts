import sharp from "sharp";
import { createHash } from "node:crypto";
import { fail } from "../errors.js";
import type { Config } from "../config/schema.js";
export type Image = {
  bytes: Buffer;
  mime: string;
  width: number;
  height: number;
  alpha: boolean;
  sha256: string;
  extension: string;
};
export function decodeBase64(value: string, limit: number): Buffer {
  if (
    !value ||
    value.length > Math.ceil(limit / 3) * 4 + 4 ||
    value.length % 4 !== 0 ||
    !/^[A-Za-z0-9+/]+={0,2}$/.test(value)
  )
    fail("invalid_response", "Invalid or excessive Base64 image.", "response");
  const bytes = Buffer.from(value, "base64");
  if (!bytes.length || bytes.length > limit)
    fail("invalid_response", "Empty or excessive image bytes.", "response");
  return bytes;
}
export function dataUrl(value: string, limit: number): Buffer {
  const match = /^data:image\/(png|jpeg|webp);base64,([A-Za-z0-9+/=]+)$/.exec(
    value,
  );
  if (!match)
    fail("invalid_input", "Expected a PNG/JPEG/WebP Base64 data URL.");
  return decodeBase64(match[2]!, limit);
}
export async function inspectImage(
  bytes: Buffer,
  config: Config,
  limit = config.files.maxOutputBytes,
): Promise<Image> {
  if (!bytes.length || bytes.length > limit)
    fail("invalid_input", "Image byte limit exceeded or empty image.");
  const signature = bytes.subarray(0, 12);
  const format = signature
    .subarray(0, 8)
    .equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]))
    ? "png"
    : signature[0] === 255 && signature[1] === 216 && signature[2] === 255
      ? "jpeg"
      : signature.toString("ascii", 0, 4) === "RIFF" &&
          signature.toString("ascii", 8, 12) === "WEBP"
        ? "webp"
        : undefined;
  if (!format)
    fail("invalid_input", "Only valid PNG, JPEG and WebP bytes are supported.");
  try {
    const image = sharp(bytes, {
      limitInputPixels: config.files.maxPixels,
      failOn: "error",
      animated: true,
    });
    const info = await image.metadata();
    if (
      !info.width ||
      !info.height ||
      (info.pages ?? 1) > 1 ||
      info.width * info.height > config.files.maxPixels
    )
      fail(
        "invalid_input",
        "Animated, excessive or ambiguous image is unsupported.",
      );
    await image.clone().resize(1, 1).raw().toBuffer();
    return {
      bytes,
      mime: `image/${format}`,
      width: info.width,
      height: info.height,
      alpha: info.hasAlpha,
      sha256: createHash("sha256").update(bytes).digest("hex"),
      extension: format === "jpeg" ? "jpg" : format,
    };
  } catch {
    return fail("invalid_input", "Image cannot be decoded safely.");
  }
}
export async function preview(image: Image, config: Config) {
  for (const dimension of [
    config.files.previewMaxDimension,
    Math.max(1, Math.floor(config.files.previewMaxDimension / 2)),
    Math.min(config.files.previewMaxDimension, 64),
  ]) {
    const { data, info } = await sharp(image.bytes)
      .resize(dimension, dimension, { fit: "inside", withoutEnlargement: true })
      .png()
      .toBuffer({ resolveWithObject: true });
    if (data.length <= config.files.previewMaxBytes)
      return {
        data: data.toString("base64"),
        mimeType: "image/png",
        width: info.width,
        height: info.height,
      };
  }
  return fail(
    "output_file_error",
    "Preview cannot fit configured byte limit.",
    "preview",
  );
}
export async function validateMask(mask: Image, source: Image) {
  if (
    mask.mime !== "image/png" ||
    !mask.alpha ||
    mask.width !== source.width ||
    mask.height !== source.height
  )
    fail(
      "invalid_input",
      "Mask must be a PNG with alpha and the same dimensions as the first reference.",
    );
}
