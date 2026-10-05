import { describe, expect, it, vi } from "vitest";
import { randomUUID } from "node:crypto";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { fingerprint, Store, type Output } from "../../src/jobs/store.js";
import { requestSchema } from "../../src/config/schema.js";
import { Logger } from "../../src/logging.js";
import { Generation } from "../../src/services/generation.js";
import { connectionIdentity } from "../../src/services/connection-identity.js";
import { cancelJob, getJob } from "../../src/services/jobs.js";
import {
  configuration,
  connection,
  imageBytes,
  json,
  receipt,
  server,
  workspace,
} from "../fixtures/runtime.js";

const output = (id: string, requestId = "request-1"): Output => ({
  output_id: id,
  request_id: requestId,
  path: `/fixture/${id}.png`,
  mime_type: "image/png",
  width: 8,
  height: 6,
  has_alpha: true,
  bytes: 100,
  sha256: "a".repeat(64),
});

describe("effective input fingerprints", () => {
  it("uses a known canonical hash and ignores object ordering and absent fields", () => {
    expect(fingerprint({})).toBe(
      "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a",
    );
    expect(
      fingerprint({
        prompt: "café",
        options: { b: 2, a: 1 },
        absent: undefined,
      }),
    ).toBe(fingerprint({ options: { a: 1, b: 2 }, prompt: "café" }));
  });

  it("distinguishes reference order, bytes, model, endpoint and scalar types", () => {
    const initial = {
      references: ["hash-a", "hash-b"],
      model: "model-a",
      endpoint: "https://a.example/v1",
      count: 1,
    };
    const changes = [
      { references: ["hash-b", "hash-a"] },
      { references: ["hash-a", "hash-c"] },
      { model: "model-b" },
      { endpoint: "https://b.example/v1" },
      { count: "1" },
    ];
    for (const change of changes)
      expect(fingerprint({ ...initial, ...change })).not.toBe(
        fingerprint(initial),
      );
    expect(fingerprint({ value: null })).not.toBe(fingerprint({}));
  });
});

