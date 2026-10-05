import { it, expect } from "vitest";
import {
  connection,
  configuration,
  imageBytes,
  receipt,
} from "../fixtures/runtime.js";
import { requestSchema, connectionSchema } from "../../src/config/schema.js";
import { effectiveRequest } from "../../src/capabilities.js";
import { imagesRequest } from "../../src/adapters/images.js";
import { inspectImage } from "../../src/files/images.js";
import {
  snapshotRequirements,
  outputDeviations,
  outputRequirementWarnings,
} from "../../src/services/output-requirements.js";
import { Discovery } from "../../src/services/discovery.js";

it.each(["json", "generation"] as const)(
  "rejects scalar reference loss for %s at configuration and request boundaries",
  async (mode) => {
    expect(() =>
      connectionSchema.parse({
        adapter: "openai-images",
        baseUrl: "https://api.example/v1",
        auth: { type: "none" },
        edit: { mode, encoding: "image", maxReferences: 2 },
      }),
    ).toThrow("incompatible");
    const c = connection("openai-images", {
      edit: { mode, encoding: "image", maxReferences: 1 },
      modelOverrides: { model: { maxReferences: 2 } },
    });
    const r = requestSchema.parse({
      prompt: "draw",
      reference_images: [
        { type: "data_url", data_url: "data:image/png;base64,AQID" },
        { type: "data_url", data_url: "data:image/png;base64,BAUG" },
      ],
    });
    expect(() => effectiveRequest(c, r, "model")).toThrow("Scalar image");
    const config = configuration(".ci-work/requirements");
    const one = await inspectImage(await imageBytes("png", true, 8, 6), config);
    const two = await inspectImage(await imageBytes("png", true, 6, 8), config);
    expect(() =>
      imagesRequest({
        connection: c,
        model: "model",
        request: r,
        operation: "edit",
        references: [one, two],
      }),
    ).toThrow("Scalar image");
    const encoded = [one, two].map(
      (image) => `data:${image.mime};base64,${image.bytes.toString("base64")}`,
    );
    for (const encoding of [
      "images",
      "image_urls",
      "input_references",
    ] as const) {
      const arrayConnection = connection("openai-images", {
        edit: { mode, encoding, maxReferences: 2 },
      });
      const built = imagesRequest({
        connection: arrayConnection,
        model: "model",
        request: r,
        operation: "edit",
        references: [one, two],
      });
      const body = JSON.parse(String(built.body));
      expect(body[encoding]).toEqual(
        encoding === "input_references"
          ? encoded.map((url) => ({ type: "image_url", image_url: { url } }))
          : encoded,
      );
    }
    const single = imagesRequest({
      connection: c,
      model: "model",
      request: r,
      operation: "edit",
      references: [one],
    });
    expect(JSON.parse(String(single.body)).image).toBe(encoded[0]);
    for (const encoding of ["image", "image[]"] as const) {
      const multipart = imagesRequest({
        connection: connection("openai-images", {
          edit: { mode: "multipart", encoding, maxReferences: 2 },
        }),
        model: "model",
        request: r,
        operation: "edit",
        references: [one, two],
      });
      expect(typeof multipart.body).not.toBe("string");
      const files = (multipart.body as import("undici").FormData).getAll(
        encoding,
      ) as File[];
      expect(files).toHaveLength(2);
      expect(Buffer.from(await files[0]!.arrayBuffer())).toEqual(one.bytes);
      expect(Buffer.from(await files[1]!.arrayBuffer())).toEqual(two.bytes);
    }
  },
);

