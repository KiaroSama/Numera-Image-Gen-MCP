import { open, link, unlink, readFile, realpath } from "node:fs/promises";
import { join } from "node:path";
import { randomUUID } from "node:crypto";
import type { Config } from "../config/schema.js";
import { contained, outputDirectory, safeRelative } from "./paths.js";
import { inspectImage, type Image } from "./images.js";
import type { Store, Output } from "../jobs/store.js";
import { fail } from "../errors.js";
export async function saveImage(
  image: Image,
  config: Config,
  store: Store,
  requestId: string,
  index: number,
  subdir?: string,
  prefix?: string,
): Promise<Output> {
  const dir = await outputDirectory(config.outputDir, subdir),
    id = randomUUID();
  const name = `${prefix ? safeRelative(prefix) + "-" : ""}${requestId}-${index}-${id}.${image.extension}`;
  if (name.includes("/") || name.includes("\\"))
    fail("invalid_input", "Filename prefix cannot contain directories.");
  const target = join(dir, name),
    temp = join(dir, `.${id}.tmp`);
  try {
    const file = await open(temp, "wx", 0o600);
    try {
      await file.writeFile(image.bytes);
      await file.sync();
    } finally {
      await file.close();
    }
    await link(temp, target);
    await unlink(temp);
    const saved = await inspectImage(await readFile(target), config);
    if (saved.sha256 !== image.sha256)
      fail("output_file_error", "Saved image hash mismatch.", "storage");
    const output = {
      output_id: id,
      request_id: requestId,
      path: target,
      mime_type: saved.mime,
      width: saved.width,
      height: saved.height,
      has_alpha: saved.alpha,
      bytes: saved.bytes.length,
      sha256: saved.sha256,
    };
    store.addOutput(output);
    return output;
  } catch {
    await unlink(temp).catch(() => {});
    return fail(
      "output_file_error",
      "Cannot save verified image; upstream may already have completed. Do not resubmit automatically.",
      "storage",
    );
  }
}
export async function ownedImage(config: Config, store: Store, id: string) {
  const output = store.output(id),
    root = await realpath(config.outputDir),
    path = await realpath(output.path).catch(() =>
      fail("output_file_error", "Owned output file is missing."),
    );
  if (!contained(root, path))
    fail("permission_denied", "Owned output path escapes its root.");
  const image = await inspectImage(await readFile(path), config);
  if (image.sha256 !== output.sha256)
    fail("output_file_error", "Owned output bytes changed after storage.");
  return { output, image };
}
