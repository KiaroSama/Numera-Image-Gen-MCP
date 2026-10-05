import type { Config, ImageRequest } from "../config/schema.js";
import type { Normalized } from "../adapters/types.js";
import { decodeBase64, dataUrl, inspectImage } from "../files/images.js";
import { saveImage } from "../files/output.js";
import { fetchAsset } from "../http/assets.js";
import { fail, safeError } from "../errors.js";
import type { Store, Receipt } from "../jobs/store.js";
import type { Logger } from "../logging.js";
import {
  outputDeviations,
  outputRequirementWarnings,
  snapshotRequirements,
} from "./output-requirements.js";

export async function retainImage(
  config: Config,
  store: Store,
  id: string,
  index: number,
  bytes: Buffer,
  receipt?: Receipt,
) {
  const image = await inspectImage(bytes, config);
  store.retainResult(id, index, image.bytes, receipt);
}

export async function finalizeResults(
  config: Config,
  store: Store,
  logger: Logger,
  receipt: Receipt,
  result: Normalized | undefined,
  request: ImageRequest,
  signal: AbortSignal,
  recovery = false,
): Promise<Receipt> {
  const id = receipt.request_id;
  // Legacy callers/receipts still acquire a reservation before retaining their first result.
  store.reserveResults(
    id,
    config.files.maxAggregateBytes,
    config.files.maxJournalBytes,
  );
  if (!store.owns(id, receipt))
    fail(
      "outcome_unknown",
      "Request writer generation changed before output recovery.",
      "storage",
    );
  if (!receipt.output_requirements) {
    const connection = config.connections[receipt.connection];
    receipt.output_requirements = {
      count: request.count,
      size: request.size,
      output_format: request.output_format,
      background: request.background,
      output_subdirectory: request.output_subdirectory,
      filename_prefix: request.filename_prefix,
      ...(!recovery && connection
        ? snapshotRequirements(connection, receipt.requested_model, request)
        : {}),
    };
  }
  receipt.output_requirements.output_subdirectory ??= receipt.created_at.slice(
    0,
    10,
  );
  receipt.status = "finalizing";
  receipt.generation_outcome = receipt.upstream_terminal ?? "completed";
  receipt.storage_outcome = "writing";
  // Retry local storage, not generation; old storage errors stop being current after recovery.
  receipt.errors = receipt.errors.filter(
    (error) =>
      ![
        "output_file_error",
        "cancelled",
        "request_timeout",
        "upstream_error",
      ].includes(String((error as { code?: string })?.code)),
  );
  receipt.deviations = receipt.deviations.filter(
    (text) => !text.startsWith("Requested "),
  );
  if (result) {
    receipt.upstream_model = result.upstreamModel;
    receipt.upstream_request_id =
      result.upstreamId ?? receipt.upstream_request_id;
    receipt.usage = result.usage;
    receipt.warnings.push(
      ...result.warnings.filter(
        (warning) => !receipt.warnings.includes(warning),
      ),
    );
  }
  receipt.warnings.push(
    ...outputRequirementWarnings(receipt).filter(
      (warning) => !receipt.warnings.includes(warning),
    ),
  );
  if (result) {
    if (result.images.length > 10)
      fail(
        "invalid_response",
        "Provider returned too many final image items.",
        "response",
      );
    // Retain all completed inline images before cancellation or any filesystem publication.
    for (const [index, item] of result.images.entries()) {
      try {
        if (item.error) fail("invalid_response", item.error, "response");
        if (item.bytes || item.base64 || item.url?.startsWith("data:")) {
          const bytes =
            item.bytes ??
            (item.base64
              ? decodeBase64(item.base64, config.files.maxOutputBytes)
              : dataUrl(item.url!, config.files.maxOutputBytes));
          await retainImage(config, store, id, index, bytes, receipt);
        } else if (item.url) store.retainUrl(id, index, item.url, receipt);
        else
          fail(
            "invalid_response",
            "Image item has no usable bytes or continuation URL.",
            "response",
          );
      } catch (error) {
        if (!store.owns(id, receipt)) throw error;
        receipt.errors.push(safeError(error));
      }
    }
  }
  store.update(receipt);
  for (const item of store.pendingResults(id)) {
    try {
      signal.throwIfAborted();
      if (!recovery && store.cancelled(id))
        fail(
          "cancelled",
          "Local output recovery was cancelled; completed image bytes remain retained.",
          "storage",
        );
      let bytes = item.bytes;
      if (!bytes) {
        if (!item.url)
          fail(
            "output_file_error",
            "Retained image item has no recoverable bytes or URL.",
            "storage",
          );
        bytes = await fetchAsset(
          item.url,
          config,
          config.files.maxOutputBytes,
          signal,
        );
        await retainImage(config, store, id, item.index, bytes, receipt);
      }
      const image = await inspectImage(bytes, config);
      await saveImage(
        image,
        config,
        store,
        id,
        item.index,
        receipt.output_requirements.output_subdirectory,
        receipt.output_requirements.filename_prefix,
        {
          stable: true,
          receipt,
          deviations: outputDeviations(receipt, image, item.index),
        },
      );
      const current = store.get(id);
      receipt.outputs = current.outputs;
      receipt.deviations = current.deviations;
    } catch (error) {
      if (!store.owns(id, receipt)) throw error;
      receipt.errors.push(safeError(error));
      if (signal.aborted || (!recovery && store.cancelled(id))) break;
    }
  }
  if (receipt.outputs.length !== request.count)
    receipt.deviations.push(
      `Requested ${request.count} image(s), saved ${receipt.outputs.length}.`,
    );
  const pending = store.hasPendingResults(id);
  receipt.status = !receipt.outputs.length
    ? "failed"
    : receipt.errors.length || receipt.deviations.length || pending
      ? "partial"
      : "completed";
  receipt.storage_outcome =
    receipt.errors.length || pending ? "partial_or_failed" : "completed";
  store.update(receipt);
  logger.log("INFO", "generation", "Outputs processed.", {
    requestId: id,
    status: receipt.status,
    count: receipt.outputs.length,
    retained: pending,
  });
  return receipt;
}
