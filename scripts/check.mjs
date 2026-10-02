import { spawnSync } from "node:child_process";
import { readdir, readFile } from "node:fs/promises";
import { join } from "node:path";
for (const [script, args] of [
  ["node_modules/typescript/bin/tsc", ["--noEmit"]],
  ["node_modules/eslint/bin/eslint.js", ["src", "tests"]],
  ["node_modules/typescript/bin/tsc", ["-p", "tsconfig.build.json"]],
]) {
  const r = spawnSync(process.execPath, [script, ...args], {
    windowsHide: true,
    stdio: "inherit",
    timeout: 30000,
  });
  if (r.status !== 0) process.exit(r.status ?? 1);
}
async function files(dir) {
  const result = [];
  for (const e of await readdir(dir, { withFileTypes: true })) {
    const p = join(dir, e.name);
    if (e.isDirectory()) result.push(...(await files(p)));
    else result.push(p);
  }
  return result;
}
const decoder = new TextDecoder("utf-8", { fatal: true });
for (const dir of ["src", "tests", "scripts", "docs", "examples", ".github"])
  for (const path of await files(dir)) {
    if (/\.(ts|mjs|ps1|json|md|ya?ml|toml)$/.test(path)) {
      const text = decoder.decode(await readFile(path));
      if (/\.(ts|mjs|ps1)$/.test(path) && text.split("\n").length > 800)
        throw new Error(`Source exceeds 800 lines: ${path}`);
    }
  }
process.stderr.write(
  "INFO: Type, lint, build, UTF-8 and source-size checks passed.\n",
);
