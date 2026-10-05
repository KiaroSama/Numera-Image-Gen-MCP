import type { Config } from "../config/schema.js";
import { selectConnection } from "../config/load.js";
import { apiJson } from "../http/client.js";
import { comfyStatus } from "../adapters/comfyui.js";
import { normalize } from "../adapters/normalize.js";
import { connectionPrefix } from "./discovery.js";
import { requestSchema } from "../config/schema.js";
import type { Generation } from "./generation.js";
import { fail, record, safeError } from "../errors.js";
import { connectionIdentity } from "./connection-identity.js";
import { retainImage } from "./result-finalization.js";
import { Store } from "../jobs/store.js";
export async function getJob(
  generation: Generation,
  id: string,
  refresh = false,
  signal?: AbortSignal,
) {
  if (refresh) generation.store.queued();
  let receipt = generation.store.get(id);
  if (!refresh) return receipt;
  // Local completed bytes are recoverable even after the selected connection was removed.
  if (generation.store.needsLocalRecovery(id))
    return generation.recoverLocal(id, signal);
  if (
    !receipt.upstream_job ||
    ["completed", "partial", "failed"].includes(receipt.status)
  )
    return receipt;
  const [, c] = selectConnection(generation.config, receipt.connection);
  if (
    (await connectionIdentity(receipt.connection, c)) !==
    generation.store.identity(id)
  )
    fail(
      "permission_denied",
      "Existing job destination/account identity changed; network recovery is not authorized.",
    );
  if (!generation.store.claimFinalization(id)) return generation.store.get(id);
  receipt = generation.store.get(id);
  const job = receipt.upstream_job!,
    controller = new AbortController();
  const combined = AbortSignal.any([
    controller.signal,
    AbortSignal.timeout(c.requestTimeoutMs),
    ...(signal ? [signal] : []),
  ]);
  const leaseWatch = setInterval(() => {
    try {
      if (!generation.store.renew(id, receipt)) controller.abort();
    } catch {
      controller.abort();
    }
  }, Store.leaseMs / 3);
  leaseWatch.unref();
  try {
    generation.store.reserveResults(
      id,
      generation.config.files.maxAggregateBytes,
      generation.config.files.maxJournalBytes,
    );
    const result =
      job.kind === "comfyui"
        ? await comfyStatus(
            c,
            job.id,
            combined,
            async (index, bytes) => {
              await retainImage(
                generation.config,
                generation.store,
                id,
                index,
                bytes,
                receipt,
              );
            },
            generation.config.files.maxOutputBytes,
            generation.config.files.maxAggregateBytes,
          )
        : normalize(
            c,
            await apiJson(
              c,
              `${job.kind}/${encodeURIComponent(job.id)}`,
              connectionPrefix(c),
              {},
              combined,
            ),
          );
    if (result && !result.job)
      return await generation.finish(
        receipt,
        result,
        requestSchema.parse({
          prompt: "recovered",
          ...Object.fromEntries(
            Object.entries(receipt.output_requirements ?? {}).filter(
              ([key]) =>
                !["upscale_source", "dimension_evidence"].includes(key),
            ),
          ),
        }),
        combined,
        true,
      );
    receipt.status = "running";
    generation.store.update(receipt);
    return receipt;
  } catch (error) {
    if (!generation.store.owns(id, receipt)) return generation.store.get(id);
    receipt = generation.store.get(id);
    receipt.errors.push(safeError(error));
    receipt.status = generation.store.hasPendingResults(id)
      ? "failed"
      : "running";
    generation.store.update(receipt);
    return receipt;
  } finally {
    clearInterval(leaseWatch);
    generation.store.release(id, receipt);
  }
}
export async function cancelJob(
  config: Config,
  generation: Generation,
  id: string,
  signal?: AbortSignal,
) {
  const receipt = generation.store.get(id);
  generation.store.cancel(id);
  generation.active.get(id)?.abort();
  let upstream_requested = false,
    upstream_cancelled: boolean | null = null;
  if (receipt.upstream_job) {
    const [, c] = selectConnection(config, receipt.connection);
    if (
      (await connectionIdentity(receipt.connection, c)) !==
      generation.store.identity(id)
    )
      fail(
        "permission_denied",
        "Existing job destination/account identity changed; cancellation is not authorized.",
      );
    const job = receipt.upstream_job;
    if (job.kind === "comfyui") {
      const result = record(
        await apiJson(
          c,
          `api/jobs/${encodeURIComponent(job.id)}/cancel`,
          "",
          {
            method: "POST",
            body: "{}",
            headers: { "Content-Type": "application/json" },
          },
          signal,
        ),
      );
      upstream_requested = true;
      upstream_cancelled = result.cancelled === true;
    }
  }
  return {
    request_id: id,
    local_wait_cancelled: true,
    upstream_requested,
    upstream_cancelled,
    refund_verified: false,
    another_submission_may_charge: true,
  };
}
