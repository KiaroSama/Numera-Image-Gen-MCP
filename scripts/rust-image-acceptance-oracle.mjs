// Exact tiny frozen tests/unit/images.test.ts inputs, separate from broad codec oracles.
export async function imageAcceptanceOracle({ source, emit, sharp }) {
  const { inspectImage } = await source("files/images.js");
  const images = [];
  for (const [
    name,
    format,
    alpha,
    width,
    height,
    background = { r: 24, g: 100, b: 180, alpha: 0.5 },
  ] of [
    ["png", "png", true, 8, 6],
    ["jpeg", "jpeg", true, 8, 6],
    ["webp", "webp", true, 8, 6],
    ["opaque-png", "png", false, 8, 6],
    ["opaque-jpeg", "jpeg", false, 8, 6],
    ["width-mask", "png", true, 7, 6],
    ["height-mask", "png", true, 8, 5],
    ["tamper-png", "png", true, 9, 6],
    ["upscale-png", "png", true, 16, 12],
    ["mcp-png", "png", true, 8, 6, { r: 20, g: 40, b: 60, alpha: 0.5 }],
    ["portrait-png", "png", true, 6, 8],
  ]) {
    const image = sharp({
      create: {
        width,
        height,
        channels: alpha ? 4 : 3,
        background,
      },
    });
    const bytes = await (
      name === "mcp-png" ? image.png() : image.toFormat(format)
    ).toBuffer();
    const { bytes: original, ...metadata } = await inspectImage(bytes, {
      files: { maxOutputBytes: 1048576, maxPixels: 64000000 },
    });
    images.push({ name, base64: original.toString("base64"), metadata });
  }
  await emit("image-acceptance", images);
}
