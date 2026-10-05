import { readFile } from "node:fs/promises";
import { Store } from "../../dist/jobs/store.js";
import { configSchema } from "../../dist/config/schema.js";
import { inspectImage } from "../../dist/files/images.js";
import { saveImage } from "../../dist/files/output.js";

async function checkpoint() {
  if (!process.send)
    throw new Error("Crash fixture requires an owned IPC channel.");
  process.send({ ready: true });
  await new Promise(() => {});
}
const [configPath, imagePath, phase] = process.argv.slice(2);
const config = configSchema.parse(
  JSON.parse(await readFile(configPath, "utf8")),
);
const store = new Store(config.stateDir);
const receipt = {
  request_id: "child-crash",
  connection: "local",
  adapter: "openai-images",
  operation: "generate",
  status: "submitting",
  requested_model: "image-model",
  upstream_model: null,
  outputs: [],
  warnings: [],
  deviations: [],
  errors: [],
  generation_outcome: "completed",
  storage_outcome: "writing",
  another_submission_may_charge: true,
  output_requirements: { count: 1, output_subdirectory: "crash" },
  created_at: "2026-10-05T00:00:00.000Z",
  updated_at: "2026-10-05T00:00:00.000Z",
};
if (phase === "preintent") await checkpoint();
receipt.status =
  phase === "prepared"
    ? "prepared"
    : phase === "submitted"
      ? "submitting"
      : phase === "finalizing"
        ? "finalizing"
        : receipt.status;
receipt.generation_outcome =
  phase === "prepared"
    ? "not_submitted"
    : ["submitted", "finalizing"].includes(phase)
      ? "unknown"
      : receipt.generation_outcome;
if (phase === "running") {
  receipt.status = "running";
  receipt.generation_outcome = "running";
  receipt.upstream_job = { id: "existing-owned-job", kind: "responses" };
}
store.prepare(receipt, "hash", "identity");
if (["prepared", "submitted", "finalizing", "running"].includes(phase))
  await checkpoint();
store.reserveResults(
  receipt.request_id,
  config.files.maxAggregateBytes,
  config.files.maxJournalBytes,
);
const image = await inspectImage(await readFile(imagePath), config);
store.retainResult(receipt.request_id, 0, image.bytes);
store.claimFinalization(receipt.request_id);
if (phase === "committed")
  await saveImage(
    image,
    config,
    store,
    receipt.request_id,
    0,
    "crash",
    undefined,
    { stable: true },
  );
if (phase === "published") {
  store.commitResult = () => {
    throw new Error("Controlled crash before SQL receipt commit");
  };
  await saveImage(
    image,
    config,
    store,
    receipt.request_id,
    0,
    "crash",
    undefined,
    { stable: true },
  ).catch(() => {});
}
// The owning parent kills this process at the checkpoint without closing SQLite.
await checkpoint();
