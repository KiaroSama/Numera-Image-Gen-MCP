import { it, expect } from "vitest";
import { readFile, rename } from "node:fs/promises";
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

it("keeps run and event metadata immutable and releases file handlers", async () =>
  workspace(async (root) => {
    const log = new Logger(root, "DEBUG");
    try {
      log.log("INFO", "fixture", "First.", {
        run_id: "forged",
        timestamp: "forged",
        level: "forged",
        component: "forged",
        message: "forged",
      });
      log.log("ERROR", "fixture", "Second.", {
        api_key: "do-not-log",
        private_path: "do-not-log",
        nested: { authorization: "do-not-log" },
      });
    } finally {
      log.close();
    }
    const lines = (await readFile(log.file!, "utf8"))
      .trim()
      .split("\n")
      .map((v) => JSON.parse(v));
    expect(lines[0]).toMatchObject({
      level: "INFO",
      component: "fixture",
      message: "First.",
    });
    expect(lines[0].run_id).toMatch(/^[a-f0-9-]{36}$/);
    expect(lines[0].run_id).toBe(lines[1].run_id);
    expect(lines[0].timestamp).not.toBe("forged");
    expect(JSON.stringify(lines)).not.toContain("do-not-log");
    await rename(log.file!, log.file! + ".closed");
  }));
