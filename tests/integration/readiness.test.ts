import { it, expect } from "vitest";
import { writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { spawn } from "node:child_process";
import {
  workspace,
  configuration,
  connection,
  server as httpFixture,
  json,
} from "../fixtures/runtime.js";

async function readiness(root: string, args: string[]) {
  const child = spawn(
    process.execPath,
    [
      resolve("scripts/bounded.mjs"),
      "20000",
      "10000",
      resolve("scripts/readiness.mjs"),
      ...args,
    ],
    {
      windowsHide: true,
      stdio: ["ignore", "pipe", "pipe"],
      env: { ...process.env, NUMERA_LOG_DIR: join(root, "logs") },
    },
  );
  let stdout = "",
    stderr = "";
  child.stdout.setEncoding("utf8").on("data", (v) => {
    stdout += v;
  });
  child.stderr.setEncoding("utf8").on("data", (v) => {
    stderr += v;
  });
  const code = await new Promise<number | null>((accept, reject) => {
    child.once("error", reject);
    child.once("close", accept);
  });
  return { code, stdout, stderr };
}

it("readiness uses real stdio tools, GET-only probe and does not leak catalog values", async () => {
  await workspace(async (root) => {
    const methods: string[] = [];
    await httpFixture(
      async (req, res) => {
        methods.push(req.method!);
        json(res, { data: [{ id: "Bearer do-not-print-fixture" }] });
      },
      async (base) => {
        const file = join(root, "config.json");
        await writeFile(
          file,
          JSON.stringify(
            configuration(root, {
              local: connection("openai-images", { baseUrl: `${base}/v1` }),
            }),
          ),
          "utf8",
        );
        const result = await readiness(root, ["--config", file, "--probe"]);
        expect(result.code).toBe(0);
        expect(JSON.parse(result.stdout)).toMatchObject({
          local_ready: true,
          connection_ready: true,
          catalog_count: 1,
          generation_verified: false,
          tool_count: 10,
        });
        expect(methods.length).toBeGreaterThan(0);
        expect(methods.every((method) => method === "GET")).toBe(true);
        expect(result.stdout + result.stderr).not.toContain(
          "do-not-print-fixture",
        );
        expect(result.stderr).toContain('"cleanup_verified":true');
      },
    );
  });
}, 25000);

it("readiness reports missing declared credentials without contacting provider", async () => {
  await workspace(async (root) => {
    let calls = 0;
    await httpFixture(
      async (_req, res) => {
        calls++;
        json(res, { data: [] });
      },
      async (base) => {
        const file = join(root, "config.json");
        await writeFile(
          file,
          JSON.stringify(
            configuration(root, {
              local: connection("openai-images", {
                baseUrl: `${base}/v1`,
                auth: {
                  type: "bearer",
                  secretEnv: "NUMERA_FIXTURE_MISSING_CREDENTIAL_314",
                },
              }),
            }),
          ),
          "utf8",
        );
        const result = await readiness(root, ["--config", file, "--probe"]);
        expect(result.code).toBe(2);
        expect(JSON.parse(result.stdout)).toMatchObject({
          local_ready: true,
          credentials_ready: false,
          connection_ready: false,
          blockers: ["declared_credentials_missing"],
        });
        expect(calls).toBe(0);
      },
    );
  });
}, 25000);

it("readiness local-only checks never contact provider and reject relative config", async () => {
  await workspace(async (root) => {
    let calls = 0;
    await httpFixture(
      async (_req, res) => {
        calls++;
        json(res, { data: [] });
      },
      async (base) => {
        const file = join(root, "config.json");
        await writeFile(
          file,
          JSON.stringify(
            configuration(root, {
              local: connection("openai-images", { baseUrl: `${base}/v1` }),
            }),
          ),
          "utf8",
        );
        const result = await readiness(root, ["--config", file]);
        expect(result.code).toBe(0);
        expect(JSON.parse(result.stdout)).toMatchObject({
          local_ready: true,
          connection_ready: null,
          generation_verified: false,
        });
        expect(calls).toBe(0);
        expect(
          (await readiness(root, ["--config", "relative.json"])).code,
        ).toBe(1);
      },
    );
  });
}, 25000);

it("readiness fails closed on unavailable catalog or unlisted explicit model without POST", async () => {
  await workspace(async (root) => {
    const methods: string[] = [];
    let reject = false;
    await httpFixture(
      async (req, res) => {
        methods.push(req.method!);
        json(
          res,
          reject
            ? { error: "Bearer must-not-print-provider-error" }
            : { data: [{ id: "known" }] },
          reject ? 503 : 200,
        );
      },
      async (base) => {
        const file = join(root, "config.json");
        await writeFile(
          file,
          JSON.stringify(
            configuration(root, {
              local: connection("openai-images", { baseUrl: `${base}/v1` }),
            }),
          ),
          "utf8",
        );
        const unknown = await readiness(root, [
          "--config",
          file,
          "--probe",
          "--model",
          "missing",
        ]);
        expect(unknown.code).toBe(2);
        expect(JSON.parse(unknown.stdout)).toMatchObject({
          selected_model_found: false,
          generation_verified: false,
        });
        reject = true;
        const failed = await readiness(root, ["--config", file, "--probe"]);
        expect(failed.code).toBe(2);
        expect(JSON.parse(failed.stdout)).toMatchObject({
          connection_ready: false,
          catalog_complete: false,
        });
        expect(failed.stdout + failed.stderr).not.toContain(
          "must-not-print-provider-error",
        );
        expect(methods.every((method) => method === "GET")).toBe(true);
      },
    );
  });
}, 25000);
