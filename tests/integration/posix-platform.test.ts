import { describe, it, expect } from "vitest";
import {
  writeFile,
  readFile,
  chmod,
  mkdir,
  symlink,
  stat,
} from "node:fs/promises";
import { join, resolve } from "node:path";
import { spawn, execFileSync } from "node:child_process";
import { workspace, connection, imageBytes } from "../fixtures/runtime.js";
import { credentials } from "../../src/config/credentials.js";
import { inputFile } from "../../src/files/paths.js";
import { loadConfig } from "../../src/config/load.js";

describe.skipIf(!["linux", "darwin"].includes(process.platform))(
  "Linux/macOS native boundaries",
  () => {
    it("uses owner-only POSIX credentials and native Unicode configuration paths", async () =>
      workspace(async (root) => {
        const file = join(root, "private key 日本語.txt");
        await writeFile(file, "posix-fixture-key", {
          encoding: "utf8",
          mode: 0o600,
        });
        const c = connection("openai-images", {
          auth: { type: "bearer", secretFile: file },
        });
        expect(await credentials(c)).toEqual({
          Authorization: "Bearer posix-fixture-key",
        });
        expect((await stat(file)).mode & 0o777).toBe(0o600);
        await chmod(file, 0o644);
        await expect(credentials(c)).rejects.toMatchObject({
          code: "missing_credentials",
        });
        const config = join(root, "config 日本語.json");
        await writeFile(
          config,
          JSON.stringify({
            api_endpoint: "https://fixture.example/v1",
            api_key: "fixture-inline",
            models: [{ id: "image/opaque", name: "日本語" }],
          }),
          { encoding: "utf8", mode: 0o600 },
        );
        expect((await loadConfig(["--config", config], {})).outputDir).toBe(
          join(root, "outputs"),
        );
      }));
    it("rejects POSIX symlinks escaping approved image roots", async () =>
      workspace(async (root) => {
        const input = join(root, "inputs");
        await mkdir(input);
        const outside = join(root, "outside.png"),
          bytes = await imageBytes();
        await writeFile(outside, bytes);
        const alias = join(input, "escape.png");
        await symlink(outside, alias);
        await expect(inputFile(alias, [input], 1024)).rejects.toMatchObject({
          code: "permission_denied",
        });
        const inside = join(input, "inside.png");
        await writeFile(inside, bytes);
        const safe = join(input, "safe.png");
        await symlink(inside, safe);
        expect(await inputFile(safe, [input], 1024)).toEqual(bytes);
      }));
    it(
      "bounds and reaps an owned POSIX process group without touching other processes",
      async () =>
        workspace(async (root) => {
          const target = join(root, "tree.mjs"),
            pidFile = join(root, "child.pid");
          await writeFile(
            target,
            `import {spawn} from 'node:child_process';import {writeFileSync} from 'node:fs';const c=spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{stdio:'ignore'});writeFileSync(process.argv[2],String(c.pid),'utf8');console.log('fixture-ready');setInterval(()=>{},1000);`,
            "utf8",
          );
          const child = spawn(
            process.execPath,
            [resolve("scripts/bounded.mjs"), "5000", "500", target, pidFile],
            {
              cwd: root,
              stdio: ["ignore", "pipe", "pipe"],
              env: { ...process.env, NUMERA_LOG_DIR: join(root, "logs") },
            },
          );
          let text = "";
          child.stdout.on("data", (v) => {
            text += v.toString("utf8");
          });
          child.stderr.on("data", (v) => {
            text += v.toString("utf8");
          });
          const code = await new Promise<number | null>((accept, reject) => {
            child.once("error", reject);
            child.once("close", accept);
          });
          expect(code).toBe(124);
          expect(text).toContain("fixture-ready");
          const pid = Number(await readFile(pidFile, "utf8"));
          let state = "";
          try {
            state = execFileSync("ps", ["-o", "stat=", "-p", String(pid)], {
              encoding: "utf8",
              timeout: 2000,
              stdio: ["ignore", "pipe", "ignore"],
            }).trim();
          } catch {}
          expect(state === "" || state.startsWith("Z")).toBe(true);
          const records = (
            await readFile(join(root, ".ci-work/processes.jsonl"), "utf8")
          )
            .trim()
            .split("\n")
            .map((v) => JSON.parse(v));
          expect(records).toEqual(
            expect.arrayContaining([
              expect.objectContaining({ event: "terminate", reason: "idle" }),
              expect.objectContaining({
                event: "finish",
                exit_code: 124,
                cleanup_verified: true,
              }),
            ]),
          );
        }),
      15000,
    );
  },
);
