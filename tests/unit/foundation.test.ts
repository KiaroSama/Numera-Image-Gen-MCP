import { describe, it, expect } from "vitest";
import { resolveOperationUrl } from "../../src/http/url.js";
import { redact } from "../../src/logging.js";

describe("trusted destination resolution", () => {
  it.each([
    [
      "http://127.0.0.1:20100/v1",
      "api-prefix",
      "http://127.0.0.1:20100/v1/images/generations",
    ],
    [
      "http://127.0.0.1:20100",
      "origin",
      "http://127.0.0.1:20100/v1/images/generations",
    ],
    [
      "https://gateway.example/api/v1/",
      "api-prefix",
      "https://gateway.example/api/v1/images/generations",
    ],
    [
      "https://gateway.example/company/v1",
      "api-prefix",
      "https://gateway.example/company/v1/images/generations",
    ],
  ] as const)("preserves %s prefix in %s mode", (base, mode, expected) => {
    expect(
      resolveOperationUrl(base, mode, "images/generations", "/v1").href,
    ).toBe(expected);
  });
  it.each([
    "https://key:secret@example.com/v1",
    "https://example.com/v1#x",
    "https://example.com/v1/images/generations",
    "https://example.com/v1?api_key=x",
  ])("rejects ambiguous or credentialed base %s", (base) => {
    expect(() =>
      resolveOperationUrl(base, "api-prefix", "images/generations", "/v1"),
    ).toThrow();
  });
  it.each(["../secret", "%2e%2e/secret", "//other.example/path", "a\\b"])(
    "rejects path %s",
    (path) => {
      expect(() =>
        resolveOperationUrl(
          "https://example.com/v1",
          "api-prefix",
          path,
          "/v1",
        ),
      ).toThrow();
    },
  );
  it("redacts nested sensitive values and signed queries without removing safe status", () => {
    const result = JSON.stringify(
      redact({
        status: 401,
        nested: {
          authorization: "Bearer private",
          prompt: "private prompt",
          url: "https://a.example/img?signature=private",
        },
      }),
    );
    expect(result).not.toContain("private");
    expect(result).toContain("401");
  });
});
