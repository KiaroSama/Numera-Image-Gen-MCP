import { record, fail } from "../errors.js";
import type { Connection } from "../config/schema.js";

export function validateGraph(
  graph: Record<string, unknown>,
  workflow: NonNullable<Connection["workflow"]>,
  info: Record<string, unknown>,
): void {
  const visiting = new Set<string>(),
    visited = new Set<string>();
  function visit(id: string) {
    if (visiting.has(id))
      fail("invalid_configuration", "ComfyUI workflow contains a cycle.");
    if (visited.has(id)) return;
    const node = record(graph[id]);
    visiting.add(id);
    const descriptor = info[String(node.class_type)];
    if (!descriptor)
      fail(
        "unsupported_operation",
        "Workflow requires a node not installed on this backend.",
      );
    const inputInfo = record(record(descriptor).input ?? {}),
      required = record(inputInfo.required ?? {}),
      optional = record(inputInfo.optional ?? {}),
      inputs = record(node.inputs);
    for (const key of Object.keys(required))
      if (!(key in inputs))
        fail(
          "invalid_configuration",
          `Workflow node ${id} is missing required input ${key}.`,
        );
    for (const [key, value] of Object.entries(inputs)) {
      const raw = required[key] ?? optional[key];
      if (raw === undefined) continue;
      if (Array.isArray(value)) {
        if (
          value.length !== 2 ||
          typeof value[0] !== "string" ||
          !Number.isInteger(value[1]) ||
          Number(value[1]) < 0 ||
          !graph[value[0]]
        )
          fail(
            "invalid_configuration",
            "Workflow link does not reference a valid node output.",
          );
        visit(value[0]);
        const upstream = record(graph[value[0]]),
          outputs = record(info[String(upstream.class_type)]).output;
        if (Array.isArray(outputs) && Number(value[1]) >= outputs.length)
          fail(
            "invalid_configuration",
            "Workflow link output index is out of range.",
          );
        continue;
      }
      if (!Array.isArray(raw) || raw.length === 0) continue;
      const type = raw[0];
      if (Array.isArray(type)) {
        if (!type.includes(value))
          fail(
            "invalid_configuration",
            `Workflow input ${key} is not an installed enum value.`,
          );
      } else if (type === "INT" || type === "FLOAT") {
        if (
          typeof value !== "number" ||
          !Number.isFinite(value) ||
          (type === "INT" && !Number.isInteger(value))
        )
          fail(
            "invalid_configuration",
            `Workflow input ${key} must be ${type}.`,
          );
        const bounds =
          raw[1] && typeof raw[1] === "object" ? record(raw[1]) : {};
        if (
          (typeof bounds.min === "number" && value < bounds.min) ||
          (typeof bounds.max === "number" && value > bounds.max)
        )
          fail(
            "invalid_configuration",
            `Workflow input ${key} exceeds node bounds.`,
          );
      } else if (
        (type === "STRING" && typeof value !== "string") ||
        (type === "BOOLEAN" && typeof value !== "boolean")
      )
        fail(
          "invalid_configuration",
          `Workflow input ${key} has the wrong scalar type.`,
        );
    }
    visiting.delete(id);
    visited.add(id);
  }
  for (const id of Object.keys(graph)) visit(id);
  for (const id of workflow.outputNodes) {
    const node = record(graph[id]),
      descriptor = record(info[String(node.class_type)]);
    if (descriptor.output_node === false)
      fail(
        "invalid_configuration",
        "Configured output node is not an output node.",
      );
  }
}