it("exposes sourced GPT Image limits separately from the gateway safety budget", async () => {
  const config = configuration(".ci-work/requirements");
  const result = await new Discovery(config).model(
    undefined,
    "cx/gpt-image-2.5-sunburst",
  );
  expect(result.dimensions.documented_limits.max_edge).toBe(3840);
  expect(result.dimensions.documented_limits.max_pixels).toBe(8294400);
  expect(result.dimensions.documented_limits.forwarding_guaranteed).toBe(false);
  expect(result.dimensions.route_support.max_pixels).toBe(64000000);
  const c = config.connections.local!;
  expect(() =>
    effectiveRequest(
      c,
      requestSchema.parse({ prompt: "draw", size: "4096x4096" }),
      "cx/gpt-image-2.5-sunburst",
    ),
  ).toThrow("documented");
  expect(() =>
    effectiveRequest(
      c,
      requestSchema.parse({ prompt: "draw", size: "3840x2160" }),
      "cx/gpt-image-2.5-sunburst",
    ),
  ).not.toThrow();
  const unknown = await new Discovery(config).model(
    undefined,
    "cx/gpt-6.1-sol",
  );
  expect(unknown.dimensions.documented_limits.max_edge).toBeNull();
  const pro = await new Discovery(config).model(
    undefined,
    "antigravity/gemini-3-pro-image-preview",
  );
  expect(pro.dimensions.model_support.aspect_ratios).toHaveLength(10);
  expect(pro.dimensions.model_support.resolution_tiers).toEqual([
    "1K",
    "2K",
    "4K",
  ]);
  expect(pro.dimensions.documented_limits.max_edge).toBe(6336);
  for (const preference of [{ aspect_ratio: "1:8" }, { image_size: "8K" }])
    expect(() =>
      effectiveRequest(
        c,
        requestSchema.parse({ prompt: "draw", ...preference }),
        "gemini-3-pro-image",
      ),
    ).toThrow("documented model presets");
  expect(pro.dimensions.documented_limits.pixel_sizes).toContainEqual({
    aspect_ratio: "1:1",
    resolution: "4K",
    width: 4096,
    height: 4096,
  });
  for (const size of [
    "3841x2160",
    "3840x2304",
    "1008x512",
    "1025x1024",
    "3840x1024",
  ]) {
    expect(() =>
      effectiveRequest(
        c,
        requestSchema.parse({ prompt: "draw", size }),
        "gpt-image-2.5-flare",
      ),
    ).toThrow("documented");
  }
});

it("persists aspect requirements and detects mismatch without modifying original bytes", async () => {
  const config = configuration(".ci-work/requirements"),
    c = config.connections.local!;
  const image = await inspectImage(await imageBytes(), config);
  const requested = requestSchema.parse({
    prompt: "draw",
    aspect_ratio: "16:9",
    image_size: "4K",
  });
  const r = {
    ...receipt(),
    output_requirements: {
      count: 1,
      ...snapshotRequirements(c, "opaque-model", requested),
    },
  };
  const restored = JSON.parse(JSON.stringify(r));
  expect(outputDeviations(r, image, 0)).toEqual(
    outputDeviations(restored, image, 0),
  );
  expect(outputDeviations(r, image, 0)).toContain(
    "Output 0 aspect ratio differs from requested aspect_ratio.",
  );
  expect(outputRequirementWarnings(r)).toHaveLength(1);
  const native = {
    ...receipt(),
    output_requirements: {
      count: 1,
      ...snapshotRequirements(
        c,
        "gemini-3.1-flash-image",
        requestSchema.parse({
          prompt: "draw",
          aspect_ratio: "4:3",
          image_size: "4K",
        }),
      ),
    },
  };
  expect(outputDeviations(native, image, 0)).toContain(
    "Output 0 dimensions differ from the documented requested resolution tier.",
  );
  for (const model of ["gemini-3.1-flash-image", "gemini-3-pro-image"]) {
    const evidence = snapshotRequirements(
      c,
      model,
      requested,
    ).dimension_evidence;
    for (const size of evidence.pixel_sizes!) {
      const requirements = {
        ...receipt(),
        output_requirements: {
          count: 1,
          aspect_ratio: size.aspect_ratio,
          image_size: size.resolution,
          dimension_evidence: evidence,
        },
      };
      // Pure comparison uses sourced metadata; actual decoded-byte persistence is tested at the storage seam.
      expect(
        outputDeviations(
          requirements,
          { ...image, width: size.width, height: size.height },
          0,
        ),
      ).toEqual([]);
      expect(outputRequirementWarnings(requirements)).toEqual([]);
    }
  }
});
