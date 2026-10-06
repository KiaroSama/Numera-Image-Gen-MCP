import { fetch, ProxyAgent, Agent, type FormData } from "undici";
import type { Connection } from "../config/schema.js";
import { credentials } from "../config/credentials.js";
import { NumeraError, fail } from "../errors.js";
import { resolveOperationUrl } from "./url.js";
import { readSse } from "./sse-reader.js";
export async function boundedBytes(
  response: {
    body: AsyncIterable<Uint8Array> | null;
    headers: { get: (k: string) => string | null };
  },
  limit: number,
  signal: AbortSignal,
): Promise<Buffer> {
  const length = Number(response.headers.get("content-length"));
  if (length > limit)
    fail("invalid_response", "Response exceeds byte limit.", "response");
  const chunks: Buffer[] = [];
  let total = 0;
  if (!response.body)
    fail("invalid_response", "Response body is empty.", "response");
  for await (const chunk of response.body) {
    signal.throwIfAborted();
    total += chunk.byteLength;
    if (total > limit)
      fail("invalid_response", "Response exceeds byte limit.", "response");
    chunks.push(Buffer.from(chunk));
  }
  return Buffer.concat(chunks, total);
}
export async function apiRequest(
  connection: Connection,
  operation: string,
  prefix: string,
  init: {
    method?: string;
    body?: string | FormData;
    headers?: Record<string, string>;
    query?: Record<string, string>;
  } = {},
  signal?: AbortSignal,
  limit = 100 * 1024 * 1024,
) {
  const deadline = AbortSignal.timeout(
    init.method === "POST"
      ? connection.requestTimeoutMs
      : connection.discoveryTimeoutMs,
  );
  const combined = signal ? AbortSignal.any([signal, deadline]) : deadline;
  const url = resolveOperationUrl(
    connection.baseUrl,
    connection.baseUrlMode,
    connection.paths[operation] ?? operation,
    prefix,
    { ...connection.query, ...init.query },
  );
  const headers = { ...(await credentials(connection)), ...init.headers };
  const agent = connection.proxy
    ? new ProxyAgent({
        uri: connection.proxy.url,
        token: connection.proxy.secretEnv
          ? process.env[connection.proxy.secretEnv]
          : undefined,
      })
    : new Agent();
  let consumed = false;
  try {
    const response = await fetch(url, {
      method: init.method ?? "GET",
      body: init.body,
      headers,
      signal: combined,
      redirect: "manual",
      dispatcher: agent,
    });
    if (response.status >= 300 && response.status < 400)
      fail(
        "unsafe_destination",
        "API redirects are not followed.",
        "submission",
      );
    if (!response.ok) {
      await response.body?.cancel();
      const code =
        response.status === 401
          ? "authentication_error"
          : response.status === 403
            ? "permission_denied"
            : response.status === 429
              ? "rate_limited"
              : response.status === 404
                ? "model_unavailable"
                : "upstream_error";
      throw new NumeraError(
        code,
        `Upstream returned HTTP ${response.status}. No automatic resubmission was made.`,
        "upstream",
        response.status,
        "Check connection/account/model and inspect its existing job before authorizing a new request.",
      );
    }
    const bytes =
      response.headers.get("content-type")?.includes("text/event-stream") &&
      response.body
        ? await readSse(response.body, limit, combined)
        : await boundedBytes(response, limit, combined);
    consumed = true;
    return {
      bytes,
      mime: response.headers.get("content-type") ?? "",
      requestId: response.headers.get("x-request-id"),
    };
  } catch (error) {
    if (error instanceof NumeraError) throw error;
    if (combined.aborted)
      throw new NumeraError(
        signal?.aborted ? "cancelled" : "request_timeout",
        "Waiting stopped; upstream completion and billing may be unknown.",
        "network",
      );
    throw new NumeraError(
      "outcome_unknown",
      "Connection interrupted; do not automatically resubmit.",
      "network",
    );
  } finally {
    // Graceful close waits for unread bodies; rejected responses must release their sockets now.
    if (consumed) await agent.close();
    else await agent.destroy();
  }
}
export async function apiJson(
  connection: Connection,
  operation: string,
  prefix: string,
  init: Parameters<typeof apiRequest>[3] = {},
  signal?: AbortSignal,
): Promise<unknown> {
  const response = await apiRequest(
    connection,
    operation,
    prefix,
    init,
    signal,
    16 * 1024 * 1024,
  );
  try {
    return JSON.parse(response.bytes.toString("utf8"));
  } catch {
    return fail(
      "invalid_response",
      "Upstream returned invalid JSON.",
      "response",
    );
  }
}
