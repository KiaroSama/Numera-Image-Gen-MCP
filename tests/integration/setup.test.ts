import { it, expect } from "vitest";
import {
  readFile,
  writeFile,
  readdir,
  copyFile,
  mkdir,
} from "node:fs/promises";
import { join, resolve, isAbsolute } from "node:path";
import { spawn } from "node:child_process";
import { workspace } from "../fixtures/runtime.js";

const scripts = resolve("scripts");
async function cli(
  root: string,
  script: string,
  args: string[] = [],
  bounds = [12000, 6000],
) {
  const child = spawn(
    process.execPath,
    [
      join(scripts, "bounded.mjs"),
      ...bounds.map(String),
      isAbsolute(script) ? script : join(scripts, script),
      ...args,
    ],
    {
      cwd: root,
      windowsHide: true,
      stdio: ["ignore", "pipe", "pipe"],
      env: { ...process.env, NUMERA_LOG_DIR: join(root, "logs") },
    },
  );
  let stdout = "",
    stderr = "";
  child.stdout.setEncoding("utf8").on("data", (value) => {
    stdout += value;
  });
  child.stderr.setEncoding("utf8").on("data", (value) => {
    stderr += value;
  });
  return await new Promise<{
    code: number | null;
    stdout: string;
    stderr: string;
  }>((accept, reject) => {
    child.once("error", reject);
    child.once("close", (code) => accept({ code, stdout, stderr }));
  });
}
function registration(
  root: string,
  client = "ClaudeDesktop",
  action = "Register",
) {
  return [
    client,
    action,
    join(root, client === "Codex" ? "host.toml" : "host.json"),
    process.execPath,
    join(root, "dist/index.js"),
    join(root, "config.json"),
  ];
}

it(
  "real registration CLI is dry-run safe, idempotent, backs up exact bytes and permits fixture rollback/removal",
  async () =>
    workspace(async (root) => {
      const file = join(root, "host.json");
      const before =
        '{"model":"unchanged 日本語","mcpServers":{"other":{"command":"stay"}}}\n';
      await writeFile(file, before, "utf8");
      const args = registration(root);
      expect(
        (await cli(root, "register.mjs", [...args, "--dry-run"])).code,
      ).toBe(0);
      expect(await readFile(file, "utf8")).toBe(before);
      expect(
        (await readdir(root)).some((name) => name.includes("backup")),
      ).toBe(false);
      expect((await cli(root, "register.mjs", args)).code).toBe(0);
      const after = await readFile(file, "utf8");
      expect(JSON.parse(after)).toMatchObject({
        model: "unchanged 日本語",
        mcpServers: {
          other: { command: "stay" },
          "numera-image-gen": { command: process.execPath },
        },
      });
      expect((await cli(root, "register.mjs", args)).code).toBe(0);
      expect(await readFile(file, "utf8")).toBe(after);
      const backups = (await readdir(root)).filter((name) =>
        name.includes("backup"),
      );
      expect(backups).toHaveLength(1);
      expect(await readFile(join(root, backups[0]!), "utf8")).toBe(before);
      await copyFile(join(root, backups[0]!), file);
      expect(await readFile(file, "utf8")).toBe(before);
      expect((await cli(root, "register.mjs", args)).code).toBe(0);
      expect(
        (
          await cli(
            root,
            "register.mjs",
            registration(root, "ClaudeDesktop", "Remove"),
          )
        ).code,
      ).toBe(0);
      expect(JSON.parse(await readFile(file, "utf8"))).toEqual(
        JSON.parse(before),
      );
      expect(
        (await readdir(root)).some((name) =>
          /\.tmp$|\.numera-lock$/.test(name),
        ),
      ).toBe(false);
      const logs = await readdir(join(root, "logs"));
      expect(new Set(logs).size).toBe(logs.length);
      for (const name of logs) {
        expect(name).toMatch(
          /_\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}_UTC-[\da-f-]+\.log$/,
        );
        const text = await readFile(join(root, "logs", name), "utf8");
        expect(text).not.toContain(root);
        for (const line of text.trim().split("\n")) {
          const event = JSON.parse(line);
          expect(event.timestamp).toMatch(/Z$/);
          expect(["INFO", "WARNING", "ERROR", "DEBUG"]).toContain(event.level);
          expect(event.run_id).toBeTruthy();
        }
      }
    }),
  20000,
);

