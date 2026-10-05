import { TerminalProviderError } from "../adapters/terminal.js";
import type { Config } from "../config/schema.js";
import { selectConnection } from "../config/load.js";
import { apiJson } from "../http/client.js";
import { comfyStatus } from "../adapters/comfyui.js";
import { normalize } from "../adapters/normalize.js";
import { connectionPrefix } from "./discovery.js";
import { requestSchema } from "../config/schema.js";
import type { Generation } from "./generation.js";
import { fail, record, safeError, NumeraError } from "../errors.js";
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
    ["completed", "partial", "failed", "cancelled"].includes(receipt.status)
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
            () => {
              receipt.generation_outcome = "completed";
              generation.store.update(receipt);
            },
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
    if (
      (result?.terminal && result.upstreamId !== job.id) ||
      (result?.upstreamId && result.upstreamId !== job.id)
    )
      fail(
        "invalid_response",
        "Polled response does not match the existing job.",
        "response",
      );
    if (result?.job && result.job.id !== job.id)
      fail("invalid_response", "Polled job identity changed.", "response");
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
    if (error instanceof TerminalProviderError)
      return generation.store.terminal(
        id,
        error.terminal,
        safeError(error),
        receipt,
        receipt.upstream_job,
      );
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
  if (["completed", "partial", "failed", "cancelled"].includes(receipt.status))
    return {
      request_id: id,
      local_wait_cancelled: false,
      upstream_requested: false,
      upstream_cancelled: receipt.upstream_terminal === "cancelled",
      refund_verified: false,
      another_submission_may_charge: true,
    };
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
      upstream_cancelled = typeof result.cancelled === "boolean" ? false : null;
      if (result.cancelled === true) {
        const status = record(
          await apiJson(
            c,
            `api/jobs/${encodeURIComponent(job.id)}`,
            "",
            {},
            signal,
          ),
        );
        if (status.id !== job.id)
          fail(
            "invalid_response",
            "Cancellation status does not match the existing job.",
          );
        if (status.status === "completed")
          await getJob(generation, id, true, signal);
        if (["cancelled", "failed"].includes(String(status.status))) {
          const state = status.status as "cancelled" | "failed";
          generation.store.terminal(
            id,
            state,
            safeError(
              new NumeraError(
                "provider_rejection",
                "Provider confirmed terminal job state.",
                "generation",
              ),
            ),
            undefined,
            job,
          );
          upstream_cancelled =
            generation.store.get(id).upstream_terminal === "cancelled";
        }
      }
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
