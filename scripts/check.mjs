import { execFileSync } from "node:child_process";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { maintenance, projectRoot } from "./logging.mjs";

await maintenance("check", async (log) => {
  const names = execFileSync("git", ["ls-files", "-z"], {
    cwd: projectRoot,
    windowsHide: true,
    encoding: "utf8",
    timeout: 8000,
    maxBuffer: 1024 * 1024,
  })
    .split("\0")
    .filter(Boolean);
  const decoder = new TextDecoder("utf-8", { fatal: true });
  let checked = 0;
  for (const name of names) {
    if (!/\.(rs|py|mjs|ps1|json|jsonl|md|ya?ml|toml)$/.test(name)) continue;
    const text = decoder.decode(await readFile(join(projectRoot, name)));
    const lines = text.split(/\r?\n/);
    if (lines.at(-1) === "") lines.pop();
    if (/\.(rs|py|mjs|ps1)$/.test(name) && lines.length > 800)
      throw Object.assign(new Error("Source exceeds 800 lines."), {
        code: "SOURCE_SIZE_LIMIT",
      });
    checked++;
  }
  log.emit("INFO", "Tracked UTF-8 and source-size checks passed.", {
    files: checked,
  });
});