it(
  "real TOML CLI preserves unrelated bytes/comments and refuses ambiguous quoted, duplicate and multiline definitions",
  async () =>
    workspace(async (root) => {
      const file = join(root, "host.toml");
      const before =
        '# retained\r\nmodel = "unchanged"\r\n[mcp_servers.other]\r\ncommand = "existing" # retained\r\n';
      await writeFile(file, before, "utf8");
      const args = registration(root, "Codex");
      expect((await cli(root, "register.mjs", args)).code).toBe(0);
      const after = await readFile(file, "utf8");
      expect(after.startsWith(before)).toBe(true);
      expect((await cli(root, "register.mjs", args)).code).toBe(0);
      expect(await readFile(file, "utf8")).toBe(after);
      expect(
        (await cli(root, "register.mjs", registration(root, "Codex", "Remove")))
          .code,
      ).toBe(0);
      expect((await readFile(file, "utf8")).startsWith(before)).toBe(true);
      for (const ambiguous of [
        '[mcp_servers."numera-image-gen"]\ncommand="other"\n',
        '[mcp_servers.numera-image-gen]\ncommand="a"\n[mcp_servers.numera-image-gen]\ncommand="b"\n',
        'description = """\n[mcp_servers.numera-image-gen]\n"""\n',
        'mcp_servers = { "numera-image-gen" = { command = "other" } }\n',
        '[mcp_servers]\nnumera-image-gen.command = "other"\n',
      ]) {
        await writeFile(file, ambiguous, "utf8");
        const result = await cli(root, "register.mjs", args);
        expect(result.code).not.toBe(0);
        expect(await readFile(file, "utf8")).toBe(ambiguous);
        expect(result.stderr).not.toContain(root);
      }
    }),
  20000,
);

it(
  "real registration CLI refuses active locks and invalid host JSON without partial writes",
  async () =>
    workspace(async (root) => {
      const file = join(root, "host.json");
      const args = registration(root);
      await writeFile(
        file,
        '{"mcpServers":{"other":{"command":"stay"}}}',
        "utf8",
      );
      const concurrent = await Promise.all([
        cli(root, "register.mjs", args),
        cli(root, "register.mjs", args),
      ]);
      expect(concurrent.some((result) => result.code === 0)).toBe(true);
      expect(
        JSON.parse(await readFile(file, "utf8")).mcpServers.other.command,
      ).toBe("stay");
      await writeFile(
        `${file}.numera-lock`,
        "owned by another request",
        "utf8",
      );
      const before = await readFile(file, "utf8");
      expect((await cli(root, "register.mjs", args)).code).not.toBe(0);
      expect(await readFile(file, "utf8")).toBe(before);
      expect(await readFile(`${file}.numera-lock`, "utf8")).toBe(
        "owned by another request",
      );
      for (const invalid of [
        '{"mcpServers":null}',
        '{"mcpServers":[]}',
        "{malformed",
      ]) {
        const invalidFile = join(root, "invalid.json");
        await writeFile(invalidFile, invalid, "utf8");
        const request = [...args];
        request[2] = invalidFile;
        expect((await cli(root, "register.mjs", request)).code).not.toBe(0);
        expect(await readFile(invalidFile, "utf8")).toBe(invalid);
        expect(
          (await readdir(root)).some((name) =>
            name.startsWith("invalid.json.numera"),
          ),
        ).toBe(false);
      }
    }),
  20000,
);

