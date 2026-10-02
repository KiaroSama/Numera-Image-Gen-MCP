import { describe, expect, it } from "vitest";
import { chmod, mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { credentials } from "../../src/config/credentials.js";
import {
  loadConfig,
  selectConnection,
  userRoot,
} from "../../src/config/load.js";
import { configSchema, requestSchema } from "../../src/config/schema.js";
import {
  capabilities,
  effectiveRequest,
  safeOptions,
} from "../../src/capabilities.js";
import { health } from "../../src/services/health.js";
import { configuration, connection, workspace } from "../fixtures/runtime.js";

const request = (options: Record<string, unknown> = {}) =>
  requestSchema.parse({ prompt: "Draw a café", ...options });

describe("configuration and isolated credentials", () => {
  it.each([
    { schemaVersion: 2 },
    { extra: true },
    { maxConcurrentRequests: 0 },
    { logging: { directory: "/logs", logPrompts: true } },
    { connections: { "bad name": {} } },
  ])("rejects invalid configuration %j", (changes) => {
    const config = configuration(join(process.cwd(), ".ci-work", "schema"));
    expect(configSchema.safeParse({ ...config, ...changes }).success).toBe(
      false,
    );
  });

  it.each([
    { prompt: "  " },
    { count: 0 },
    { count: 11 },
    { request_id: "../id" },
    { reference_images: [{ type: "path", path: "" }] },
    { extra: true },
  ])("rejects invalid request %j", (changes) => {
    expect(
      requestSchema.safeParse({ prompt: "draw", ...changes }).success,
    ).toBe(false);
  });

  it("applies flags before environment before file, retaining profile isolation and Unicode", async () =>
    workspace(async (root) => {
      const config = configuration(root, {
        a: connection(),
        b: connection("gemini"),
      });
      const path = join(root, "config.json");
      await writeFile(path, JSON.stringify(config), "utf8");
      const loaded = await loadConfig(
        [
          "--config",
          path,
          "--output-dir",
          join(root, "flag"),
          "--log-level",
          "DEBUG",
          "--request-timeout-ms",
          "900",
          "--default-connection",
          "b",
        ],
        {
          NUMERA_OUTPUT_DIR: join(root, "env"),
          NUMERA_STATE_DIR: join(root, "environment-state"),
          NUMERA_LOG_LEVEL: "ERROR",
          NUMERA_REQUEST_TIMEOUT_MS: "1000",
        },
      );
      expect(loaded.outputDir).toBe(join(root, "flag"));
      expect(loaded.stateDir).toBe(join(root, "environment-state"));
      expect(loaded.logging.level).toBe("DEBUG");
      expect(
        Object.values(loaded.connections).map((c) => c.requestTimeoutMs),
      ).toEqual([900, 900]);
      expect(selectConnection(loaded)[0]).toBe("b");
      expect(request().prompt).toBe("Draw a café");
      expect(config.connections.a?.requestTimeoutMs).toBe(300000);
    }));

  it.each([
    { defaultConnection: "absent" },
    { outputDir: "relative" },
    {
      connections: {
        local: connection("openai-images", { auth: { type: "bearer" } }),
      },
    },
    {
      connections: {
        local: connection("openai-images", {
          auth: {
            type: "bearer",
            secretEnv: "EXAMPLE",
            secretFile: "/example",
          },
        }),
      },
    },
    {
      connections: {
        local: connection("openai-images", {
          auth: {
            type: "bearer",
            secretEnv: "EXAMPLE",
            origin: "https://other.example",
          },
        }),
      },
    },
    {
      connections: {
        local: connection("openai-images", {
          auth: { type: "header", secretEnv: "EXAMPLE", header: "Cookie" },
        }),
      },
    },
  ])("rejects unsafe loaded configuration %j", async (changes) =>
    workspace(async (root) => {
      const path = join(root, "config.json");
      await writeFile(
        path,
        JSON.stringify({ ...configuration(root), ...changes }),
        "utf8",
      );
      await expect(loadConfig(["--config", path], {})).rejects.toMatchObject({
        code: "invalid_configuration",
      });
    }),
  );

  it("rejects unreadable, non-JSON and relative config paths and invalid overrides", async () =>
    workspace(async (root) => {
      await expect(
        loadConfig(["--config", "relative.json"], {}),
      ).rejects.toMatchObject({ code: "invalid_configuration" });
      await expect(
        loadConfig(["--config", join(root, "absent.json")], {}),
      ).rejects.toMatchObject({ code: "invalid_configuration" });
      const path = join(root, "config.json");
      await writeFile(path, "{", "utf8");
      await expect(loadConfig(["--config", path], {})).rejects.toMatchObject({
        code: "invalid_configuration",
      });
      await writeFile(path, JSON.stringify(configuration(root)), "utf8");
      await expect(
        loadConfig(["--config", path, "--request-timeout-ms", "NaN"], {}),
      ).rejects.toMatchObject({ code: "invalid_configuration" });
    }));

  it("creates compatibility configuration only when explicitly selected", async () =>
    workspace(async (root) => {
      const env = {
        LOCALAPPDATA: root,
        XDG_CONFIG_HOME: root,
        OPENAI_BASE_URL: "https://compat.example/v1",
        OPENAI_MODEL: "image-model",
      };
      const config = await loadConfig(["--openai-compat"], env);
      expect(userRoot(env)).toContain(root);
      expect(config.connections["openai-compat"]).toMatchObject({
        baseUrl: "https://compat.example/v1",
        defaultModel: "image-model",
        auth: { secretEnv: "OPENAI_API_KEY" },
      });
      expect(Object.keys(config.connections)).toEqual(["openai-compat"]);
      await expect(
        loadConfig(["--config", join(root, "missing")], env),
      ).rejects.toMatchObject({ code: "invalid_configuration" });
    }));

  it("requires an enabled selected connection and never falls back to another profile", () => {
    const config = configuration(join(process.cwd(), ".ci-work", "selection"), {
      a: connection(),
      b: connection("gemini", { enabled: false }),
    });
    expect(selectConnection(config, "a")[1].adapter).toBe("openai-images");
    for (const name of ["b", "missing"])
      expect(() => selectConnection(config, name)).toThrow("enabled");
    delete config.defaultConnection;
    expect(() => selectConnection(config)).toThrow("enabled");
  });

  it("uses only the selected secret source without cross-profile or global fallback", async () => {
    const a = connection("openai-images", {
      auth: { type: "bearer", secretEnv: "EXAMPLE_A" },
    });
    const b = connection("gemini", {
      auth: { type: "header", header: "x-api-key", secretEnv: "EXAMPLE_B" },
    });
    const env = {
      EXAMPLE_A: "fixture-A",
      EXAMPLE_B: "fixture-B",
      OPENAI_API_KEY: "fixture-unrelated",
    };
    expect(await credentials(a, env)).toEqual({
      Authorization: "Bearer fixture-A",
    });
    expect(await credentials(b, env)).toEqual({ "x-api-key": "fixture-B" });
    await expect(
      credentials(a, {
        EXAMPLE_B: "fixture-B",
        OPENAI_API_KEY: "fixture-unrelated",
      }),
    ).rejects.toMatchObject({ code: "missing_credentials" });
    expect(await credentials(connection(), env)).toEqual({});
    for (const invalid of ["", "line\nbreak", "line\rbreak", "null\0byte"])
      await expect(
        credentials(a, { EXAMPLE_A: invalid }),
      ).rejects.toMatchObject({ code: "missing_credentials" });
  });

  it("reads only bounded declared private fixture secret files", async () =>
    workspace(async (root) => {
      const path = join(root, "fixture-secret.txt");
      await writeFile(path, "  fixture-secret  \n", {
        encoding: "utf8",
        mode: 0o600,
      });
      await chmod(path, 0o600);
      const profile = connection("openai-images", {
        auth: { type: "bearer", secretFile: path },
      });
      expect(await credentials(profile, {})).toEqual({
        Authorization: "Bearer fixture-secret",
      });
      await writeFile(path, "x".repeat(65537), "utf8");
      await expect(credentials(profile, {})).rejects.toMatchObject({
        code: "missing_credentials",
      });
      await expect(
        credentials(
          connection("openai-images", {
            auth: { type: "bearer", secretFile: "relative.txt" },
          }),
          {},
        ),
      ).rejects.toMatchObject({ code: "invalid_configuration" });
      await expect(
        credentials(
          connection("openai-images", {
            auth: { type: "bearer", secretFile: root },
          }),
          {},
        ),
      ).rejects.toMatchObject({ code: "missing_credentials" });
    }));

  it("checks storage readiness without probing generation or any host", async () =>
    workspace(async (root) => {
      const config = configuration(root);
      await mkdir(root, { recursive: true });
      const result = await health(config);
      expect(result.storage).toEqual({
        outputs: true,
        state: true,
        logs: true,
      });
      expect(result.connections).toEqual([
        {
          name: "local",
          reachable: null,
          authentication_checked: false,
          generation_verified: false,
          error: null,
        },
      ]);
      expect(result.generation_verified).toBe(false);
    }));
});

describe("effective request policy", () => {
  it("merges profile/model/request preferences without mutating shared defaults", () => {
    const profile = connection("openai-images", {
      defaults: { quality: "low", size: "small" },
      modelOverrides: { model: { defaults: { quality: "high" }, maxCount: 3 } },
      extensions: { strength: 0.2 },
      providerOptionKeys: ["strength"],
    });
    const result = effectiveRequest(
      profile,
      request({ size: "large", count: 2, provider_options: { strength: 0.8 } }),
      "model",
    );
    expect(result).toMatchObject({
      size: "large",
      quality: "high",
      count: 2,
      provider_options: { strength: 0.8 },
    });
    expect(profile.defaults).toEqual({ quality: "low", size: "small" });
    expect(profile.extensions).toEqual({ strength: 0.2 });
  });

  it.each([
    "model",
    "authorization",
    "api_key",
    "endpoint",
    "upload",
    "constructor",
  ])("rejects provider option %s at any nesting level", (key) => {
    expect(() => safeOptions({ nested: { [key]: "fixture" } })).toThrow();
  });

  it("allows bounded scalar options but rejects unapproved keys, nested arrays and oversized objects", () => {
    expect(
      safeOptions(
        {
          strength: 0.5,
          choices: [1, "two", null],
          nested: { enabled: false },
        },
        ["strength", "choices", "nested"],
      ),
    ).toEqual({
      strength: 0.5,
      choices: [1, "two", null],
      nested: { enabled: false },
    });
    expect(() => safeOptions({ strength: 0.5 }, [])).toThrow();
    expect(() => safeOptions({ choices: [{ value: 1 }] })).toThrow();
    expect(() => safeOptions({ choices: Array(33).fill(1) })).toThrow();
    expect(() =>
      safeOptions(
        Object.fromEntries(Array.from({ length: 51 }, (_, i) => [`k${i}`, i])),
      ),
    ).toThrow();
    let nested: Record<string, unknown> = { value: 1 };
    for (let i = 0; i < 8; i++) nested = { nested };
    expect(() => safeOptions(nested)).toThrow();
  });

  it.each([
    [
      { provider_options: { quality: "low" }, quality: "high" },
      { providerOptionKeys: ["quality"] },
      "invalid_input",
    ],
    [
      { output_format: "jpeg", background: "transparent" },
      {},
      "unsupported_parameter",
    ],
    [{ count: 2 }, {}, "unsupported_parameter"],
    [
      {
        reference_images: [
          { type: "url", url: "https://fixture.example/image.png" },
        ],
      },
      {},
      "unsupported_operation",
    ],
    [
      { mask: { type: "path", path: "/mask.png" } },
      {},
      "unsupported_operation",
    ],
  ] as const)(
    "rejects unsupported or conflicting effective inputs %j",
    (changes, overrides, code) => {
      expect(() =>
        effectiveRequest(
          connection("openai-images", overrides),
          request(changes),
          "model",
        ),
      ).toThrow(expect.objectContaining({ code }));
    },
  );

  it("enforces model allow/deny, reference limits and verified mask polarity", () => {
    for (const overrides of [
      { allowedModels: ["another"] },
      { deniedModels: ["model"] },
    ])
      expect(() =>
        effectiveRequest(connection("gemini", overrides), request(), "model"),
      ).toThrow(expect.objectContaining({ code: "model_unavailable" }));
    const profile = connection("openai-images", {
      edit: {
        mode: "multipart",
        encoding: "image",
        masks: true,
        maxReferences: 1,
      },
    });
    expect(() =>
      effectiveRequest(
        profile,
        request({ mask: { type: "path", path: "/mask.png" } }),
        "model",
      ),
    ).toThrow("polarity");
    expect(() =>
      effectiveRequest(
        profile,
        request({
          reference_images: [
            { type: "path", path: "/a.png" },
            { type: "path", path: "/b.png" },
          ],
        }),
        "model",
      ),
    ).toThrow("Too many");
    profile.edit!.maskPolarity = "transparent-edit";
    expect(
      effectiveRequest(
        profile,
        request({ mask: { type: "path", path: "/mask.png" } }),
        "model",
      ).mask,
    ).toBeDefined();
  });

  it("reports gateway and workflow forwarding separately from unknown account/model support", () => {
    expect(
      capabilities(
        connection("openai-images", { gateway: "omniroute" }),
        "codex/model",
      ),
    ).toMatchObject({
      max_references: 8,
      gateway_forwarding: { image_to_image: "supported", masks: "unsupported" },
      account_availability: "unknown",
    });
    expect(
      capabilities(
        connection("openai-images", { gateway: "9router" }),
        "codex/model",
      ),
    ).toMatchObject({
      max_references: 1,
      gateway_forwarding: { image_to_image: "supported" },
    });
    expect(
      capabilities(connection("comfyui"), "workflow").gateway_forwarding
        .image_to_image,
    ).toBe("unsupported");
    const profile = connection("gemini", {
      modelOverrides: {
        model: {
          maxCount: 4,
          supportedParameters: ["quality"],
          capabilities: { text_to_image: "supported" },
          evidence: "fixture docs",
        },
      },
    });
    expect(capabilities(profile, "model")).toMatchObject({
      max_count: 4,
      parameters: ["quality"],
      evidence: [{ kind: "configured" }],
    });
    expect(() =>
      effectiveRequest(
        connection("openai-images", {
          gateway: "omniroute",
          modelOverrides: { "codex/model": { maxCount: 3 } },
        }),
        request({ count: 2 }),
        "codex/model",
      ),
    ).toThrow("fans out");
  });
});
