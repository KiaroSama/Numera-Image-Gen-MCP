// Original source-generated animated media; target decoders must reject, never take frame one.
export async function imageEdgeOracle({ sharp, inspectImage, config, emit }) {
  const pixels = Buffer.from([255, 0, 0, 255, 0, 255, 0, 255]);
  const bytes = await sharp(pixels, {
    raw: { width: 1, height: 2, channels: 4, pageHeight: 1 },
  })
    .webp({ lossless: true, delay: [50, 50], loop: 0 })
    .toBuffer();
  const metadata = await sharp(bytes, { animated: true }).metadata();
  if (metadata.pages !== 2)
    throw new Error("Source animated fixture has no second frame.");
  let error;
  try {
    await inspectImage(bytes, config);
  } catch (failure) {
    error = failure.code;
  }
  if (error !== "invalid_input")
    throw new Error("Source accepted animated media unexpectedly.");
  await emit("animated-webp", {
    base64: bytes.toString("base64"),
    pages: metadata.pages,
    error,
  });
}
