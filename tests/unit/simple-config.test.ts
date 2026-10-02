import { it, expect } from "vitest";
import { writeFile } from "node:fs/promises";
import { join } from "node:path";
import { loadConfig, selectConnection } from "../../src/config/load.js";
import { credentials } from "../../src/config/credentials.js";
import {
  workspace,
  server,
  json,
  configuration,
  connection,
} from "../fixtures/runtime.js";
import { Discovery } from "../../src/services/discovery.js";

const compact = {
  api_endpoint: "http://127.0.0.1:20100/v1",
  api_key: "fixture-local-key-not-real",
  models: [
    { id: "antigravity/opaque-image", name: "Image 日本語" },
    { id: "custom/another", name: "Second" },
  ],
};
it("loads endpoint, private key and named models with portable defaults", async () =>
  workspace(async (root) => {
    const file = join(root, "config.json");
    await writeFile(file, JSON.stringify(compact), "utf8");
    const config = await loadConfig(["--config", file], {});
    const [name, c] = selectConnection(config);
    expect(name).toBe("default");
    expect(c).toMatchObject({
      gateway: "omniroute",
      adapter: "openai-images",
      baseUrl: compact.api_endpoint,
      defaultModel: "antigravity/opaque-image",
      configuredModels: compact.models,
    });
    expect(config.outputDir).toBe(join(root, "outputs"));
    expect(config.stateDir).toBe(join(root, "state"));
    expect(config.logging.directory).toBe(join(root, "logs"));
    expect(config.files.allowedInputRoots).toEqual([join(root, "inputs")]);
    expect(await credentials(c, {})).toEqual({
      Authorization: "Bearer fixture-local-key-not-real",
    });
  }));

it("configured models list offline and retain names on failed refresh", async () =>
  workspace(async (root) => {
    let calls = 0;
    await server(
      async (_req, res) => {
        calls++;
        json(res, { error: "fixture" }, 503);
      },
      async (origin) => {
        const file = join(root, "config.json");
        await writeFile(
          file,
          JSON.stringify({ ...compact, api_endpoint: `${origin}/v1` }),
          "utf8",
        );
        const discovery = new Discovery(
          await loadConfig(["--config", file], {}),
        );
        const offline = await discovery.list();
        expect(calls).toBe(0);
        expect(offline.models).toMatchObject([
          {
            id: "antigravity/opaque-image",
            name: "Image 日本語",
            configured: true,
          },
          { id: "custom/another", name: "Second" },
        ]);
        const refreshed = await discovery.list(undefined, true);
        expect(calls).toBe(1);
        expect(refreshed.complete).toBe(false);
        expect(refreshed.models).toMatchObject([
          { id: "antigravity/opaque-image", name: "Image 日本語" },
          { id: "custom/another", name: "Second" },
        ]);
      },
    );
  }));

it.each([
  { api_key: " " },
  { api_key: "bad\nkey" },
  { models: [] },
  {
    models: [
      { id: "duplicate", name: "One" },
      { id: "duplicate", name: "Two" },
    ],
  },
  { models: [{ id: " ", name: "Name" }] },
  { models: [{ id: "native", name: " " }] },
  { api_endpoint: "http://user:password@localhost/v1" },
  { api_endpoint: "http://localhost/v1?key=fixture" },
  { extra: true },
  { profile: "guessed" },
])(
  "rejects unsafe compact configuration without exposing key %j",
  async (change) =>
    workspace(async (root) => {
      const file = join(root, "config.json");
      await writeFile(file, JSON.stringify({ ...compact, ...change }), "utf8");
      try {
        await loadConfig(["--config", file], {});
        throw new Error("Accepted invalid config");
      } catch (error) {
        expect(error).toMatchObject({ code: "invalid_configuration" });
        expect(String(error)).not.toContain(compact.api_key);
      }
    }),
);

it.each(["9router", "openai-images"])(
  "supports explicit %s profile and CLI overrides",
  async (profile) =>
    workspace(async (root) => {
      const file = join(root, "config.json");
      await writeFile(file, JSON.stringify({ ...compact, profile }), "utf8");
      const config = await loadConfig(
        ["--config", file, "--output-dir", join(root, "override")],
        {},
      );
      expect(config.outputDir).toBe(join(root, "override"));
      expect(selectConnection(config)[1].gateway).toBe(
        profile === "9router" ? "9router" : undefined,
      );
    }),
);

it("refresh supplements configured metadata without losing names or native IDs", async () =>
  workspace(async (root) => {
    await server(
      async (_req, res) =>
        json(res, {
          data: [
            { id: "custom/another", type: "image" },
            { id: "upstream/new", type: "image" },
          ],
        }),
      async (origin) => {
        const file = join(root, "config.json");
        await writeFile(
          file,
          JSON.stringify({ ...compact, api_endpoint: `${origin}/v1` }),
          "utf8",
        );
        const discovery = new Discovery(
          await loadConfig(["--config", file], {}),
        );
        const refreshed = await discovery.list(undefined, true);
        expect(refreshed.models.map((m) => m.id)).toEqual([
          "antigravity/opaque-image",
          "custom/another",
          "upstream/new",
        ]);
        expect(refreshed.models[1]).toMatchObject({
          name: "Second",
          metadata: { advertised: { id: "custom/another" } },
        });
        expect(
          await discovery.model(undefined, "custom/another"),
        ).toMatchObject({
          model_id: "custom/another",
          display_name: "Second",
          account_availability: "unknown",
        });
        expect((await discovery.list()).cached).toBe(true);
      },
    );
  }));

it.each([
  null,
  "text",
  [],
  { ...compact, models: [{ id: "only", name: "Name" }], api_key: "" },
])("rejects malformed config shape %j safely", async (value) =>
  workspace(async (root) => {
    const file = join(root, "config.json");
    await writeFile(file, JSON.stringify(value), "utf8");
    await expect(loadConfig(["--config", file], {})).rejects.toMatchObject({
      code: "invalid_configuration",
    });
  }),
);
it("advanced configs reject competing secret sources and duplicate configured model IDs", async () =>
  workspace(async (root) => {
    const file = join(root, "config.json");
    for (const changes of [
      { auth: { type: "bearer", apiKey: "fixture", secretEnv: "OTHER" } },
      {
        configuredModels: [
          { id: "a", name: "A" },
          { id: "a", name: "B" },
        ],
      },
    ]) {
      await writeFile(
        file,
        JSON.stringify(
          configuration(root, { local: connection("openai-images", changes) }),
        ),
        "utf8",
      );
      await expect(loadConfig(["--config", file], {})).rejects.toMatchObject({
        code: "invalid_configuration",
      });
    }
  }));