describe("generation receipt completion and owned job recovery", () => {
  it("rejects missing model, empty edits, unsafe output and unsupported wait before submitting", async ({
    signal,
  }) =>
    workspace(async (root) => {
      const options = configuration(root),
        store = new Store(options.stateDir),
        logger = new Logger(options.logging.directory, "ERROR");
      try {
        const generation = new Generation(options, store, logger);
        await expect(
          generation.run({ prompt: "draw" }, "generate", signal),
        ).rejects.toMatchObject({ code: "invalid_input" });
        await expect(
          generation.run({ prompt: "draw", model: "image" }, "edit", signal),
        ).rejects.toMatchObject({ code: "invalid_input" });
        for (const changes of [
          { output_subdirectory: "../escape" },
          { filename_prefix: "nested/prefix" },
        ])
          await expect(
            generation.run(
              { prompt: "draw", model: "image", ...changes },
              "generate",
              signal,
            ),
          ).rejects.toMatchObject({ code: "invalid_input" });
        await expect(
          generation.run(
            { prompt: "draw", model: "image", wait: false },
            "generate",
            signal,
          ),
        ).rejects.toMatchObject({ code: "unsupported_operation" });
        expect(store.queued()).toBe(0);
        expect(generation.active.size).toBe(0);
      } finally {
        logger.close();
        store.close();
      }
    }));

  it("validates actual data URL edit inputs and masks before upstream submission or storing intent", async ({
    signal,
  }) =>
    workspace(async (root) => {
      const profile = connection("openai-images", {
        defaultModel: "image",
        edit: {
          mode: "multipart",
          encoding: "image",
          masks: true,
          maskPolarity: "transparent-edit",
          maxReferences: 2,
        },
      });
      const options = configuration(root, { local: profile }),
        store = new Store(options.stateDir),
        logger = new Logger(options.logging.directory, "ERROR");
      try {
        const generation = new Generation(options, store, logger),
          png = await imageBytes();
        const reference = {
          type: "data_url",
          data_url: `data:image/png;base64,${png.toString("base64")}`,
        };
        const jpeg = await imageBytes("jpeg");
        await expect(
          generation.run(
            {
              prompt: "edit",
              reference_images: [reference],
              mask: {
                type: "data_url",
                data_url: `data:image/jpeg;base64,${jpeg.toString("base64")}`,
              },
            },
            "edit",
            signal,
          ),
        ).rejects.toMatchObject({ code: "invalid_input" });
        await expect(
          generation.run(
            { prompt: "edit", mask: reference },
            "generate",
            signal,
          ),
        ).rejects.toMatchObject({ code: "invalid_input" });
        options.files.maxAggregateBytes = png.length;
        await expect(
          generation.run(
            { prompt: "edit", reference_images: [reference, reference] },
            "edit",
            signal,
          ),
        ).rejects.toMatchObject({ code: "invalid_input" });
        await expect(
          generation.run(
            {
              prompt: "edit",
              reference_images: [
                { type: "data_url", data_url: "data:image/png;base64,YQ==" },
              ],
            },
            "edit",
            signal,
          ),
        ).rejects.toMatchObject({ code: "invalid_input" });
        expect(store.queued()).toBe(0);
        expect(generation.active.size).toBe(0);
      } finally {
        logger.close();
        store.close();
      }
    }));

  it("keeps verified output after another image fails and records format/size/alpha deviations", async ({
    signal,
  }) =>
    workspace(async (root) => {
      const options = configuration(root),
        store = new Store(options.stateDir),
        logger = new Logger(options.logging.directory, "ERROR");
      try {
        const initial = receipt();
        store.prepare(initial, "hash", "identity");
        const generation = new Generation(options, store, logger);
        const jpeg = await imageBytes("jpeg");
        const result = await generation.finish(
          initial,
          {
            images: [{ bytes: jpeg }, { error: "fixture output failure" }],
            upstreamModel: "actual-model",
            upstreamId: "upstream-1",
            usage: { images: 1 },
            warnings: ["fixture warning"],
          },
          requestSchema.parse({
            prompt: "draw",
            count: 2,
            size: "10x10",
            output_format: "png",
            background: "transparent",
          }),
          signal,
        );
        expect(result).toMatchObject({
          status: "partial",
          generation_outcome: "completed",
          storage_outcome: "partial_or_failed",
          upstream_model: "actual-model",
          upstream_request_id: "upstream-1",
          usage: { images: 1 },
        });
        expect(result.outputs).toHaveLength(1);
        expect(await readFile(result.outputs[0]!.path)).toEqual(jpeg);
        expect(result.errors).toEqual([
          expect.objectContaining({ code: "invalid_response" }),
        ]);
        expect(result.warnings).toEqual(["fixture warning"]);
        expect(result.deviations).toEqual([
          "Output 0 format is image/jpeg, not requested png.",
          "Output 0 dimensions differ from requested size.",
          "Output 0 has no alpha channel.",
          "Requested 2 image(s), saved 1.",
        ]);
        expect(store.get(initial.request_id)).toMatchObject({
          status: "partial",
          outputs: result.outputs,
        });
      } finally {
        logger.close();
        store.close();
      }
    }));

  it("enforces aggregate output limits and cancellation without erasing saved output metadata", async ({
    signal,
  }) =>
    workspace(async (root) => {
      const options = configuration(root),
        store = new Store(options.stateDir),
        logger = new Logger(options.logging.directory, "ERROR");
      try {
        const bytes = await imageBytes();
        options.files.maxAggregateBytes = bytes.length;
        const generation = new Generation(options, store, logger),
          initial = receipt();
        store.prepare(initial, "hash", "identity");
        const partial = await generation.finish(
          initial,
          {
            images: [{ bytes }, { base64: bytes.toString("base64") }],
            upstreamModel: null,
            warnings: [],
          },
          requestSchema.parse({ prompt: "draw", count: 2 }),
          signal,
        );
        expect(partial.outputs).toHaveLength(1);
        expect(partial.status).toBe("partial");
        expect(partial.errors).toEqual([
          expect.objectContaining({
            code: "invalid_response",
            message: "Aggregate output bytes exceeded.",
          }),
        ]);
        const cancelled = receipt("cancelled");
        store.prepare(cancelled, "cancelled-hash", "identity");
        const result = await generation.finish(
          cancelled,
          { images: [{ bytes }], upstreamModel: null, warnings: [] },
          requestSchema.parse({ prompt: "draw" }),
          AbortSignal.abort(),
        );
        expect(result).toMatchObject({
          status: "failed",
          generation_outcome: "completed",
          storage_outcome: "partial_or_failed",
          outputs: [],
        });
        expect(result.errors).toHaveLength(1);
        expect(store.pendingResults(cancelled.request_id)).toHaveLength(1);
        const recovered = await getJob(
          generation,
          cancelled.request_id,
          true,
          signal,
        );
        expect(recovered.status).toBe("completed");
        expect(await readFile(recovered.outputs[0]!.path)).toEqual(bytes);
        expect(store.listOutputs()).toHaveLength(2);
      } finally {
        logger.close();
        store.close();
      }
    }));

  it("completes data URL output through actual image storage and does not refresh a terminal job", async ({
    signal,
  }) =>
    workspace(async (root) => {
      const options = configuration(root),
        store = new Store(options.stateDir),
        logger = new Logger(options.logging.directory, "ERROR");
      try {
        const generation = new Generation(options, store, logger),
          initial = receipt();
        store.prepare(initial, "hash", "identity");
        const bytes = await imageBytes();
        const result = await generation.finish(
          initial,
          {
            images: [
              { url: `data:image/png;base64,${bytes.toString("base64")}` },
            ],
            upstreamModel: null,
            warnings: [],
          },
          requestSchema.parse({ prompt: "draw" }),
          signal,
        );
        expect(result).toMatchObject({
          status: "completed",
          storage_outcome: "completed",
          errors: [],
          deviations: [],
        });
        expect(
          await getJob(generation, initial.request_id, true, signal),
        ).toEqual(result);
        expect(
          await cancelJob(options, generation, initial.request_id, signal),
        ).toEqual({
          request_id: initial.request_id,
          local_wait_cancelled: true,
          upstream_requested: false,
          upstream_cancelled: null,
          refund_verified: false,
          another_submission_may_charge: true,
        });
        expect(store.cancelled(initial.request_id)).toBe(true);
      } finally {
        logger.close();
        store.close();
      }
    }));

  it("recovers only an owned Responses job and aborts an active local wait independently of refund claims", async ({
    signal,
  }) =>
    workspace(async (root) => {
      const paths: string[] = [],
        bytes = await imageBytes();
      await server(
        (request, response) => {
          paths.push(request.url ?? "");
          json(response, {
            status: "completed",
            output: [
              {
                type: "image_generation_call",
                status: "completed",
                result: bytes.toString("base64"),
              },
            ],
          });
        },
        async (origin) => {
          const options = configuration(root, {
            local: connection("openai-responses", {
              baseUrl: `${origin}/v1`,
              orchestrationModel: "text",
              discoveryTimeoutMs: 2000,
            }),
          });
          const store = new Store(options.stateDir),
            logger = new Logger(options.logging.directory, "ERROR");
          try {
            const initial = {
              ...receipt(),
              status: "running",
              upstream_job: { id: "owned job", kind: "responses" },
            };
            store.prepare(
              initial,
              "hash",
              await connectionIdentity("local", options.connections.local!),
            );
            const generation = new Generation(options, store, logger);
            expect(
              (await getJob(generation, initial.request_id, false, signal))
                .status,
            ).toBe("running");
            expect(paths).toEqual([]);
            expect(
              (await getJob(generation, initial.request_id, true, signal))
                .status,
            ).toBe("completed");
            expect(paths).toEqual(["/v1/responses/owned%20job"]);
            const controller = new AbortController();
            generation.active.set(initial.request_id, controller);
            expect(
              (await cancelJob(options, generation, initial.request_id, signal))
                .refund_verified,
            ).toBe(false);
            expect(controller.signal.aborted).toBe(true);
            generation.active.delete(initial.request_id);
          } finally {
            logger.close();
            store.close();
          }
        },
      );
    }));
});

