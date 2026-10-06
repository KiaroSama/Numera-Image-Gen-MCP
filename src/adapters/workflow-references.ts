import { fail, record } from "../errors.js";
import type { AdapterInput } from "./types.js";

export function validateReferenceBindings(input: AdapterInput): void {
  const workflow = input.connection.workflow;
  if (!workflow)
    fail("invalid_configuration", "Configure an explicit ComfyUI workflow.");
  const required = [
    ...input.references.map((_, index) =>
      index === 0 ? "image" : `image_${index + 1}`,
    ),
    ...(input.mask ? ["mask"] : []),
  ];
  for (const key of required) {
    const target = workflow.bindings[key];
    if (!target)
      fail("unsupported_operation", `Workflow lacks ${key} reference binding.`);
    const node = workflow.graph[target.node];
    if (!node || !record(node).inputs || !target.input)
      fail(
        "invalid_configuration",
        `Workflow binding ${key} has no valid input target.`,
      );
    record(record(node).inputs);
    // A reference must not overwrite another reference, mask or prompt binding.
    if (
      Object.entries(workflow.bindings).some(
        ([other, value]) =>
          other !== key &&
          value.node === target.node &&
          value.input === target.input,
      )
    )
      fail(
        "invalid_configuration",
        "Reference bindings must have distinct input targets.",
      );
  }
}
