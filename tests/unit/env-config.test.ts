import { writeFile } from "node:fs/promises";
import { join } from "node:path";
import { it, expect } from "vitest";
import { loadConfig } from "../../src/config/load.js";
import { ConfigReload } from "../../src/config/reload.js";
import { Logger } from "../../src/logging.js";
import { workspace, configuration } from "../fixtures/runtime.js";

const valid =
  'API_ENDPOINT=https://api.example/v1\nAPI_KEY="synthetic-test-key"\nMODEL_1_ID=family/image\nMODEL_1_NAME="تصویر #1"\n';

it("loads quoted UTF-8 env slots with provider defaults omitted without mutating the process environment", () =>
  workspace(async (root) => {
    const file = join(root, ".env"),
      before = { ...process.env };
    await writeFile(
      file,
      valid +
        'MODEL_1_SIZE=\nMODEL_1_QUALITY=""\nMODEL_1_OUTPUT_FORMAT=\nMODEL_2_ID=disabled\nMODEL_2_NAME=Disabled\nMODEL_2_ENABLED=false\n',
      "utf8",
    );
    const config = await loadConfig(["--config", file], {});
    expect(config.connections.default).toMatchObject({
      defaultModel: "family/image",
      configuredModels: [{ id: "family/image", name: "تصویر #1" }],
      deniedModels: ["disabled"],
      modelOverrides: { "family/image": { defaults: {} } },
    });
    expect(process.env).toEqual(before);
  }));

it.each([
  "MODEL_1_ENABLED=yes\n",
  "MODEL_101_ID=overflow\n",
  "MODEL_01_ID=zero\n",
  "MODEL_1_QUALTY=typo\n",
  "PROFILEE=typo\n",
  "API_KEY=duplicate\n",
  "broken line\n",
  'MODEL_1_SIZE="unterminated\n',
  'MODEL_1_SIZE="512x512" garbage\n',
  "EDIT_MASKS=true\n",
  "EDIT_MODE=json\nEDIT_ENCODING=images\nEDIT_MAX_REFERENCES=1.5\n",
  "EDIT_MODE=json\nEDIT_ENCODING=images\nEDIT_MAX_REFERENCES=33\n",
  "EDIT_MODE=json\nEDIT_ENCODING=images\nEDIT_MASKS=1\n",
])(
  "rejects invalid env assignments without credential-bearing errors: %s",
  (suffix) =>
    workspace(async (root) => {
      const file = join(root, ".env");
      await writeFile(file, valid + suffix, "utf8");
      await expect(loadConfig(["--config", file], {})).rejects.toMatchObject({
        code: "invalid_configuration",
      });
      try {
        await loadConfig(["--config", file], {});
      } catch (error) {
        expect(String(error)).not.toContain("synthetic-test-key");
      }
    }),
);

it("loads explicit edit routes and native Gemini credentials without guessing forwarding", () =>
  workspace(async (root) => {
    const file = join(root, "settings.env");
    await writeFile(
      file,
      valid +
        "PROFILE=gemini\nEDIT_MODE=json\nEDIT_ENCODING=images\nEDIT_MAX_REFERENCES=4\nEDIT_MASKS=true\nEDIT_MASK_POLARITY=white-edit\n",
      "utf8",
    );
    const config = await loadConfig(["--config", file], {});
    expect(config.connections.default).toMatchObject({
      adapter: "gemini",
      auth: { type: "header", header: "x-goog-api-key" },
      edit: {
        mode: "json",
        encoding: "images",
        maxReferences: 4,
        masks: true,
        maskPolarity: "white-edit",
      },
    });
  }));

it("rejects oversized and invalid UTF-8 files and accepts export/comments/multiline quoted names", () =>
  workspace(async (root) => {
    const file = join(root, ".env");
    await writeFile(file, Buffer.alloc(1024 * 1024 + 1, 32));
    await expect(loadConfig(["--config", file], {})).rejects.toMatchObject({
      code: "invalid_configuration",
    });
    await writeFile(file, Buffer.from([0xff]));
    await expect(loadConfig(["--config", file], {})).rejects.toMatchObject({
      code: "invalid_configuration",
    });
    await writeFile(
      file,
      valid.replace(
        'MODEL_1_NAME="تصویر #1"',
        'export MODEL_1_NAME="line one\nline two" # comment',
      ),
      "utf8",
    );
    expect(
      (await loadConfig(["--config", file], {})).connections.default!
        .configuredModels[0]!.name,
    ).toBe("line one\nline two");
  }));

it("retains last good advanced JSON when fixed storage changes and rejects a snapshot capacity refusal", () =>
  workspace(async (root) => {
    const file = join(root, "config.json"),
      config = configuration(root);
    await writeFile(file, JSON.stringify(config), "utf8");
    const logger = new Logger(config.logging.directory, "ERROR");
    const reload = new ConfigReload(
      config,
      file,
      ["--config", file],
      {},
      logger,
    );
    try {
      expect(await reload.refresh()).toBe(config);
      await writeFile(
        file,
        JSON.stringify({ ...config, stateDir: join(root, "different") }),
        "utf8",
      );
      expect(await reload.refresh()).toBe(config);
      await writeFile(
        file,
        JSON.stringify({ ...config, returnMode: "files_and_preview" }),
        "utf8",
      );
      expect(await reload.refresh(() => false)).toBe(config);
    } finally {
      logger.close();
    }
  }));
