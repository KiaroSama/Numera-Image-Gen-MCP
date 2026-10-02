import { fail } from "../errors.js";
export function resolveOperationUrl(
  base: string,
  mode: "api-prefix" | "origin",
  operation: string,
  prefix: string,
  query: Record<string, string> = {},
): URL {
  let url: URL;
  try {
    url = new URL(base);
  } catch {
    return fail("invalid_configuration", "Invalid API base URL.");
  }
  if (
    !["http:", "https:"].includes(url.protocol) ||
    url.username ||
    url.password ||
    url.hash ||
    url.search ||
    /%2e|%2f|%5c/i.test(base) ||
    /(?:^|\/)\.\.(?:\/|$)/.test(base)
  )
    fail(
      "invalid_configuration",
      "API base must not contain credentials, query, fragments or traversal.",
    );
  if (
    /\/(?:generations|edits|generateContent|responses|interactions|images|chat\/completions)\/?$/.test(
      url.pathname,
    )
  )
    fail(
      "invalid_configuration",
      "Supply an API prefix, not a full endpoint URL.",
    );
  if (mode === "origin" && url.pathname !== "/")
    fail(
      "invalid_configuration",
      "Origin mode requires an origin without a path.",
    );
  if (
    !operation ||
    operation.startsWith("/") ||
    /[\\?#\x00]/.test(operation) ||
    operation
      .split("/")
      .some(
        (p) => decodeURIComponent(p) === ".." || decodeURIComponent(p) === ".",
      )
  )
    fail("invalid_configuration", "Invalid relative operation path.");
  url.pathname = `${mode === "origin" ? prefix.replace(/\/$/, "") : url.pathname.replace(/\/$/, "")}/${operation}`;
  for (const [k, v] of Object.entries(query)) {
    if (k !== "pageToken" && /key|token|secret|auth|signature/i.test(k))
      fail(
        "invalid_configuration",
        "Credentials must not be query parameters.",
      );
    url.searchParams.set(k, v);
  }
  return url;
}
