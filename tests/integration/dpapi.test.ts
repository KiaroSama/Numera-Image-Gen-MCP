import { it, expect } from "vitest";
import { readFile, writeFile, mkdir, copyFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { spawn } from "node:child_process";
import { workspace, connection } from "../fixtures/runtime.js";
import { credentials } from "../../src/config/credentials.js";

it.skipIf(process.platform !== "win32")(
  "actual Windows DPAPI script restricts ACL, round-trips same-user fixture and refuses overwrite",
  async () => {
    await workspace(async (root) => {
      const scripts = join(root, "scripts");
      await mkdir(scripts);
      for (const name of ["common.ps1", "protect-secret.ps1"])
        await copyFile(resolve("scripts", name), join(scripts, name));
      const wrapper = join(root, "fixture.ps1");
      await writeFile(
        wrapper,
        `
$ErrorActionPreference='Stop'
function Read-Host { param([string]$Prompt,[switch]$AsSecureString); ConvertTo-SecureString 'non-secret-DPAPI-fixture-日本語' -AsPlainText -Force }
& (Join-Path $PSScriptRoot 'scripts/protect-secret.ps1') -Path $env:NUMERA_FIXTURE_DPAPI
$acl=Get-Acl -LiteralPath $env:NUMERA_FIXTURE_DPAPI
$sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$rules=@($acl.Access | Where-Object {$_.AccessControlType -eq 'Allow'})
$foreign=@($rules | Where-Object {$_.IdentityReference.Translate([Security.Principal.SecurityIdentifier]).Value -ne $sid})
if (-not $acl.AreAccessRulesProtected -or $rules.Count -lt 1 -or $foreign.Count -ne 0) { throw 'Fixture ACL is not current-user only.' }
Write-Output 'ACL_VERIFIED'
`,
        "utf8",
      );
      const file = join(root, "protected 日本語.txt");
      const child = spawn(
        process.execPath,
        [
          resolve("scripts/bounded.mjs"),
          "15000",
          "8000",
          "--exec",
          "pwsh",
          "-NoProfile",
          "-NonInteractive",
          "-File",
          wrapper,
        ],
        {
          windowsHide: true,
          stdio: ["ignore", "pipe", "pipe"],
          env: {
            ...process.env,
            NUMERA_LOG_DIR: join(root, "logs"),
            NUMERA_FIXTURE_DPAPI: file,
          },
        },
      );
      let out = "";
      child.stdout.setEncoding("utf8").on("data", (value) => {
        out += value;
      });
      child.stderr.setEncoding("utf8").on("data", (value) => {
        out += value;
      });
      const code = await new Promise<number | null>((accept, reject) => {
        child.once("error", reject);
        child.once("close", accept);
      });
      expect(code).toBe(0);
      expect(out).toContain("ACL_VERIFIED");
      expect(out).not.toContain("non-secret-DPAPI-fixture");
      const ciphertext = await readFile(file, "utf8");
      expect(ciphertext).not.toContain("non-secret-DPAPI-fixture");
      const c = connection("openai-images", {
        auth: { type: "bearer", secretDpapiFile: file },
      });
      expect(await credentials(c)).toEqual({
        Authorization: "Bearer non-secret-DPAPI-fixture-日本語",
      });
      const corrupt = join(root, "corrupt.txt");
      await writeFile(corrupt, "not-dpapi", "utf8");
      await expect(
        credentials(
          connection("openai-images", {
            auth: { type: "bearer", secretDpapiFile: corrupt },
          }),
        ),
      ).rejects.toMatchObject({ code: "missing_credentials" });
      const repeat = spawn(
        process.execPath,
        [
          resolve("scripts/bounded.mjs"),
          "15000",
          "8000",
          "--exec",
          "pwsh",
          "-NoProfile",
          "-NonInteractive",
          "-File",
          join(scripts, "protect-secret.ps1"),
          "-Path",
          file,
        ],
        {
          windowsHide: true,
          stdio: ["ignore", "pipe", "pipe"],
          env: { ...process.env, NUMERA_LOG_DIR: join(root, "logs") },
        },
      );
      repeat.stdout.resume();
      repeat.stderr.resume();
      const repeatedCode = await new Promise<number | null>(
        (accept, reject) => {
          repeat.once("error", reject);
          repeat.once("close", accept);
        },
      );
      expect(repeatedCode).toBe(1);
      expect(await readFile(file, "utf8")).toBe(ciphertext);
      const { readdir } = await import("node:fs/promises");
      for (const name of await readdir(join(root, "logs"))) {
        const log = await readFile(join(root, "logs", name), "utf8");
        expect(log).not.toContain("non-secret-DPAPI-fixture");
        expect(log).not.toContain(ciphertext);
      }
    });
  },
  30000,
);