it(
  "bounded CLI kills the owned process tree on idle timeout and records verified cleanup",
  async () =>
    workspace(async (root) => {
      const file = join(root, "tree.mjs");
      await writeFile(
        file,
        `import { spawn } from 'node:child_process'; import { writeFileSync } from 'node:fs';\nconst child=spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{windowsHide:true,stdio:'ignore'});writeFileSync(process.argv[2],String(child.pid),'utf8');setInterval(()=>{},1000);`,
        "utf8",
      );
      const pidFile = join(root, "pid.txt");
      const result = await cli(root, file, [pidFile], [5000, 500]);
      expect(result.code).toBe(124);
      const pid = Number(await readFile(pidFile, "utf8"));
      expect(() => process.kill(pid, 0)).toThrow();
      const records = (
        await readFile(join(root, ".ci-work/processes.jsonl"), "utf8")
      )
        .trim()
        .split("\n")
        .map((line) => JSON.parse(line));
      expect(records).toEqual(
        expect.arrayContaining([
          expect.objectContaining({
            event: "start",
            idle_ms: 500,
            wall_ms: 5000,
          }),
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

it(
  "PowerShell 7 syntax and every setup WhatIf action run without build, smoke or host changes",
  async () =>
    workspace(async (root) => {
      await mkdir(join(root, "dist"));
      const fixtureScripts = join(root, "scripts");
      await mkdir(fixtureScripts);
      for (const name of [
        "setup.ps1",
        "common.ps1",
        "bounded.mjs",
        "logging.mjs",
        "register.mjs",
      ])
        await copyFile(join(scripts, name), join(fixtureScripts, name));
      await writeFile(
        join(fixtureScripts, "smoke.mjs"),
        "process.exit(79);",
        "utf8",
      );
      await writeFile(
        join(root, "package.json"),
        JSON.stringify({
          name: "fixture",
          scripts: { smoke: 'node -e "process.exit(79)"' },
        }),
        "utf8",
      );
      const file = join(root, "host.json");
      await writeFile(file, '{"retained":true}', "utf8");
      const fixture = join(root, "ps-check.mjs");
      await writeFile(
        fixture,
        `import { spawnSync } from 'node:child_process';\nconst r=spawnSync('pwsh',['-NoProfile','-NonInteractive','-Command',process.argv[2]],{windowsHide:true,encoding:'utf8',timeout:10000,stdio:['ignore','pipe','pipe']});process.stdout.write(r.stdout??'');process.stderr.write(r.stderr??'');process.exitCode=r.status??1;`,
        "utf8",
      );
      const q = (value: string) => "'" + value.replaceAll("'", "''") + "'";
      const command = `$ErrorActionPreference='Stop'; foreach($file in @(${["common.ps1", "setup.ps1", "protect-secret.ps1"].map((name) => q(join(scripts, name))).join(",")})) { $tokens=$null; $errors=$null; [System.Management.Automation.Language.Parser]::ParseFile($file,[ref]$tokens,[ref]$errors)|Out-Null; if($errors.Count){throw 'PowerShell syntax invalid'} }; foreach($action in @('Install','Update','Diagnose','Register','Remove')) { & ${q(join(fixtureScripts, "setup.ps1"))} -ProjectRoot ${q(root)} -ClientConfig ${q(file)} -NumeraConfig ${q(join(root, "config.json"))} -Action $action -WhatIf; if($LASTEXITCODE -and $LASTEXITCODE -ne 0){exit $LASTEXITCODE} }`;
      const result = await cli(root, fixture, [command]);
      expect(result.code, result.stderr).toBe(0);
      expect(await readFile(file, "utf8")).toBe('{"retained":true}');
      expect(
        (await readdir(root)).some((name) =>
          /backup|numera-lock|node_modules|state|outputs/.test(name),
        ),
      ).toBe(false);
      expect(result.stderr).not.toContain(root);
    }),
  20000,
);
