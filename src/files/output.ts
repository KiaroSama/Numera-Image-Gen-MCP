import { open, link, unlink, realpath } from "node:fs/promises";
import { join, dirname } from "node:path";
import { randomUUID } from "node:crypto";
import type { Config } from "../config/schema.js";
import { contained, outputDirectory, safeRelative } from "./paths.js";
import { inspectImage, type Image } from "./images.js";
import type { Store, Output, Receipt } from "../jobs/store.js";
import { fail } from "../errors.js";
export async function saveImage(
  image: Image,
  config: Config,
  store: Store,
  requestId: string,
  index: number,
  subdir?: string,
  prefix?: string,
  options?: { stable: boolean; deviations?: string[]; receipt?: Receipt },
): Promise<Output> {
  const dir = await outputDirectory(config.outputDir, subdir);
  const filename = (id: string) => {
    const name = `${prefix ? safeRelative(prefix) + "-" : ""}${requestId}-${index}-${id}.${image.extension}`;
    if (name.includes("/") || name.includes("\\"))
      fail("invalid_input", "Filename prefix cannot contain directories.");
    return join(dir, name);
  };
  const publication = options?.stable
    ? store.publication(requestId, index, filename, options.receipt)
    : undefined;
  const id = publication?.output_id ?? randomUUID(),
    target = publication?.target ?? filename(id);
  const parent = await realpath(dirname(target));
  if (parent !== dir || !contained(await realpath(config.outputDir), parent))
    fail(
      "permission_denied",
      "Retained output target does not match the configured output directory.",
    );
  if (publication && publication.sha256 !== image.sha256)
    fail(
      "output_file_error",
      "Retained output hash does not match image bytes.",
      "storage",
    );
  const temp = join(dir, `.${id}-${randomUUID()}.tmp`);
  try {
    if (publication?.phase === "committed") return store.output(id);
    const file = await open(temp, "wx", 0o600);
    try {
      await file.writeFile(image.bytes);
      await file.sync();
    } finally {
      await file.close();
    }
    if (options?.stable && !store.owns(requestId, options.receipt))
      fail(
        "outcome_unknown",
        "Output writer lease changed before publication.",
        "storage",
      );
    try {
      await link(temp, target);
    } catch (error) {
      if (
        !options?.stable ||
        (error as NodeJS.ErrnoException).code !== "EEXIST"
      )
        throw error;
      // A crash may have published this item already; filename alone never proves ownership.
    }
    await unlink(temp);
    // Windows does not provide the same directory-fsync contract; do not claim global power-loss durability.
    if (process.platform !== "win32") {
      const directory = await open(dir, "r");
      try {
        await directory.sync();
      } finally {
        await directory.close();
      }
    }
    const resolved = await realpath(target);
    if (resolved !== target || !contained(parent, resolved))
      fail("permission_denied", "Published image path changed during storage.");
    const savedFile = await open(target, "r");
    let bytes: Buffer;
    try {
      const stat = await savedFile.stat();
      if (!stat.isFile() || stat.size > config.files.maxOutputBytes)
        fail(
          "output_file_error",
          "Published output is not a bounded regular image.",
          "storage",
        );
      bytes = await savedFile.readFile();
    } finally {
      await savedFile.close();
    }
    const saved = await inspectImage(bytes, config);
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
    if (options?.stable)
      store.commitResult(
        requestId,
        index,
        output,
        options.deviations,
        options.receipt,
      );
    else store.addOutput(output);
    return output;
  } catch {
    await unlink(temp).catch(() => {});
    return fail(
      "output_file_error",
      "Cannot save verified image; retained results can be recovered with get_job. Do not resubmit automatically.",
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
  const file = await open(path, "r");
  let bytes: Buffer;
  try {
    const info = await file.stat();
    if (!info.isFile() || info.size > config.files.maxOutputBytes)
      fail(
        "output_file_error",
        "Owned output exceeds bounded regular-file limits.",
      );
    bytes = await file.readFile();
  } finally {
    await file.close();
  }
  const image = await inspectImage(bytes, config);
  if (image.sha256 !== output.sha256)
    fail("output_file_error", "Owned output bytes changed after storage.");
  return { output, image };
}
