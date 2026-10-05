import { expect, it, vi } from "vitest";
import * as fs from "node:fs/promises";
import { join } from "node:path";
import { workspace } from "../fixtures/runtime.js";

vi.mock("node:fs/promises", async (original) => {
  const actual = await original<typeof fs>();
  return { ...actual, lstat: vi.fn(actual.lstat) };
});
const { outputDirectory } = await import("../../src/files/paths.js");

it.each(["directory", "file", "link"] as const)(
  "rechecks a concurrent %s after two real missing-path observations",
  async (kind) =>
    workspace(async (root) => {
      const target = join(root, "batch"),
        outside = join(root, "outside");
      const actual = await vi.importActual<typeof fs>("node:fs/promises");
      const stat = vi.mocked(fs.lstat);
      let observations = 0;
      let release!: () => void;
      const ready = new Promise<void>((resolve) => {
        release = resolve;
      });
      stat.mockImplementation(async (path, options) => {
        if (String(path) !== target || observations >= 2)
          return actual.lstat(path, options);
        try {
          return await actual.lstat(path, options);
        } catch (error) {
          if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
          observations++;
          if (observations === 2) {
            try {
              if (kind === "file")
                await actual.writeFile(target, "fixture", "utf8");
              if (kind === "link") {
                await actual.mkdir(outside);
                await actual.symlink(
                  outside,
                  target,
                  process.platform === "win32" ? "junction" : "dir",
                );
              }
            } finally {
              release();
            }
          }
          await ready;
          throw error;
        }
      });
      try {
        const results = await Promise.allSettled([
          outputDirectory(root, "batch/nested"),
          outputDirectory(root, "batch/nested"),
        ]);
        expect(observations).toBe(2);
        if (kind === "directory")
          expect(results).toEqual([
            { status: "fulfilled", value: join(target, "nested") },
            { status: "fulfilled", value: join(target, "nested") },
          ]);
        else
          for (const result of results) {
            expect(result.status).toBe("rejected");
            if (result.status === "rejected")
              expect(result.reason).toMatchObject({
                code: "output_file_error",
              });
          }
        if (kind === "link") expect(await actual.readdir(outside)).toEqual([]);
      } finally {
        release();
        stat.mockImplementation(actual.lstat);
        stat.mockClear();
      }
    }),
);
