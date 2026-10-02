import { beforeAll, describe, expect, it } from "vitest";
import { join } from "node:path";
import {
  prepareComfy,
  submitComfy,
  comfyStatus,
  validateWorkflow,
  waitComfy,
} from "../../src/adapters/comfyui.js";
import type { AdapterInput } from "../../src/adapters/types.js";
import { requestSchema, type Connection } from "../../src/config/schema.js";
import { record } from "../../src/errors.js";
import { inspectImage, type Image } from "../../src/files/images.js";
import {
  configuration,
  connection,
  imageBytes,
  json,
  requestBody,
  server,
} from "../fixtures/runtime.js";

const workflow = (): NonNullable<Connection["workflow"]> => ({
  graph: {
    text: { class_type: "CLIPTextEncode", inputs: { text: "original" } },
    sampler: { class_type: "KSampler", inputs: { seed: 0, batch_size: 1 } },
    load: {
      class_type: "LoadImage",
      inputs: { image: "original.png", mask: "mask.png" },
    },
    second: { class_type: "LoadImage", inputs: { image: "second.png" } },
    save: { class_type: "SaveImage", inputs: { images: ["sampler", 0] } },
  },
  bindings: {
    prompt: { node: "text", input: "text" },
    seed: { node: "sampler", input: "seed" },
    count: { node: "sampler", input: "batch_size" },
    image: { node: "load", input: "image" },
    image_2: { node: "second", input: "image" },
    mask: { node: "load", input: "mask" },
  },
  outputNodes: ["save"],
  requireGpu: true,
});
const input = (
  origin = "https://fixture.example",
  changes: Record<string, unknown> = {},
  profile: Record<string, unknown> = {},
): AdapterInput => ({
  connection: connection("comfyui", {
    baseUrl: origin,
    workflow: workflow(),
    discoveryTimeoutMs: 2000,
    requestTimeoutMs: 2000,
    ...profile,
  }),
  request: requestSchema.parse({ prompt: "Draw a café", ...changes }),
  model: "workflow",
  operation: "generate",
  references: [],
});
let image: Image;
beforeAll(async () => {
  image = await inspectImage(
    await imageBytes(),
    configuration(join(process.cwd(), ".ci-work", "comfy")),
  );
});

const installed = {
  CLIPTextEncode: {},
  KSampler: {},
  LoadImage: {},
  SaveImage: {},
};

describe("explicit ComfyUI workflow bindings", () => {
  it("binds prompt, seed, count and provider options on a clone rather than shared workflow state", () => {
    const configured = workflow();
    configured.bindings.strength = { node: "sampler", input: "denoise" };
    const adapter = input(
      undefined,
      { seed: 42, count: 2, provider_options: { strength: 0.7 } },
      { workflow: configured },
    );
    const result = validateWorkflow(adapter);
    expect(result.graph).toMatchObject({
      text: { inputs: { text: "Draw a café" } },
      sampler: { inputs: { seed: 42, batch_size: 2, denoise: 0.7 } },
    });
    expect(adapter.connection.workflow?.graph).toEqual(configured.graph);
    result.bind("image", "uploaded.png", true);
    expect(result.graph).toMatchObject({
      load: { inputs: { image: "uploaded.png" } },
    });
    expect(configured.graph).toMatchObject({
      load: { inputs: { image: "original.png" } },
    });
    result.bind("optional-unbound", 1);
  });

  it.each([
    "size",
    "aspect_ratio",
    "image_size",
    "quality",
    "output_format",
    "background",
    "negative_prompt",
  ] as const)(
    "requires an explicit %s binding rather than dropping common inputs",
    (field) => {
      const value = field === "output_format" ? "png" : "fixture-value";
      expect(() =>
        validateWorkflow(input(undefined, { [field]: value })),
      ).toThrow(expect.objectContaining({ code: "unsupported_parameter" }));
      const configured = workflow();
      configured.bindings[field] = { node: "sampler", input: field };
      expect(
        validateWorkflow(
          input(undefined, { [field]: value }, { workflow: configured }),
        ).graph,
      ).toMatchObject({ sampler: { inputs: { [field]: value } } });
    },
  );

  it("rejects absent/empty/oversized or non-API workflows and invalid output/binding nodes", () => {
    const absent = input();
    delete absent.connection.workflow;
    expect(() => validateWorkflow(absent)).toThrow(
      expect.objectContaining({ code: "invalid_configuration" }),
    );
    const empty = workflow();
    empty.graph = {};
    const oversized = workflow();
    oversized.graph = Object.fromEntries(
      Array.from({ length: 301 }, (_, i) => [
        `node-${i}`,
        { class_type: "LoadImage", inputs: {} },
      ]),
    );
    const uiGraph = workflow();
    uiGraph.graph.text = { widgets_values: ["prompt"] };
    const missingOutput = workflow();
    missingOutput.outputNodes = ["absent"];
    const badBinding = workflow();
    badBinding.bindings.prompt = { node: "absent", input: "text" };
    for (const configured of [
      empty,
      oversized,
      uiGraph,
      missingOutput,
      badBinding,
    ])
      expect(() =>
        validateWorkflow(input(undefined, {}, { workflow: configured })),
      ).toThrow(expect.objectContaining({ code: "invalid_configuration" }));
    const noPrompt = workflow();
    delete noPrompt.bindings.prompt;
    expect(() =>
      validateWorkflow(input(undefined, {}, { workflow: noPrompt })),
    ).toThrow(expect.objectContaining({ code: "unsupported_parameter" }));
    const noCount = workflow();
    delete noCount.bindings.count;
    expect(() =>
      validateWorkflow(input(undefined, { count: 2 }, { workflow: noCount })),
    ).toThrow(expect.objectContaining({ code: "unsupported_parameter" }));
    expect(() =>
      validateWorkflow(input(undefined, { provider_options: { unbound: 1 } })),
    ).toThrow(expect.objectContaining({ code: "unsupported_parameter" }));
  });
});