describe("transactional owned receipts", () => {
  it("reuses the same receipt across independent connections and after restart without resubmission", async () =>
    workspace(async (root) => {
      const directory = join(root, "state"),
        first = new Store(directory),
        second = new Store(directory);
      try {
        const initial = receipt(),
          hash = fingerprint({ prompt: "draw", model: "image-model" });
        expect(first.prepare(initial, hash, "connection-identity")).toEqual({
          fresh: true,
          receipt: initial,
        });
        const duplicate = second.prepare(
          receipt(),
          hash,
          "connection-identity",
        );
        expect(duplicate.fresh).toBe(false);
        expect(duplicate.receipt).toEqual(initial);
        const finished = {
          ...first.get(initial.request_id),
          status: "completed",
          generation_outcome: "completed",
          storage_outcome: "completed",
        };
        first.update(finished);
        expect(second.get(initial.request_id)).toMatchObject({
          status: "completed",
          generation_outcome: "completed",
        });
        const detached = second.get(initial.request_id);
        detached.warnings.push("not persisted");
        expect(first.get(initial.request_id).warnings).toEqual([]);
      } finally {
        first.close();
        second.close();
      }
      const restarted = new Store(directory);
      try {
        expect(
          restarted.prepare(
            receipt(),
            fingerprint({ prompt: "draw", model: "image-model" }),
            "connection-identity",
          ),
        ).toMatchObject({ fresh: false, receipt: { status: "completed" } });
      } finally {
        restarted.close();
      }
    }));

  it("rejects input or identity conflicts and rolls back without corrupting the original receipt", async () =>
    workspace(async (root) => {
      const store = new Store(join(root, "state"));
      try {
        store.prepare(receipt(), "hash-1", "identity-1");
        expect(() => store.prepare(receipt(), "hash-2", "identity-1")).toThrow(
          expect.objectContaining({ code: "request_id_conflict" }),
        );
        expect(() => store.prepare(receipt(), "hash-1", "identity-2")).toThrow(
          expect.objectContaining({ code: "request_id_conflict" }),
        );
        expect(store.prepare(receipt(), "hash-1", "identity-1").fresh).toBe(
          false,
        );
        expect(store.get("request-1").status).toBe("prepared");
        expect(() => store.get("unknown")).toThrow(
          expect.objectContaining({ code: "invalid_input" }),
        );
        expect(() => store.cancel("unknown")).toThrow(
          expect.objectContaining({ code: "invalid_input" }),
        );
      } finally {
        store.close();
      }
    }));

  it("bounds global and per-profile admission across instances and releases terminal work", async () =>
    workspace(async (root) => {
      const directory = join(root, "state"),
        first = new Store(directory),
        second = new Store(directory);
      try {
        for (const [id, profile] of [
          ["a1", "a"],
          ["a2", "a"],
          ["b1", "b"],
          ["b2", "b"],
        ])
          (id === "a2" || id === "b1" ? second : first).prepare(
            receipt(id!, profile!),
            id!,
            profile!,
          );
        expect(second.queued()).toBe(4);
        expect(first.admit("a1", "a", 2, 1)).toBe(true);
        expect(second.admit("a2", "a", 2, 1)).toBe(false);
        expect(second.admit("b1", "b", 2, 1)).toBe(true);
        expect(first.admit("b2", "b", 2, 2)).toBe(false);
        expect(first.get("a2").status).toBe("prepared");
        expect(first.queued()).toBe(2);
        first.update({ ...first.get("a1"), status: "completed" });
        expect(second.admit("a2", "a", 2, 1)).toBe(true);
        expect(first.get("a2").status).toBe("submitting");
      } finally {
        first.close();
        second.close();
      }
    }));

  it.each([false, true])(
    "recovers expired intent with recoverable upstream job=%s without issuing fresh work",
    async (hasJob) =>
      workspace(async (root) => {
        const store = new Store(join(root, "state"));
        const clock = vi.spyOn(Date, "now");
        try {
          clock.mockReturnValue(1000);
          const initial = receipt();
          initial.status = "running";
          initial.generation_outcome = "running";
          if (hasJob)
            initial.upstream_job = { id: "upstream-owned", kind: "responses" };
          store.prepare(initial, "hash", "identity");
          clock.mockReturnValue(3601001);
          const recovered = store.prepare(receipt(), "hash", "identity");
          expect(recovered).toMatchObject({
            fresh: false,
            receipt: {
              status: hasJob ? "running" : "outcome_unknown",
              generation_outcome: hasJob ? "running" : "unknown",
            },
          });
          expect(store.get(initial.request_id).status).toBe(
            recovered.receipt.status,
          );
          if (hasJob)
            expect(recovered.receipt.upstream_job).toEqual({
              id: "upstream-owned",
              kind: "responses",
            });
        } finally {
          clock.mockRestore();
          store.close();
        }
      }),
  );

  it("does not classify an expired completed receipt as uncertain", async () =>
    workspace(async (root) => {
      const store = new Store(join(root, "state")),
        clock = vi.spyOn(Date, "now");
      try {
        clock.mockReturnValue(0);
        store.prepare(
          {
            ...receipt(),
            status: "completed",
            generation_outcome: "completed",
          },
          "hash",
          "identity",
        );
        clock.mockReturnValue(3600001);
        expect(
          store.prepare(receipt(), "hash", "identity").receipt.status,
        ).toBe("completed");
      } finally {
        clock.mockRestore();
        store.close();
      }
    }));

  it("persists cancellation independently from billing and receipt outcome", async () =>
    workspace(async (root) => {
      const directory = join(root, "state"),
        first = new Store(directory),
        second = new Store(directory);
      try {
        first.prepare(receipt(), "hash", "identity");
        expect(second.cancelled("request-1")).toBe(false);
        first.cancel("request-1");
        first.cancel("request-1");
        expect(second.cancelled("request-1")).toBe(true);
        expect(second.get("request-1")).toMatchObject({
          status: "prepared",
          another_submission_may_charge: false,
        });
        expect(second.cancelled("absent")).toBe(false);
      } finally {
        first.close();
        second.close();
      }
      const restarted = new Store(directory);
      try {
        expect(restarted.cancelled("request-1")).toBe(true);
      } finally {
        restarted.close();
      }
    }));

  it("persists output metadata with newest-first pagination and one lookahead row", async () =>
    workspace(async (root) => {
      const directory = join(root, "state"),
        store = new Store(directory);
      const ids = [randomUUID(), randomUUID(), randomUUID(), randomUUID()];
      try {
        expect(store.listOutputs()).toEqual([]);
        for (const id of ids) store.addOutput(output(id));
        expect(store.output(ids[0]!)).toEqual(output(ids[0]!));
        expect(store.listOutputs(0, 2).map((row) => row.output_id)).toEqual([
          ids[3],
          ids[2],
          ids[1],
        ]);
        expect(store.listOutputs(2, 2).map((row) => row.output_id)).toEqual([
          ids[1],
          ids[0],
        ]);
        expect(store.listOutputs(4, 2)).toEqual([]);
        expect(() => store.output("absent")).toThrow(
          expect.objectContaining({ code: "invalid_input" }),
        );
        expect(() =>
          store.addOutput(output(ids[0]!, "different-request")),
        ).toThrow();
        expect(store.output(ids[0]!).request_id).toBe("request-1");
      } finally {
        store.close();
      }
      const restarted = new Store(directory);
      try {
        expect(restarted.output(ids[3]!)).toEqual(output(ids[3]!));
      } finally {
        restarted.close();
      }
    }));
});
