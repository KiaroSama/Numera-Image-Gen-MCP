import { realpath, lstat, open, mkdir } from "node:fs/promises";
import { isAbsolute, relative, resolve, parse, join } from "node:path";
import { fileURLToPath } from "node:url";
import { fail } from "../errors.js";
export function contained(root: string, path: string) {
  const rel = relative(root, path);
  return rel === "" || (!rel.startsWith("..") && !isAbsolute(rel));
}
export function safeRelative(value: string) {
  if (
    !value ||
    isAbsolute(value) ||
    /[\x00-\x1f:<>"|?*]/.test(value) ||
    value
      .split(/[\\/]/)
      .some(
        (p) =>
          !p ||
          p === "." ||
          p === ".." ||
          /[. ]$/.test(p) ||
          /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(p),
      )
  )
    fail("invalid_input", "Unsafe relative output path or filename.");
  return value;
}
export async function inputFile(
  raw: string,
  roots: string[],
  limit: number,
): Promise<Buffer> {
  let path = raw;
  try {
    if (path.startsWith("file:")) path = fileURLToPath(path);
  } catch {
    fail("input_file_error", "Invalid file URL.");
  }
  if (
    !isAbsolute(path) ||
    path.includes("\0") ||
    /^\\\\[?.]/.test(path) ||
    path.slice(parse(path).root.length).includes(":")
  )
    fail(
      "input_file_error",
      "Expected an absolute regular image path without device paths or streams.",
    );
  const canonical = await realpath(path).catch(() =>
    fail("input_file_error", "Cannot resolve image file."),
  );
  const allowed = await Promise.all(
    roots.map((r) => realpath(r).catch(() => "")),
  );
  if (!allowed.some((root) => root && contained(root, canonical)))
    fail("permission_denied", "Image is outside allowed input roots.");
  const before = await lstat(canonical);
  if (!before.isFile() || before.size > limit)
    fail("input_file_error", "Input is not a bounded regular file.");
  const handle = await open(canonical, "r").catch(() =>
    fail("input_file_error", "Cannot open bounded image file."),
  );
  try {
    const info = await handle.stat();
    if (!info.isFile() || info.size > limit)
      fail("input_file_error", "Input is not a bounded regular file.");
    const bytes = await handle.readFile();
    if (bytes.length > limit)
      fail("input_file_error", "Input exceeds byte limit.");
    return bytes;
  } finally {
    await handle.close();
  }
}
export async function outputDirectory(root: string, subdirectory?: string) {
  await mkdir(root, { recursive: true, mode: 0o700 });
  const base = await realpath(root);
  const target = resolve(
    base,
    subdirectory
      ? safeRelative(subdirectory)
      : new Date().toISOString().slice(0, 10),
  );
  let current = base;
  for (const part of relative(base, target).split(/[\\/]/).filter(Boolean)) {
    current = join(current, part);
    try {
      const info = await lstat(current);
      if (info.isSymbolicLink() || !info.isDirectory())
        fail(
          "output_file_error",
          "Output path contains a link or non-directory.",
        );
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
      await mkdir(current, { mode: 0o700 });
    }
    if (!contained(base, await realpath(current)))
      fail("output_file_error", "Output path escapes output root.");
  }
  return target;
}
