// Frozen first-party output requirements, including exact persisted deviations and warnings.
export async function requirementsOracle({ source, emit, plain, images }) {
  const { snapshotRequirements, outputDeviations, outputRequirementWarnings } =
    await source("services/output-requirements.js");
  const original = images[0].metadata;
  const image = {
    mime: original.mime,
    width: original.width,
    height: original.height,
    alpha: original.alpha,
  };
  const cases = [];
  const add = (name, model, request, metadata = image) => {
    const receipt = {
      output_requirements: {
        count: 1,
        ...request,
        ...snapshotRequirements(plain, model, request),
      },
    };
    cases.push({
      name,
      model,
      request,
      receipt,
      image: metadata,
      deviations: outputDeviations(receipt, metadata, 0),
      warnings: outputRequirementWarnings(receipt),
    });
  };
  add("original-aspect-tier", "gemini-3.1-flash-image", {
    aspect_ratio: "16:9",
    image_size: "4K",
  });
  add("opaque-aspect-tier", "opaque-model", {
    aspect_ratio: "16:9",
    image_size: "4K",
  });
  add("known-missing-aspect", "gemini-3.1-flash-image", {
    image_size: "4K",
  });
  add("known-unmatched-tier", "gemini-3.1-flash-image", {
    aspect_ratio: "16:9",
    image_size: "unknown-tier",
  });
  add("known-unmatched-aspect", "gemini-3.1-flash-image", {
    aspect_ratio: "unknown-aspect",
    image_size: "4K",
  });
  add("no-tier", "opaque-model", { aspect_ratio: "16:9" });
  for (const model of ["gemini-3.1-flash-image", "gemini-3-pro-image"])
    for (const size of snapshotRequirements(plain, model, {}).dimension_evidence
      .pixel_sizes) {
      add(
        `${model}-${size.aspect_ratio}-${size.resolution}`,
        model,
        { aspect_ratio: size.aspect_ratio, image_size: size.resolution },
        { ...image, width: size.width, height: size.height },
      );
    }
  await emit("output-requirements", cases);
}
