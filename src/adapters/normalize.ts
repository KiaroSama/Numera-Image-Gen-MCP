import type { Connection } from "../config/schema.js";
import { array, record, fail } from "../errors.js";
import { sseEvents } from "../http/stream.js";
import type { Normalized, ResultItem } from "./types.js";
export function normalize(c: Connection, raw: unknown): Normalized {
  const value = record(raw);
  const images: ResultItem[] = [];
  let terminal: Normalized["terminal"];
  const warnings: string[] = [];
  const signatures: unknown[] = [];
  const imageRecord = (v: unknown): Record<string, unknown> | undefined => {
    if (!v || typeof v !== "object" || Array.isArray(v)) {
      images.push({ error: "Malformed image result item." });
      return;
    }
    return v as Record<string, unknown>;
  };
  const push = (v: unknown) => {
    const item = imageRecord(v);
    if (!item) return;
    if (typeof item.b64_json === "string" && item.b64_json)
      images.push({ base64: item.b64_json });
    else if (typeof item.url === "string" && item.url)
      images.push({ url: item.url });
    else images.push({ error: "Image item contains no final bytes or URL." });
  };
  if (c.adapter === "gemini") {
    if (value.promptFeedback && record(value.promptFeedback).blockReason)
      fail(
        "provider_rejection",
        "Provider rejected image generation.",
        "response",
      );
    for (const candidate of array(value.candidates)) {
      const item = imageRecord(candidate);
      if (!item) continue;
      if (item.finishReason && item.finishReason !== "STOP")
        warnings.push(`Candidate ended with ${String(item.finishReason)}.`);
      const content = item.content ? imageRecord(item.content) : undefined;
      const parts = array(content?.parts);
      for (const [partIndex, p] of parts.entries()) {
        const part = imageRecord(p);
        if (!part) continue;
        if (typeof part.thoughtSignature === "string")
          signatures.push({
            part_index: partIndex,
            signature: part.thoughtSignature,
          });
        if (part.thought === true) continue;
        if (part.inlineData) {
          const inline = imageRecord(part.inlineData);
          if (inline) push({ b64_json: inline.data });
        }
      }
    }
  } else if (c.adapter === "gemini-interactions") {
    if (["in_progress", "queued"].includes(String(value.status))) {
      if (typeof value.id !== "string" || !value.id)
        fail(
          "outcome_unknown",
          "Async response has no recoverable job ID.",
          "response",
        );
    }
    if (["in_progress", "queued"].includes(String(value.status)))
      return {
        images: [],
        upstreamModel: typeof value.model === "string" ? value.model : null,
        job: { id: String(value.id), kind: "interactions" },
        warnings: [],
      };
    if (value.status === "budget_exceeded") terminal = "failed";
    else if (
      ["failed", "cancelled", "incomplete"].includes(String(value.status))
    )
      terminal = value.status as Normalized["terminal"];
    else if (value.status && value.status !== "completed")
      fail(
        "provider_rejection",
        `Interaction ended with ${String(value.status)}.`,
        "response",
      );
    for (const step of array(value.steps)) {
      const s = imageRecord(step);
      if (!s) continue;
      if (s.type === "thought" && typeof s.signature === "string")
        signatures.push({ step_type: "thought", signature: s.signature });
      if (s.type !== "model_output") continue;
      for (const content of array(s.content)) {
        const part = imageRecord(content);
        if (part?.type === "image")
          push({ b64_json: part.data, url: part.uri });
      }
    }
    if (!images.some((image) => image.base64 || image.url) && value.output_image) {
      const image = imageRecord(value.output_image);
      if (image) push({ b64_json: image.data, url: image.uri });
    }
  } else if (c.adapter === "openai-responses") {
    if (["in_progress", "queued"].includes(String(value.status))) {
      if (typeof value.id !== "string" || !value.id)
        fail(
          "outcome_unknown",
          "Async response has no recoverable job ID.",
          "response",
        );
    }
    if (["in_progress", "queued"].includes(String(value.status)))
      return {
        images: [],
        upstreamModel: null,
        job: { id: String(value.id), kind: "responses" },
        warnings: [],
      };
    if (["failed", "cancelled", "incomplete"].includes(String(value.status)))
      terminal = value.status as Normalized["terminal"];
    else if (value.status && value.status !== "completed")
      fail(
        "provider_rejection",
        `Response ended with ${String(value.status)}.`,
        "response",
      );
    for (const item of array(value.output)) {
      const i = imageRecord(item);
      if (i?.type === "image_generation_call" && i.status === "completed")
        push({ b64_json: i.result });
    }
  } else if (c.adapter === "chat-images") {
    for (const choice of array(value.choices)) {
      const item = imageRecord(choice);
      if (!item) continue;
      const message = imageRecord(item.message);
      if (!message) continue;
      if (message.refusal)
        fail(
          "provider_rejection",
          "Provider refused image output.",
          "response",
        );
      for (const image of array(message.images)) {
        const i = imageRecord(image);
        if (!i) continue;
        const ref = imageRecord(i.image_url);
        if (ref) push({ url: ref.url });
      }
    }
  } else for (const item of array(value.data)) push(item);
  if (!terminal && !images.some((i) => i.base64 || i.url || i.bytes))
    fail(
      "no_image_returned",
      "No final image was returned; text, empty bodies and partial previews are not final images.",
      "response",
    );
  return {
    images,
    terminal,
    upstreamModel: typeof value.model === "string" ? value.model : null,
    upstreamId: typeof value.id === "string" ? value.id : undefined,
    usage: value.usage,
    continuation: signatures.length ? { signatures } : undefined,
    warnings,
  };
}
export function normalizeResponse(
  c: Connection,
  bytes: Buffer,
  mime: string,
): Normalized {
  if (/^image\/(png|jpeg|webp)(?:;|$)/.test(mime))
    return { images: [{ bytes }], upstreamModel: null, warnings: [] };
  if (mime.includes("text/event-stream")) {
    const events = sseEvents(bytes);
    if (c.adapter === "openai-responses") {
      const terminal = events.find((e) =>
        ["response.failed", "response.incomplete"].includes(e.type),
      );
      const response = terminal?.data.response;
      if (
        response &&
        typeof record(response).id === "string" &&
        ["failed", "incomplete"].includes(String(record(response).status))
      )
        return normalize(c, response);
    }
    if (
      events.some(
        (e) =>
          e.type === "error" ||
          e.type === "response.failed" ||
          e.type === "response.incomplete",
      )
    )
      fail(
        "outcome_unknown",
        "Image stream ended with an error; no resubmission made.",
        "response",
      );
    if (c.gateway === "9router") {
      const done = events.find((e) => e.type === "done" && e.data.data);
      if (!done)
        fail(
          "outcome_unknown",
          "9router stream has no terminal done result.",
          "response",
        );
      return normalize(c, done.data);
    }
    if (c.adapter === "openrouter-images") {
      const finals = events.filter(
        (e) => e.type === "image_generation.completed",
      );
      if (!events.some((e) => e.type === "done") || !finals.length)
        fail(
          "outcome_unknown",
          "Image stream has no final completion.",
          "response",
        );
      return normalize(c, {
        data: finals.map((e) => e.data),
        usage: finals.at(-1)?.data.usage,
      });
    }
    if (c.adapter === "openai-responses") {
      const done = events.find((e) => e.type === "response.completed");
      if (!done?.data.response)
        fail(
          "outcome_unknown",
          "Responses stream has no completed response.",
          "response",
        );
      return normalize(c, done.data.response);
    }
    if (c.adapter === "gemini") {
      const results = events.filter((e) => e.data.candidates);
      return normalize(c, {
        candidates: results.flatMap((e) => array(e.data.candidates)),
      });
    }
    fail(
      "unsupported_operation",
      "Streaming contract is not implemented for this adapter.",
      "response",
    );
  }
  let raw: unknown;
  try {
    raw = JSON.parse(bytes.toString("utf8"));
  } catch {
    return fail(
      "invalid_response",
      "Image response is not valid JSON.",
      "response",
    );
  }
  return normalize(c, raw);
}
