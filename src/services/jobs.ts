import type { Config } from "../config/schema.js";
import { selectConnection } from "../config/load.js";
import { apiJson } from "../http/client.js";
import { comfyStatus } from "../adapters/comfyui.js";
import { normalize } from "../adapters/normalize.js";
import { connectionPrefix } from "./discovery.js";
import { requestSchema } from "../config/schema.js";
import type { Generation } from "./generation.js";
import { record } from "../errors.js";
export async function getJob(
  generation: Generation,
  id: string,
  refresh = false,
  signal?: AbortSignal,
) {
  const receipt = generation.store.get(id);
  if (
    !refresh ||
    !receipt.upstream_job ||
    ["completed", "partial", "failed"].includes(receipt.status)
  )
    return receipt;
  const [, c] = selectConnection(generation.config, receipt.connection),
    job = receipt.upstream_job,
    combined = signal ?? AbortSignal.timeout(c.requestTimeoutMs);
  const result =
    job.kind === "comfyui"
      ? await comfyStatus(c, job.id, combined)
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
  if (result && !result.job) {
    if (!generation.store.claimFinalization(id))
      return generation.store.get(id);
    return generation.finish(
      receipt,
      result,
      requestSchema.parse({
        prompt: "recovered",
        ...receipt.output_requirements,
      }),
      combined,
    );
  }
  return receipt;
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
