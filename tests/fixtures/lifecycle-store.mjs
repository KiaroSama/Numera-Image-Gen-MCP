import { Store } from "../../dist/jobs/store.js";
const [root, id, mode] = process.argv.slice(2);
const store = new Store(root);
try {
  if (mode === "read") {
    const r = store.get(id);
    process.send?.({
      status: r.status,
      generation_outcome: r.generation_outcome,
      outputs: r.outputs.length,
    });
  } else {
    const now = new Date().toISOString();
    store.prepare(
      {
        request_id: id,
        connection: "local",
        adapter: "openai-images",
        operation: "generate",
        status: "prepared",
        requested_model: "image-model",
        upstream_model: null,
        outputs: [],
        warnings: [],
        deviations: [],
        errors: [],
        generation_outcome: "not_submitted",
        storage_outcome: "not_started",
        another_submission_may_charge: true,
        created_at: now,
        updated_at: now,
      },
      id,
      "identity",
    );
    process.send?.({ ready: true });
    await new Promise((resolve) => process.once("message", resolve));
    try {
      store.reserveResults(id, 2048, 2048);
      process.send?.({ reserved: true });
    } catch (error) {
      process.send?.({ reserved: false, code: error.code });
    }
  }
} finally {
  store.close();
  process.disconnect?.();
}