describe("ComfyUI loopback wire and job lifecycle", () => {
  it("checks installed nodes/GPU, uploads all refs and mask, submits bound graph, and fetches only explicit final output nodes", async ({
    signal,
  }) => {
    const paths: string[] = [],
      uploads: Buffer[] = [];
    let submitted: Record<string, unknown> | undefined;
    const imageQueries: string[] = [];
    await server(
      async (request, response) => {
        const url = new URL(request.url ?? "/", "http://fixture.local");
        paths.push(`${request.method} ${url.pathname}`);
        if (url.pathname === "/object_info") json(response, installed);
        else if (url.pathname === "/system_stats")
          json(response, { devices: [{ type: "cuda" }] });
        else if (url.pathname === "/upload/image") {
          uploads.push(await requestBody(request));
          json(response, {
            name: `upload-${uploads.length}.png`,
            subfolder: "fixtures",
          });
        } else if (url.pathname === "/prompt") {
          submitted = record(
            JSON.parse((await requestBody(request)).toString("utf8")),
          );
          json(response, { prompt_id: "owned-job" });
        } else if (url.pathname === "/history/owned-job")
          json(response, {
            "owned-job": {
              status: { completed: true, status_str: "success" },
              outputs: {
                save: {
                  images: [
                    {
                      filename: "final.png",
                      subfolder: "batch",
                      type: "output",
                    },
                    { filename: "ignored-input.png", type: "input" },
                    { filename: "temp.png", type: "temp" },
                  ],
                },
                unbound: {
                  images: [{ filename: "unowned.png", type: "output" }],
                },
              },
            },
          });
        else if (url.pathname === "/view") {
          imageQueries.push(url.search);
          response.writeHead(200, { "Content-Type": "image/png" });
          response.end(image.bytes);
        } else json(response, {}, 404);
      },
      async (origin) => {
        const adapter = input(origin, { seed: 123 });
        adapter.references = [image, image];
        adapter.mask = image;
        adapter.operation = "edit";
        const prepared = await prepareComfy(adapter, signal);
        const job = await submitComfy(adapter, prepared, signal);
        expect(job).toEqual({ id: "owned-job", kind: "comfyui", warnings: [] });
        expect(uploads).toHaveLength(3);
        for (const upload of uploads) {
          expect(upload.includes(image.bytes)).toBe(true);
          expect(upload.toString("latin1")).toContain(
            "Content-Type: image/png",
          );
        }
        expect(submitted).toMatchObject({
          client_id: expect.any(String),
          prompt: {
            text: { inputs: { text: "Draw a café" } },
            sampler: { inputs: { seed: 123 } },
            load: {
              inputs: {
                image: "fixtures/upload-1.png",
                mask: "fixtures/upload-3.png",
              },
            },
            second: { inputs: { image: "fixtures/upload-2.png" } },
          },
        });
        expect(adapter.connection.workflow?.graph).toMatchObject({
          load: { inputs: { image: "original.png" } },
        });
        const result = await comfyStatus(adapter.connection, job.id, signal);
        expect(result).toEqual({
          images: [{ bytes: image.bytes }, { bytes: image.bytes }],
          upstreamModel: null,
          upstreamId: "owned-job",
          warnings: [],
        });
        expect(imageQueries).toEqual([
          "?filename=final.png&subfolder=batch&type=output",
          "?filename=temp.png&subfolder=&type=temp",
        ]);
        expect(paths).toEqual([
          "GET /object_info",
          "GET /system_stats",
          "POST /upload/image",
          "POST /upload/image",
          "POST /upload/image",
          "POST /prompt",
          "GET /history/owned-job",
          "GET /view",
          "GET /view",
        ]);
      },
    );
  });

  it.each(["missing-node", "cpu", "no-devices"])(
    "rejects unavailable %s before upload or prompt submission",
    async (scenario) => {
      const signal = AbortSignal.timeout(5000);
      const paths: string[] = [];
      await server(
        (request, response) => {
          paths.push(request.url ?? "");
          if (request.url === "/object_info")
            json(response, scenario === "missing-node" ? {} : installed);
          else
            json(response, {
              devices: scenario === "no-devices" ? [] : [{ type: "cpu" }],
            });
        },
        async (origin) => {
          await expect(
            prepareComfy(input(origin), signal),
          ).rejects.toMatchObject({ code: "unsupported_operation" });
          expect(
            paths.some(
              (path) => path.includes("upload") || path.includes("prompt"),
            ),
          ).toBe(false);
        },
      );
    },
  );

  it("accepts explicit CPU opt-out without probing system_stats", async ({
    signal,
  }) => {
    const paths: string[] = [];
    await server(
      (request, response) => {
        paths.push(request.url ?? "");
        json(response, installed);
      },
      async (origin) => {
        const configured = workflow();
        configured.requireGpu = false;
        await prepareComfy(input(origin, {}, { workflow: configured }), signal);
        expect(paths).toEqual(["/object_info"]);
      },
    );
  });

  it("fails missing reference/mask bindings before submitting a prompt", async ({
    signal,
  }) => {
    const configured = workflow();
    delete configured.bindings.image;
    delete configured.bindings.mask;
    const adapter = input(undefined, {}, { workflow: configured });
    adapter.references = [image];
    await expect(
      submitComfy(adapter, validateWorkflow(adapter), signal),
    ).rejects.toMatchObject({ code: "unsupported_operation" });
    adapter.references = [];
    adapter.mask = image;
    await expect(
      submitComfy(adapter, validateWorkflow(adapter), signal),
    ).rejects.toMatchObject({ code: "unsupported_operation" });
  });

  it("rejects malformed upload identity and unknown submission outcome without retry", async ({
    signal,
  }) => {
    let uploads = 0,
      prompts = 0;
    await server(
      (request, response) => {
        if (request.url === "/upload/image") {
          uploads++;
          json(response, { name: 42, subfolder: "" });
        } else {
          prompts++;
          json(response, {});
        }
      },
      async (origin) => {
        const adapter = input(origin);
        adapter.references = [image];
        await expect(
          submitComfy(adapter, validateWorkflow(adapter), signal),
        ).rejects.toMatchObject({ code: "invalid_response", stage: "upload" });
        expect(uploads).toBe(1);
        expect(prompts).toBe(0);
        adapter.references = [];
        await expect(
          submitComfy(adapter, validateWorkflow(adapter), signal),
        ).rejects.toMatchObject({
          code: "outcome_unknown",
          stage: "submission",
        });
        expect(prompts).toBe(1);
      },
    );
  });

  it("rejects malformed mask upload identity before paid prompt submission", async ({
    signal,
  }) => {
    let prompts = 0;
    await server(
      (request, response) => {
        if (request.url === "/upload/image") json(response, {});
        else {
          prompts++;
          json(response, { prompt_id: "must-not-submit" });
        }
      },
      async (origin) => {
        const adapter = input(origin);
        adapter.mask = image;
        await expect(
          submitComfy(adapter, validateWorkflow(adapter), signal),
        ).rejects.toMatchObject({ code: "invalid_response", stage: "upload" });
        expect(prompts).toBe(0);
      },
    );
  });

  it.each(["absent", "running", "error", "empty"])(
    "distinguishes history state %s without generating or fetching unrelated images",
    async (state) => {
      const signal = AbortSignal.timeout(5000);
      let views = 0;
      await server(
        (request, response) => {
          if (request.url?.startsWith("/view")) views++;
          const status =
            state === "running"
              ? { completed: false }
              : state === "error"
                ? { status_str: "error" }
                : { completed: true };
          json(
            response,
            state === "absent"
              ? {}
              : {
                  job: {
                    status,
                    outputs: {
                      save: {
                        images: [{ filename: "not-output.png", type: "input" }],
                      },
                    },
                  },
                },
          );
        },
        async (origin) => {
          const profile = input(origin).connection;
          if (state === "error")
            await expect(
              comfyStatus(profile, "job", signal),
            ).rejects.toMatchObject({ code: "provider_rejection" });
          else if (state === "empty")
            await expect(
              comfyStatus(profile, "job", signal),
            ).rejects.toMatchObject({ code: "no_image_returned" });
          else
            await expect(
              comfyStatus(profile, "job", signal),
            ).resolves.toBeUndefined();
          expect(views).toBe(0);
        },
      );
    },
  );

  it("returns completed history on the first bounded wait iteration and aborts without polling after cancellation", async ({
    signal,
  }) => {
    let calls = 0;
    await server(
      (request, response) => {
        calls++;
        if (request.url?.startsWith("/history"))
          json(response, {
            job: {
              status: { completed: true },
              outputs: {
                save: { images: [{ filename: "final.png", type: "output" }] },
              },
            },
          });
        else {
          response.writeHead(200, { "Content-Type": "image/png" });
          response.end(image.bytes);
        }
      },
      async (origin) => {
        expect(
          (await waitComfy(input(origin).connection, "job", signal)).images,
        ).toEqual([{ bytes: image.bytes }]);
        expect(calls).toBe(2);
        await expect(
          waitComfy(
            input(origin).connection,
            "job",
            AbortSignal.abort(new Error("fixture cancellation")),
          ),
        ).rejects.toThrow("fixture cancellation");
        expect(calls).toBe(2);
      },
    );
  });
});
