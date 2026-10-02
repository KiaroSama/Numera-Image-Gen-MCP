import { it, expect } from "vitest";
import { readFile } from "node:fs/promises";
import { Logger } from "../../src/logging.js";
import { workspace } from "../fixtures/runtime.js";
it("creates independent same-second logs and redacts all levels including private signatures", async () =>
  workspace(async (root) => {
    const a = new Logger(root, "DEBUG"),
      b = new Logger(root, "DEBUG");
    try {
      expect(a.file).not.toBe(b.file);
      for (const level of ["INFO", "WARNING", "ERROR", "DEBUG"] as const)
        a.log(level, "fixture", "Safe event.", {
          authorization: "Bearer private",
          signature: "private",
          prompt: "private",
        });
    } finally {
      a.close();
      b.close();
    }
    const lines = (await readFile(a.file!, "utf8"))
      .trim()
      .split("\n")
      .map((l) => JSON.parse(l));
    expect(lines.map((l) => l.level)).toEqual([
      "INFO",
      "WARNING",
      "ERROR",
      "DEBUG",
    ]);
    expect(JSON.stringify(lines)).not.toContain("private");
    expect(lines.every((l) => l.timestamp.endsWith("Z"))).toBe(true);
  }));
