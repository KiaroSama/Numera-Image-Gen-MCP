import assert from "node:assert/strict";
import { readdir, readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { join } from "node:path";
import { runBounded } from "./bounded.mjs";
import { maintenance, projectRoot } from "./logging.mjs";

await maintenance("check", async (log) => {
  const require = createRequire(import.meta.url);
  const api = require("typescript");
  const native = require("@typescript/native/package.json");
  assert.equal(native.version, "7.0.2", "Native compiler version mismatch.");
  assert.equal(api.version, "6.0.3", "Lint compiler API version mismatch.");
  assert.equal(
    typeof api.createProgram,
    "function",
    "Lint compiler API missing.",
  );
  log.emit("INFO", "Native compiler7 and lint API6 compatibility verified.");
  for (const [script, args] of [
    [
      "node_modules/@typescript/native/bin/tsc",
      ["--singleThreaded", "--noEmit"],
    ],
    ["node_modules/eslint/bin/eslint.js", ["src", "tests"]],
    [
      "node_modules/@typescript/native/bin/tsc",
      ["--singleThreaded", "-p", "tsconfig.build.json"],
    ],
  ]) {
    const result = await runBounded(
      process.execPath,
      [join(projectRoot, script), ...args],
      { wall: 30000, idle: 20000, log },
    );
    if (result.code) {
      process.exitCode = result.code;
      return;
    }
  }
  async function files(dir) {
    const result = [];
    for (const entry of await readdir(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) result.push(...(await files(path)));
      else result.push(path);
    }
    return result;
  }
  const decoder = new TextDecoder("utf-8", { fatal: true });
  for (const dir of ["src", "tests", "scripts", "docs", "examples", ".github"])
    for (const path of await files(join(projectRoot, dir))) {
      if (/\.(ts|mjs|ps1|json|md|ya?ml|toml)$/.test(path)) {
        const text = decoder.decode(await readFile(path));
        if (/\.(ts|mjs|ps1)$/.test(path) && text.split("\n").length > 800)
          throw Object.assign(new Error("Source exceeds 800 lines."), {
            code: "SOURCE_SIZE_LIMIT",
          });
      }
    }
  log.emit("INFO", "Type, lint, build, UTF-8 and source-size checks passed.");
});
