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
  decodeBase64,
  dataUrl,
  validateMask,
  type Image,
} from "../files/images.js";
import { inputFile, safeRelative } from "../files/paths.js";
import { ownedImage, saveImage } from "../files/output.js";
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
        output_subdirectory: request.output_subdirectory,
        filename_prefix: request.filename_prefix,
      },
      created_at: now,
      updated_at: now,
    };
    const prepared = this.store.prepare(initial, hash, identity);
    if (!prepared.fresh) return prepared.receipt;
    const receipt = prepared.receipt;
    this.active.set(id, controller);
    const cancelWatch = setInterval(() => {
      if (this.store.cancelled(id)) controller.abort();
    }, 200);
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
      receipt.status = "submitting";
      receipt.generation_outcome = "unknown";
      this.store.update(receipt);
      this.logger.log("INFO", "generation", "Submission intent persisted.", {
        requestId: id,
        connection: name,
      });
      let normalized: Normalized;
      if (workflow) {
        const submitted = await submitComfy(input, workflow, combined);
        receipt.upstream_job = { id: submitted.id, kind: submitted.kind };
        receipt.warnings.push(...submitted.warnings);
        receipt.status = "running";
        receipt.generation_outcome = "running";
        this.store.update(receipt);
        if (!request.wait) return receipt;
        normalized = await waitComfy(c, receipt.upstream_job.id, combined);
        if (!this.store.claimFinalization(id)) return this.store.get(id);
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
      if (normalized.continuation)
        this.store.saveContinuation(
          id,
          fingerprint({ identity, model }),
          normalized.continuation,
        );
      return await this.finish(receipt, normalized, request, combined);
    } catch (error) {
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
      if (receipt.upstream_job && combined.aborted) {
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
    }
  }
  async finish(
    receipt: Receipt,
    result: Normalized,
    request: ImageRequest,
    signal: AbortSignal,
  ) {
    receipt.status = "finalizing";
    receipt.generation_outcome = "completed";
    receipt.storage_outcome = "writing";
    receipt.upstream_model = result.upstreamModel;
    receipt.upstream_request_id =
      result.upstreamId ?? receipt.upstream_request_id;
    receipt.usage = result.usage;
    receipt.warnings.push(...result.warnings);
    this.store.update(receipt);
    if (result.images.length > 10)
      fail(
        "invalid_response",
        "Provider returned too many final image items.",
        "response",
      );
    let total = 0;
    for (const [index, item] of result.images.entries()) {
      try {
        signal.throwIfAborted();
        if (item.error) fail("invalid_response", item.error, "response");
        const bytes =
          item.bytes ??
          (item.base64
            ? decodeBase64(item.base64, this.config.files.maxOutputBytes)
            : item.url?.startsWith("data:")
              ? dataUrl(item.url, this.config.files.maxOutputBytes)
              : await fetchAsset(
                  item.url!,
                  this.config,
                  this.config.files.maxOutputBytes,
                  signal,
                ));
        total += bytes.length;
        if (total > this.config.files.maxAggregateBytes)
          fail(
            "invalid_response",
            "Aggregate output bytes exceeded.",
            "response",
          );
        const image = await inspectImage(bytes, this.config);
        const output = await saveImage(
          image,
          this.config,
          this.store,
          receipt.request_id,
          index,
          request.output_subdirectory,
          request.filename_prefix,
        );
        receipt.outputs.push(output);
        this.store.update(receipt);
        if (
          request.output_format &&
          image.mime !== `image/${request.output_format}`
        )
          receipt.deviations.push(
            `Output ${index} format is ${image.mime}, not requested ${request.output_format}.`,
          );
        if (
          request.size &&
          /^\d+x\d+$/.test(request.size) &&
          request.size !== `${image.width}x${image.height}`
        )
          receipt.deviations.push(
            `Output ${index} dimensions differ from requested size.`,
          );
        const source = receipt.output_requirements?.upscale_source;
        if (
          source &&
          (image.width < source.width ||
            image.height < source.height ||
            (image.width === source.width && image.height === source.height))
        )
          receipt.deviations.push(
            `Output ${index} was not upscaled beyond the reference dimensions.`,
          );
        if (request.background === "transparent" && !image.alpha)
          receipt.deviations.push(`Output ${index} has no alpha channel.`);
      } catch (e) {
        receipt.errors.push(safeError(e));
      }
    }
    if (receipt.outputs.length !== request.count)
      receipt.deviations.push(
        `Requested ${request.count} image(s), saved ${receipt.outputs.length}.`,
      );
    receipt.status = !receipt.outputs.length
      ? "failed"
      : receipt.errors.length || receipt.deviations.length
        ? "partial"
        : "completed";
    receipt.storage_outcome = receipt.errors.length
      ? "partial_or_failed"
      : "completed";
    this.store.update(receipt);
    this.logger.log("INFO", "generation", "Outputs processed.", {
      requestId: receipt.request_id,
      status: receipt.status,
      count: receipt.outputs.length,
    });
    return receipt;
  }
}
