import { open } from "node:fs/promises";
import { parseEnv } from "node:util";
import { fail } from "../errors.js";

export const maxConfigBytes = 1024 * 1024;
export async function readConfigText(file: string): Promise<string> {
  try {
    const handle = await open(file, "r");
    try {
      if (!(await handle.stat()).isFile()) throw new Error("Not a file.");
      const buffer = Buffer.alloc(maxConfigBytes + 1);
      let length = 0;
      while (length < buffer.length) {
        const { bytesRead } = await handle.read(
          buffer,
          length,
          buffer.length - length,
          null,
        );
        if (!bytesRead) break;
        length += bytesRead;
      }
      if (length > maxConfigBytes) throw new Error("Too large.");
      return new TextDecoder("utf-8", { fatal: true }).decode(
        buffer.subarray(0, length),
      );
    } finally {
      await handle.close();
    }
  } catch {
    return fail(
      "invalid_configuration",
      "Cannot read configuration as a UTF-8 file up to 1 MiB.",
    );
  }
}

export function parseEnvConfig(text: string): Record<string, unknown> {
  const invalid = () =>
    fail(
      "invalid_configuration",
      "Invalid env configuration. Check keys, values and unique assignments.",
    );
  if (Buffer.byteLength(text, "utf8") > maxConfigBytes || text.includes("\0"))
    invalid();
  // Native parseEnv accepts malformed lines and overwrites duplicates. Validate assignments first.
  const assignment = /(?:export[ \t]+)?([A-Za-z_][A-Za-z0-9_]*)[ \t]*=[ \t]*/y;
  const keys = new Set<string>();
  let offset = 0;
  while (offset < text.length) {
    const trivia = /^[\s﻿]*(?:#[^\r\n]*(?:\r?\n|$))?/.exec(
      text.slice(offset),
    )![0];
    if (trivia) {
      offset += trivia.length;
      continue;
    }
    assignment.lastIndex = offset;
    const match = assignment.exec(text);
    if (!match || keys.has(match[1]!)) invalid();
    keys.add(match![1]!);
    offset = assignment.lastIndex;
    const quote = text[offset];
    if (quote === '"' || quote === "'" || quote === "`") {
      const end = text.indexOf(quote, offset + 1);
      if (end < 0) invalid();
      offset = end + 1;
      const tail = /^[ \t]*(?:#[^\r\n]*)?(?:\r?\n|$)/.exec(text.slice(offset));
      if (!tail) invalid();
      offset += tail![0].length;
    } else {
      const end = text.indexOf("\n", offset);
      offset = end < 0 ? text.length : end + 1;
    }
  }
  const values = parseEnv(text);
  if (Object.keys(values).length !== keys.size) invalid();
  const top = new Set([
    "API_ENDPOINT",
    "API_KEY",
    "PROFILE",
    "ORCHESTRATION_MODEL",
    "EDIT_MODE",
    "EDIT_ENCODING",
    "EDIT_MASK_POLARITY",
    "EDIT_MAX_REFERENCES",
    "EDIT_MASKS",
  ]);
  const slots = new Map<number, Record<string, unknown>>();
  const boolean = (value: string) => {
    if (value !== "true" && value !== "false") invalid();
    return value === "true";
  };
  for (const [key, value] of Object.entries(values)) {
    if (typeof value !== "string") invalid();
    if (top.has(key)) continue;
    const match =
      /^MODEL_([1-9][0-9]{0,2})_(ID|NAME|ENABLED|OUTPUT_FORMAT|SIZE|QUALITY)$/.exec(
        key,
      );
    if (!match || Number(match[1]) > 100) invalid();
    const slot = Number(match![1]);
    const model = slots.get(slot) ?? {};
    slots.set(slot, model);
    if (value !== "")
      model[match![2]!.toLowerCase()] =
        match![2] === "ENABLED" ? boolean(value!) : value;
  }
  const raw: Record<string, unknown> = {
    api_endpoint: values.API_ENDPOINT,
    api_key: values.API_KEY,
    models: [...slots.entries()]
      .sort(([a], [b]) => a - b)
      .map(([, model]) => model)
      .filter((model) => Object.keys(model).length > 0),
  };
  for (const [key, field] of [
    ["PROFILE", "profile"],
    ["ORCHESTRATION_MODEL", "orchestration_model"],
  ] as const)
    if (values[key]) raw[field] = values[key];
  const edit: Record<string, unknown> = {};
  for (const [key, field] of [
    ["EDIT_MODE", "mode"],
    ["EDIT_ENCODING", "encoding"],
    ["EDIT_MASK_POLARITY", "maskPolarity"],
  ] as const)
    if (values[key]) edit[field] = values[key];
  if (values.EDIT_MASKS) edit.masks = boolean(values.EDIT_MASKS);
  if (values.EDIT_MAX_REFERENCES) {
    if (!/^(?:[1-9]|[12][0-9]|3[0-2])$/.test(values.EDIT_MAX_REFERENCES))
      invalid();
    edit.maxReferences = Number(values.EDIT_MAX_REFERENCES);
  }
  if (values.EDIT_MODE) raw.edit = edit;
  else if (Object.keys(edit).length) invalid();
  return raw;
}
