import { randomUUID } from "node:crypto";
import { setTimeout as delay } from "node:timers/promises";
import type { Config, ImageRequest } from "../config/schema.js";
import { requestSchema } from "../config/schema.js";
import { selectConnection } from "../config/load.js";
import { credentials } from "../config/credentials.js";
import { effectiveRequest } from "../capabilities.js";
import { validateDescriptors } from "./model-policy.js";
import { connectionIdentity } from "./connection-identity.js";
import { editingRequest, regionMask } from "./editing.js";
import { buildRequest, normalizeResponse } from "../adapters/index.js";
import { prepareComfy, submitComfy, waitComfy } from "../adapters/comfyui.js";
import { apiRequest, apiJson } from "../http/client.js";
import { fetchAsset } from "../http/assets.js";
import {
  inspectImage,
  dataUrl,
  validateMask,
  type Image,
} from "../files/images.js";
import { finalizeResults, retainImage } from "./result-finalization.js";
import { snapshotRequirements } from "./output-requirements.js";
import { inputFile, safeRelative } from "../files/paths.js";
import { ownedImage } from "../files/output.js";
import { fingerprint, Store, type Receipt } from "../jobs/store.js";
import type { Normalized, Operation } from "../adapters/types.js";
import { NumeraError, fail, safeError } from "../errors.js";
import type { Logger } from "../logging.js";
export class Generation {
  readonly active = new Map<string, AbortController>();
  constructor(
    readonly config: Config,
    readonly store: Store,
    private logger: Logger,
  ) {}
  private async source(
    source: ImageRequest["reference_images"][number],
    signal: AbortSignal,
  ): Promise<Image> {
    if (source.type === "output_id") {
      const image = (
        await ownedImage(this.config, this.store, source.output_id)
      ).image;
      if (image.bytes.length > this.config.files.maxInputBytes)
        fail("invalid_input", "Owned output exceeds reference byte limit.");
      return image;
    }
    const bytes =
      source.type === "path"
        ? await inputFile(
            source.path,
            this.config.files.allowedInputRoots,
            this.config.files.maxInputBytes,
          )
        : source.type === "data_url"
          ? dataUrl(source.data_url, this.config.files.maxInputBytes)
          : await fetchAsset(
              source.url,
              this.config,
              this.config.files.maxInputBytes,
              signal,
            );
    return inspectImage(bytes, this.config, this.config.files.maxInputBytes);
  }
  async run(
    raw: unknown,
    operation: Operation,
    signal?: AbortSignal,
  ): Promise<Receipt> {
    const original = editingRequest(requestSchema.parse(raw), operation),
      [name, c] = selectConnection(this.config, original.connection),
      model = original.model ?? c.defaultModel;
    if (!model)
      fail(
        "invalid_input",
        "Select a model or configure this connection defaultModel.",
      );
    const preliminary = effectiveRequest(c, original, model);
    const policy = await validateDescriptors(c, model, preliminary, signal);
    const request = policy.request,
      id = request.request_id ?? randomUUID();
    if (request.reference_images.length > this.config.files.maxReferences)
      fail("invalid_input", "Reference count exceeds configured input limit.");
    if (operation === "edit" && !request.reference_images.length)
      fail("invalid_input", "edit_image requires at least one reference.");
    if (request.output_subdirectory) safeRelative(request.output_subdirectory);
    if (
      request.filename_prefix &&
      (safeRelative(request.filename_prefix).includes("/") ||
        request.filename_prefix.includes("\\"))
    )
      fail("invalid_input", "Filename prefix cannot include directories.");
    if (!request.wait && c.adapter !== "comfyui")
      fail(
        "unsupported_operation",
        "wait=false requires a recoverable ComfyUI job.",
      );
    const controller = new AbortController(),
      deadline = AbortSignal.timeout(c.requestTimeoutMs),
      combined = AbortSignal.any([
        controller.signal,
        deadline,
        ...(signal ? [signal] : []),
      ]);
    await credentials(c);
    const references: Image[] = [];
    let total = 0;
    for (const source of request.reference_images) {
      const image = await this.source(source, combined);
      total += image.bytes.length;
      if (total > this.config.files.maxAggregateBytes)
        fail("invalid_input", "Aggregate reference bytes exceeded.");
      references.push(image);
    }
    let upscaleSource: { width: number; height: number } | undefined;
    if (request.upscale) {
      const [width, height] = request.size!.split("x").map(Number);
      const source = references[0]!;
      if (
        width! * height! > this.config.files.maxPixels ||
        width! < source.width ||
        height! < source.height ||
        (width === source.width && height === source.height)
      )
        fail(
          "invalid_input",
          "Upscale size must enlarge the reference without shrinking either dimension and fit the pixel limit.",
        );
      upscaleSource = { width: source.width, height: source.height };
    }
    const mask = request.mask
      ? await this.source(request.mask, combined)
      : request.edit_region
        ? await regionMask(request, references[0]!, c, this.config)
        : undefined;
    if (mask) {
      if (total + mask.bytes.length > this.config.files.maxAggregateBytes)
        fail("invalid_input", "Aggregate reference and mask bytes exceeded.");
      if (!references[0])
        fail("invalid_input", "Mask needs a reference image.");
      await validateMask(mask, references[0]);
    }
    const input = {
      connection: c,
      request,
      model,
      operation,
      references,
      mask,
    };
    const built = c.adapter === "comfyui" ? undefined : buildRequest(input);
    const workflow =
      c.adapter === "comfyui" ? await prepareComfy(input, combined) : undefined;
    const identity = await connectionIdentity(name, c);
    const hash = fingerprint({
      identity,
      model,
      operation,
      prompt: request.prompt,
      count: request.count,
      parameters: {
        ...request,
        request_id: undefined,
        return_mode: undefined,
        wait: undefined,
        reference_images: undefined,
        mask: undefined,
      },
      reference_hashes: references.map((i) => i.sha256),
      mask_hash: mask?.sha256,
      workflow: c.workflow,
    });
    const now = new Date().toISOString();
    const initial: Receipt = {
      request_id: id,
      connection: name,
      adapter: c.adapter,
      operation,
      status: "prepared",
      requested_model: model,
      upstream_model: null,
      outputs: [],
      warnings: [
        "Gateway/provider internal retries and fallback are outside Numera control.",
        ...policy.warnings,
      ],
      deviations: [],
      errors: [],
      generation_outcome: "not_submitted",
      storage_outcome: "not_started",
      another_submission_may_charge: true,
      output_requirements: {
        count: request.count,
        size: request.size,
        upscale_source: upscaleSource,
        output_format: request.output_format,
        background: request.background,
        output_subdirectory: request.output_subdirectory ?? now.slice(0, 10),
        filename_prefix: request.filename_prefix,
        ...snapshotRequirements(c, model, request),
      },
      created_at: now,
      updated_at: now,
    };
    const prepared = this.store.prepare(initial, hash, identity);
    if (!prepared.fresh) return prepared.receipt;
    let receipt = prepared.receipt;
    this.active.set(id, controller);
    let lastRenewal = Date.now();
    const cancelWatch = setInterval(() => {
      if (this.store.cancelled(id)) controller.abort();
      if (Date.now() - lastRenewal >= Store.leaseMs / 3) {
        try {
          if (!this.store.renew(id, receipt)) controller.abort();
        } catch {
          controller.abort();
        }
        lastRenewal = Date.now();
      }
    }, 200);
    cancelWatch.unref();
    try {
      if (this.store.queued() > this.config.maxQueuedRequests)
        fail("rate_limited", "Local bounded queue is full.");
      while (
        !this.store.admit(
          id,
          name,
          this.config.maxConcurrentRequests,
          c.adapter === "comfyui" ? 1 : c.maxConcurrentRequests,
        )
      ) {
        combined.throwIfAborted();
        await delay(50, undefined, { signal: combined });
      }
      combined.throwIfAborted();
      this.store.reserveResults(
        id,
        this.config.files.maxAggregateBytes,
        this.config.files.maxJournalBytes,
      );
      receipt.status = "submitting";
      receipt.generation_outcome = "unknown";
      this.store.update(receipt);
      this.logger.log("INFO", "generation", "Submission intent persisted.", {
        requestId: id,
        connection: name,
      });
      let normalized: Normalized;
      let finalizationClaimed = false;
      if (workflow) {
        const submitted = await submitComfy(input, workflow, combined);
        receipt.upstream_job = { id: submitted.id, kind: submitted.kind };
        receipt.warnings.push(...submitted.warnings);
        receipt.status = "running";
        receipt.generation_outcome = "running";
        this.store.update(receipt);
        if (!request.wait) return receipt;
        normalized = await waitComfy(
          c,
          receipt.upstream_job.id,
          combined,
          async (index, bytes) => {
            if (!finalizationClaimed) {
              if (!this.store.claimFinalization(id, receipt))
                fail(
                  "outcome_unknown",
                  "Another waiter owns output finalization.",
                  "storage",
                );
              finalizationClaimed = true;
              receipt = this.store.get(id);
              receipt.status = "finalizing";
            }
            await retainImage(
              this.config,
              this.store,
              id,
              index,
              bytes,
              receipt,
            );
          },
          this.config.files.maxOutputBytes,
          this.config.files.maxAggregateBytes,
        );
      } else {
        const response = await apiRequest(
          c,
          built!.operation,
          built!.prefix,
          {
            method: "POST",
            body: built!.body,
            headers: built!.headers,
            query: built!.query,
          },
          combined,
          this.config.files.maxAggregateBytes,
        );
        normalized = normalizeResponse(c, response.bytes, response.mime);
        receipt.upstream_request_id = response.requestId ?? undefined;
        if (normalized.job) {
          receipt.upstream_job = normalized.job;
          receipt.status = "running";
          this.store.update(receipt);
          while (normalized.job) {
            await delay(500, undefined, { signal: combined });
            const raw = await apiJson(
              c,
              `${normalized.job.kind}/${encodeURIComponent(normalized.job.id)}`,
              built!.prefix,
              {},
              combined,
            );
            normalized = normalizeResponse(
              c,
              Buffer.from(JSON.stringify(raw)),
              "application/json",
            );
          }
        }
      }
      if (!finalizationClaimed) {
        if (!this.store.claimFinalization(id, receipt))
          return this.store.get(id);
        finalizationClaimed = true;
        receipt = this.store.get(id);
      }
      const completed = await this.finish(
        receipt,
        normalized,
        request,
        combined,
        true,
      );
      if (normalized.continuation) {
        try {
          this.store.saveContinuation(
            id,
            fingerprint({ identity, model }),
            normalized.continuation,
          );
        } catch (error) {
          if (!this.store.owns(id, receipt)) return this.store.get(id);
          completed.warnings.push(
            "Private continuation metadata could not be retained; original outputs remain available.",
          );
          this.store.update(completed);
          this.logger.log(
            "WARNING",
            "generation",
            "Continuation storage failed after retaining outputs.",
            { code: safeError(error).code },
          );
        }
      }
      return completed;
    } catch (error) {
      if (!this.store.owns(id, receipt)) return this.store.get(id);
      const current = this.store.get(id);
      receipt = current;
      const ambiguous =
        receipt.status === "submitting" &&
        (error instanceof NumeraError
          ? [
              "outcome_unknown",
              "request_timeout",
              "invalid_response",
              "cancelled",
              "no_image_returned",
            ].includes(error.code)
          : true);
      receipt.status = receipt.outputs.length
        ? "partial"
        : ambiguous
          ? "outcome_unknown"
          : combined.aborted
            ? "cancelled"
            : "failed";
      receipt.errors.push(safeError(error));
      receipt.generation_outcome = ambiguous
        ? "unknown"
        : receipt.generation_outcome;
      if (
        receipt.upstream_job &&
        combined.aborted &&
        receipt.generation_outcome !== "completed"
      ) {
        receipt.status = "running";
        receipt.generation_outcome = "running";
        receipt.warnings.push(
          "Local waiting stopped; existing upstream job may still be running. Use get_job, never resubmit.",
        );
      }
      this.store.update(receipt);
      this.logger.log("ERROR", "generation", "Request did not complete.", {
        requestId: id,
        error: safeError(error),
      });
      return receipt;
    } finally {
      clearInterval(cancelWatch);
      this.active.delete(id);
      this.store.release(id, receipt);
    }
  }
  async finish(
    receipt: Receipt,
    result: Normalized,
    request: ImageRequest,
    signal: AbortSignal,
    claimed = false,
  ): Promise<Receipt> {
    const id = receipt.request_id;
    if (
      claimed
        ? !this.store.owns(id, receipt)
        : !this.store.claimFinalization(id, receipt)
    )
      return this.store.get(id);
    receipt = this.store.get(id);
    const leaseWatch = setInterval(() => {
      try {
        this.store.renew(id, receipt);
      } catch {
        /* Fenced writes stop a failed renewal. */
      }
    }, Store.leaseMs / 3);
    leaseWatch.unref();
    try {
      return await finalizeResults(
        this.config,
        this.store,
        this.logger,
        receipt,
        result,
        request,
        signal,
      );
    } finally {
      clearInterval(leaseWatch);
      if (!claimed) this.store.release(id, receipt);
    }
  }
  async recoverLocal(
    id: string,
    signal: AbortSignal = new AbortController().signal,
  ): Promise<Receipt> {
    if (!this.store.needsLocalRecovery(id) || !this.store.claimFinalization(id))
      return this.store.get(id);
    const receipt = this.store.get(id);
    const request = requestSchema.parse({
      prompt: "recovered",
      ...Object.fromEntries(
        Object.entries(receipt.output_requirements ?? {}).filter(
          ([key]) => !["upscale_source", "dimension_evidence"].includes(key),
        ),
      ),
    });
    const leaseWatch = setInterval(() => {
      try {
        this.store.renew(id, receipt);
      } catch {
        /* Fenced writes stop a failed renewal. */
      }
    }, Store.leaseMs / 3);
    leaseWatch.unref();
    try {
      return await finalizeResults(
        this.config,
        this.store,
        this.logger,
        receipt,
        undefined,
        request,
        signal,
        true,
      );
    } finally {
      clearInterval(leaseWatch);
      this.store.release(id, receipt);
    }
  }
}
