import { expect, it, vi } from "vitest";
import { readFile } from "node:fs/promises";
import {
  configuration,
  imageBytes,
  receipt,
  workspace,
} from "../fixtures/runtime.js";
import { Store } from "../../src/jobs/store.js";
import { Logger } from "../../src/logging.js";
import { Generation } from "../../src/services/generation.js";
import { getJob } from "../../src/services/jobs.js";
import { requestSchema } from "../../src/config/schema.js";
import { snapshotRequirements } from "../../src/services/output-requirements.js";

it.each([false, true])(
  "keeps original bytes and identical aspect/tier deviations after recovered=%s",
  async (recover) =>
    workspace(async (root) => {
      const config = configuration(root),
        store = new Store(config.stateDir),
        logger = new Logger(config.logging.directory, "ERROR");
      const bytes = await imageBytes();
      const request = requestSchema.parse({
        prompt: "draw",
        aspect_ratio: "16:9",
        image_size: "4K",
      });
      const initial = {
        ...receipt(),
        output_requirements: {
          count: 1,
          ...snapshotRequirements(
            config.connections.local!,
            "gemini-3.1-flash-image",
            request,
          ),
        },
      };
      try {
        store.prepare(initial, "hash", "identity");
        const commit = recover
          ? vi.spyOn(store, "commitResult").mockImplementationOnce(() => {
              throw new Error("Scheduled storage failure");
            })
          : undefined;
        let result = await new Generation(config, store, logger).finish(
          initial,
          { images: [{ bytes }], upstreamModel: null, warnings: [] },
          request,
          new AbortController().signal,
        );
        commit?.mockRestore();
        if (recover) {
          expect(store.hasPendingResults(initial.request_id)).toBe(true);
          result = await getJob(
            new Generation({ ...config, connections: {} }, store, logger),
            initial.request_id,
            true,
          );
        }
        expect(result.status).toBe("partial");
        expect(result.deviations).toEqual([
          "Output 0 aspect ratio differs from requested aspect_ratio.",
          "Output 0 dimensions differ from the documented requested resolution tier.",
        ]);
        expect(result.outputs).toHaveLength(1);
        expect(await readFile(result.outputs[0]!.path)).toEqual(bytes);
        expect(store.listOutputs()).toHaveLength(1);
        expect(store.hasPendingResults(initial.request_id)).toBe(false);
        expect(store.get(initial.request_id).output_requirements).toMatchObject(
          { aspect_ratio: "16:9", image_size: "4K" },
        );
        expect(
          await getJob(
            new Generation(config, store, logger),
            initial.request_id,
            true,
          ),
        ).toEqual(result);
      } finally {
        store.close();
        logger.close();
      }
    }),
);
