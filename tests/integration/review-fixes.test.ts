import { describe, it, expect } from "vitest";
import { readFile } from "node:fs/promises";
import { Store } from "../../src/jobs/store.js";
import { Generation } from "../../src/services/generation.js";
import { connectionIdentity } from "../../src/services/connection-identity.js";
import { getJob } from "../../src/services/jobs.js";
import { Logger } from "../../src/logging.js";
import { requestSchema } from "../../src/config/schema.js";
import { effectiveRequest } from "../../src/capabilities.js";
import { buildRequest, normalize } from "../../src/adapters/index.js";
import {
  workspace,
  connection,
  configuration,
  receipt,
  imageBytes,
  server,
  json,
} from "../fixtures/runtime.js";

describe("review regressions", () => {
  it.each([
    "gemini",
    "gemini-interactions",
    "openai-responses",
    "chat-images",
  ] as const)("rejects ignored native batch for %s", (adapter) => {
    const c = connection(adapter, {
      modelOverrides: { model: { maxCount: 3 } },
    });
    expect(() =>
      effectiveRequest(
        c,
        requestSchema.parse({ prompt: "draw", count: 3 }),
        "model",
      ),
    ).toThrow("batch field");
  });
  it.each([
    ["omniroute", "codex/model", "output_format", "webp"],
    ["9router", "antigravity/model", "quality", "high"],
  ] as const)(
    "rejects non-forwarded %s field",
    (gateway, model, key, value) => {
      const c = connection("openai-images", { gateway });
      const r = requestSchema.parse({ prompt: "draw", [key]: value });
      expect(() =>
        buildRequest({
          connection: c,
          request: r,
          model,
          operation: "generate",
          references: [],
        }),
      ).toThrow("unsupported");
    },
  );
  it.each(["gemini-interactions", "openai-responses"] as const)(
    "%s needs a real async job ID",
    (adapter) => {
      expect(() =>
        normalize(connection(adapter), { status: "in_progress" }),
      ).toThrow("recoverable job ID");
    },
  );
  it("restores original output requirements and admits one concurrent finalizer", async ({
    signal,
  }) =>
    workspace(async (root) => {
      const bytes = await imageBytes();
      let downloads = 0;
      await server(
        (req, res) => {
          downloads++;
          json(res, {
            id: "owned",
            status: "completed",
            output: [
              {
                type: "image_generation_call",
                status: "completed",
                result: bytes.toString("base64"),
              },
              {
                type: "image_generation_call",
                status: "completed",
                result: bytes.toString("base64"),
              },
            ],
          });
        },
        async (origin) => {
          const config = configuration(root, {
            local: connection("openai-responses", { baseUrl: origin + "/v1" }),
          });
          const store1 = new Store(config.stateDir),
            store2 = new Store(config.stateDir),
            log = new Logger(config.logging.directory, "ERROR");
          try {
            const initial = {
              ...receipt(),
              status: "running",
              upstream_job: { id: "owned", kind: "responses" },
              output_requirements: {
                count: 2,
                output_subdirectory: "requested",
                filename_prefix: "original",
                output_format: "png" as const,
                size: "8x6",
              },
            };
            store1.prepare(
              initial,
              "hash",
              await connectionIdentity("local", config.connections.local!),
            );
            store1.release(initial.request_id);
            const results = await Promise.all([
              getJob(
                new Generation(config, store1, log),
                initial.request_id,
                true,
                signal,
              ),
              getJob(
                new Generation(config, store2, log),
                initial.request_id,
                true,
                signal,
              ),
            ]);
            const final = store1.get(initial.request_id);
            expect(final.status).toBe("completed");
            expect(final.outputs).toHaveLength(2);
            expect(final.deviations).toEqual([]);
            expect(store1.listOutputs()).toHaveLength(2);
            for (const output of final.outputs) {
              expect(output.path).toContain("requested");
              expect(output.path).toContain("original-");
              expect(await readFile(output.path)).toEqual(bytes);
            }
            expect(results.some((r) => r.status === "completed")).toBe(true);
            expect(downloads).toBe(1);
          } finally {
            store1.close();
            store2.close();
            log.close();
          }
        },
      );
    }));
  it("rejects excessive final items before processing URLs", async ({
    signal,
  }) =>
    workspace(async (root) => {
      const config = configuration(root),
        store = new Store(config.stateDir),
        log = new Logger(config.logging.directory, "ERROR");
      try {
        const initial = receipt();
        store.prepare(initial, "hash", "identity");
        await expect(
          new Generation(config, store, log).finish(
            initial,
            {
              images: Array.from({ length: 11 }, () => ({
                url: "http://127.0.0.1/never-fetch",
              })),
              upstreamModel: null,
              warnings: [],
            },
            requestSchema.parse({ prompt: "draw" }),
            signal,
          ),
        ).rejects.toMatchObject({
          code: "invalid_response",
          message: "Provider returned too many final image items.",
        });
        expect(store.listOutputs()).toEqual([]);
      } finally {
        store.close();
        log.close();
      }
    }));
});
