import { it, expect } from "vitest";
import { Store } from "../../src/jobs/store.js";
import { normalize } from "../../src/adapters/normalize.js";
import { connection, workspace, receipt } from "../fixtures/runtime.js";

it("preserves signatures privately without exposing thoughts as image outputs", async () =>
  workspace(async (root) => {
    const result = normalize(connection("gemini"), {
      candidates: [
        {
          content: {
            parts: [
              {
                thought: true,
                thoughtSignature: "opaque-private",
                text: "hidden",
              },
              {
                inlineData: { mimeType: "image/png", data: "aW1hZ2U=" },
                thoughtSignature: "final-private",
              },
            ],
          },
        },
      ],
    });
    expect(result.images).toEqual([{ base64: "aW1hZ2U=" }]);
    expect(result.continuation).toBeDefined();
    const store = new Store(root);
    try {
      store.prepare(receipt(), "hash", "scope");
      store.saveContinuation("request-1", "scope", result.continuation);
      expect(store.get("request-1")).not.toHaveProperty("continuation");
      expect(store.continuation("request-1", "scope")).toEqual(
        result.continuation,
      );
      expect(() => store.continuation("request-1", "different")).toThrow(
        "scope",
      );
    } finally {
      store.close();
    }
  }));
it("retains Interactions thought signatures separately from final images", () => {
  const result = normalize(connection("gemini-interactions"), {
    id: "interaction",
    status: "completed",
    steps: [
      {
        type: "thought",
        signature: "encrypted",
        summary: [{ type: "image", data: "not-final" }],
      },
      { type: "model_output", content: [{ type: "image", data: "ZmluYWw=" }] },
    ],
  });
  expect(result.images).toEqual([{ base64: "ZmluYWw=" }]);
  expect(result.continuation).toBeDefined();
});
