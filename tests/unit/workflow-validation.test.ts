import { it, expect } from "vitest";
import { validateGraph } from "../../src/adapters/workflow-validation.js";
import { connection } from "../fixtures/runtime.js";
it("validates installed enum/scalar/link contracts before submission", () => {
  const c = connection("comfyui", {
    workflow: {
      graph: {
        a: { class_type: "Source", inputs: { count: 1, mode: "fast" } },
        b: { class_type: "Save", inputs: { image: ["a", 0] } },
      },
      bindings: { prompt: { node: "a", input: "mode" } },
      outputNodes: ["b"],
    },
  });
  const info = {
    Source: {
      input: {
        required: {
          count: ["INT", { min: 1, max: 2 }],
          mode: [["fast", "slow"]],
        },
      },
      output: ["IMAGE"],
    },
    Save: { input: { required: { image: ["IMAGE"] } }, output_node: true },
  };
  expect(() =>
    validateGraph(c.workflow!.graph, c.workflow!, info),
  ).not.toThrow();
  const bad = structuredClone(c.workflow!.graph);
  (bad.a as { inputs: Record<string, unknown> }).inputs.mode = "unsupported";
  expect(() => validateGraph(bad, c.workflow!, info)).toThrow("enum");
  const cycle = structuredClone(c.workflow!.graph);
  (cycle.a as { inputs: Record<string, unknown> }).inputs.count = ["b", 0];
  expect(() => validateGraph(cycle, c.workflow!, info)).toThrow("cycle");
});
