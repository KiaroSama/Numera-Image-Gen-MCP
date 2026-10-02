import { readdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import { runBounded } from "./bounded.mjs";
import { maintenance, projectRoot } from "./logging.mjs";

await maintenance("check", async (log) => {
  for (const [script, args] of [
    ["node_modules/typescript/bin/tsc", ["--noEmit"]],
    ["node_modules/eslint/bin/eslint.js", ["src", "tests"]],
    ["node_modules/typescript/bin/tsc", ["-p", "tsconfig.build.json"]],
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
