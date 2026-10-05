import { expect, it } from "vitest";
import { Store } from "../../src/jobs/store.js";
import { Generation } from "../../src/services/generation.js";
import { Logger } from "../../src/logging.js";
import { requestSchema } from "../../src/config/schema.js";
import {
  configuration,
  imageBytes,
  receipt,
  server,
  workspace,
} from "../fixtures/runtime.js";

it("retains completed inline bytes but starts no new asset download after cancellation", async () =>
  workspace(async (root) => {
    let downloads = 0;
    await server(
      (_request, response) => {
        downloads++;
        response.writeHead(500);
        response.end();
      },
      async (origin) => {
        const config = configuration(root),
          store = new Store(config.stateDir),
          logger = new Logger(config.logging.directory, "ERROR");
        config.files.assetAllowances = [{ origin, pathPrefix: "/image" }];
        try {
          const initial = receipt("cancel-assets");
          store.prepare(initial, "hash", "identity");
          store.cancel(initial.request_id);
          const bytes = await imageBytes();
          const result = await new Generation(config, store, logger).finish(
            initial,
            {
              images: [{ url: `${origin}/image.png` }, { bytes }],
              upstreamModel: null,
              warnings: [],
            },
            requestSchema.parse({ prompt: "draw", count: 2 }),
            AbortSignal.abort(),
          );
          expect(downloads).toBe(0);
          expect(result.outputs).toEqual([]);
          const pending = store.pendingResults(initial.request_id);
          expect(pending).toHaveLength(2);
          expect(pending[1]!.bytes).toEqual(bytes);
          expect(result.generation_outcome).toBe("completed");
        } finally {
          store.close();
          logger.close();
        }
      },
    );
  }));
