import type { Connection, ImageRequest } from "../config/schema.js";
import type { Receipt } from "../jobs/store.js";
import type { Image } from "../files/images.js";
import { dimensionEvidence } from "./image-dimensions.js";

export function snapshotRequirements(
  connection: Connection,
  model: string,
  request: ImageRequest,
) {
  return {
    aspect_ratio: request.aspect_ratio,
    image_size: request.image_size,
    dimension_evidence: dimensionEvidence(connection, model),
  };
}

export function outputDeviations(
  receipt: Receipt,
  image: Image,
  index: number,
): string[] {
  const expected = receipt.output_requirements;
  if (!expected) return [];
  const deviations: string[] = [];
  if (
    expected.output_format &&
    image.mime !== `image/${expected.output_format}`
  )
    deviations.push(
      `Output ${index} format is ${image.mime}, not requested ${expected.output_format}.`,
    );
  if (
    expected.size &&
    /^\d+x\d+$/.test(expected.size) &&
    expected.size !== `${image.width}x${image.height}`
  )
    deviations.push(`Output ${index} dimensions differ from requested size.`);
  if (expected.aspect_ratio) {
    const [width, height] = expected.aspect_ratio.split(":").map(Number);
    const native = expected.dimension_evidence?.pixel_sizes?.filter(
      (size) => size.aspect_ratio === expected.aspect_ratio,
    );
    // Native ratio labels are rounded; only sourced pixel pairs override literal ratio comparison.
    const matchesNative = native?.some(
      (size) => size.width === image.width && size.height === image.height,
    );
    if (
      width &&
      height &&
      !matchesNative &&
      image.width * height !== image.height * width
    )
      deviations.push(
        `Output ${index} aspect ratio differs from requested aspect_ratio.`,
      );
  }
  const evidence = expected.dimension_evidence;
  if (expected.image_size && evidence?.pixel_sizes && expected.aspect_ratio) {
    const target = evidence.pixel_sizes.find(
      (size) =>
        size.aspect_ratio === expected.aspect_ratio &&
        size.resolution === expected.image_size,
    );
    if (
      target &&
      (target.width !== image.width || target.height !== image.height)
    )
      deviations.push(
        `Output ${index} dimensions differ from the documented requested resolution tier.`,
      );
  }
  const source = expected.upscale_source;
  if (
    source &&
    (image.width < source.width ||
      image.height < source.height ||
      (image.width === source.width && image.height === source.height))
  )
    deviations.push(
      `Output ${index} was not upscaled beyond the reference dimensions.`,
    );
  if (expected.background === "transparent" && !image.alpha)
    deviations.push(`Output ${index} has no alpha channel.`);
  return deviations;
}

export function outputRequirementWarnings(receipt: Receipt): string[] {
  const requirements = receipt.output_requirements;
  const target = requirements?.dimension_evidence?.pixel_sizes?.find(
    (size) =>
      size.aspect_ratio === requirements.aspect_ratio &&
      size.resolution === requirements.image_size,
  );
  return requirements?.image_size && !target
    ? [
        "Requested resolution tier could not be verified: exact model/tier/aspect evidence is unavailable.",
      ]
    : [];
}
