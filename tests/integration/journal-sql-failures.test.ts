import { expect, it } from "vitest";
import { DatabaseSync } from "node:sqlite";
import { join } from "node:path";
import { readFile } from "node:fs/promises";
import { Store } from "../../src/jobs/store.js";
import { Generation } from "../../src/services/generation.js";
import { getJob } from "../../src/services/jobs.js";
import { Logger } from "../../src/logging.js";
import { requestSchema } from "../../src/config/schema.js";
import {
  configuration,
  imageBytes,
  receipt,
  workspace,
} from "../fixtures/runtime.js";

it("rolls back real output-index SQL failure while retaining the published image and journal", async () =>
  workspace(async (root) => {
    const config = configuration(root),
      store = new Store(config.stateDir),
      logger = new Logger(config.logging.directory, "ERROR");
    const fault = new DatabaseSync(join(config.stateDir, "receipts.sqlite"));
    const bytes = await imageBytes();
    try {
      store.prepare(receipt("sql-fault"), "hash", "identity");
      fault.exec(
        "CREATE TRIGGER reject_output BEFORE INSERT ON outputs BEGIN SELECT RAISE(FAIL,'Controlled output-index failure'); END;",
      );
      const result = await new Generation(config, store, logger).finish(
        store.get("sql-fault"),
        { images: [{ bytes }], upstreamModel: null, warnings: [] },
        requestSchema.parse({ prompt: "draw" }),
        new AbortController().signal,
      );
      expect(result.outputs).toHaveLength(0);
      expect(store.listOutputs()).toHaveLength(0);
      const pending = store.pendingResults("sql-fault");
      expect(pending).toHaveLength(1);
      expect(await readFile(pending[0]!.target!)).toEqual(bytes);
      fault.exec("DROP TRIGGER reject_output");
      const recovered = await getJob(
        new Generation(config, store, logger),
        "sql-fault",
        true,
      );
      expect(recovered.status).toBe("completed");
      expect(store.listOutputs()).toHaveLength(1);
      expect(await readFile(recovered.outputs[0]!.path)).toEqual(bytes);
    } finally {
      fault.close();
      store.close();
      logger.close();
    }
  }));

it("refuses future state versions without changing their schema version", async () =>
  workspace(async (root) => {
    const config = configuration(root),
      initial = new Store(config.stateDir);
    initial.close();
    const database = new DatabaseSync(join(config.stateDir, "receipts.sqlite"));
    try {
      database.exec("PRAGMA user_version=3");
      expect(() => new Store(config.stateDir)).toThrow("newer than this build");
      expect(database.prepare("PRAGMA user_version").get()?.user_version).toBe(
        3,
      );
    } finally {
      database.close();
    }
  }));
