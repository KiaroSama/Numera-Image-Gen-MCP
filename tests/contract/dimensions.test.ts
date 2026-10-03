import { it, expect } from "vitest";
import { Discovery } from "../../src/services/discovery.js";
import { configuration, connection } from "../fixtures/runtime.js";

it("lists native ratios and dimensions separately from generic route acceptance", async () => {
  const discovery = new Discovery(
    configuration(".ci-work/dimensions", { local: connection() }),
  );
  const result = await discovery.model(
    undefined,
    "antigravity/gemini-3.1-flash-image",
  );
  expect(result.dimensions.preset_aspect_ratios).toContain("16:9");
  expect(result.dimensions.model_support.aspect_ratios).toHaveLength(14);
  expect(result.dimensions.model_support.resolution_tiers).toEqual([
    "0.5K",
    "1K",
    "2K",
    "4K",
  ]);
  expect(result.dimensions.model_support.pixel_sizes).toContainEqual({
    aspect_ratio: "16:9",
    resolution: "4K",
    width: 5504,
    height: 3072,
  });
  expect(result.dimensions.route_support.accepted_parameters).toEqual(["size"]);
  expect(result.dimensions.model_support.custom_dimensions.status).toBe(
    "unsupported",
  );
});

it("does not invent native support for opaque model IDs", async () => {
  const result = await new Discovery(
    configuration(".ci-work/dimensions", { local: connection() }),
  ).model(undefined, "cx/gpt-6.1-sol");
  expect(result.dimensions.model_support.aspect_ratios).toBeNull();
  expect(result.dimensions.model_support.resolution_tiers).toBeNull();
  expect(result.dimensions.model_support.custom_dimensions.status).toBe(
    "unknown",
  );
  expect(result.dimensions.preset_aspect_ratios).toContain("1:1");
});

it("reports gateway tier restrictions rather than equating them with native support", async () => {
  const result = await new Discovery(
    configuration(".ci-work/dimensions", {
      local: connection("openai-images", { gateway: "omniroute" }),
    }),
  ).model(undefined, "antigravity/gemini-3.1-flash-image");
  expect(result.dimensions.route_support.accepted_parameters).toEqual([
    "size",
    "aspect_ratio",
    "image_size",
  ]);
  expect(result.dimensions.route_support.resolution_tiers).toEqual([
    "1K",
    "2K",
    "4K",
  ]);
  expect(result.account_availability).toBe("unknown");
});

it.each([
  ["gemini", {}, ["aspect_ratio", "image_size"]],
  ["gemini-interactions", {}, ["aspect_ratio", "image_size"]],
  ["comfyui", {}, []],
  [
    "openai-images",
    {
      modelOverrides: {
        "gemini-3.1-flash-image": { supportedParameters: ["size", "quality"] },
      },
    },
    ["size"],
  ],
] as const)(
  "reports %s adapter dimension inputs without generation",
  async (adapter, overrides, accepted) => {
    const result = await new Discovery(
      configuration(".ci-work/dimensions", {
        local: connection(adapter, overrides),
      }),
    ).model(undefined, "gemini-3.1-flash-image");
    expect(result.dimensions.route_support.accepted_parameters).toEqual(
      accepted,
    );
    expect(result.dimensions.route_support.max_pixels).toBe(64000000);
    expect(result.dimensions.account_verified).toBe(false);
  },
);
