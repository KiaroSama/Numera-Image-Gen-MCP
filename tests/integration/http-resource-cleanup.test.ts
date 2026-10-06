import { it, expect, vi } from "vitest";
import { getDefaultAutoSelectFamily, setDefaultAutoSelectFamily } from "node:net";
import { apiRequest } from "../../src/http/client.js";
import { fetchAsset } from "../../src/http/assets.js";
import {
  configuration, connection, workspace, server, imageBytes,
} from "../fixtures/runtime.js";

// Pin DNS deterministically while exercising the real Undici socket and HTTP response.
vi.mock("node:dns/promises", () => ({
  lookup: vi.fn(async () => [{ address: "127.0.0.1", family: 4 }]),
}));

it.each([true, false])("downloads a DNS-pinned hostname with autoSelectFamily=%s", async (enabled) =>
  workspace(async (root) => {
    const previous = getDefaultAutoSelectFamily();
    setDefaultAutoSelectFamily(enabled);
    try {
      const bytes = await imageBytes();
      await server((req, res) => {
        expect(req.headers.authorization).toBeUndefined();
        res.end(bytes);
      }, async (origin) => {
        const hostname = origin.replace("127.0.0.1", "fixture.invalid");
        const config = configuration(root);
        config.files.assetAllowances = [{ origin: hostname, pathPrefix: "/image" }];
        expect(await fetchAsset(`${hostname}/image`, config, 4096)).toEqual(bytes);
        await expect(fetchAsset(`${hostname}/not-allowed`, config, 4096)).rejects.toMatchObject({ code: "unsafe_destination" });
      });
    } finally {
      setDefaultAutoSelectFamily(previous);
    }
  }),
);

for (const mode of ["redirect", "oversize", "chunk-limit", "sse-error"] as const)
  it(`releases ${mode} response sockets without waiting for the provider deadline`, async () => {
    let calls = 0;
    await server((_req, res) => {
      calls++;
      res.writeHead(mode === "redirect" ? 302 : 200, {
        "Content-Type": mode === "sse-error" ? "text/event-stream" : "application/json",
        ...(mode === "redirect" ? { Location: "/elsewhere" } : {}),
        ...(mode === "oversize" ? { "Content-Length": "9999999" } : {}),
      });
      res.flushHeaders();
      res.write(mode === "sse-error" ? 'event: error\ndata: {"error":"fixture"}\n\n' : mode === "chunk-limit" ? "x".repeat(200) : "x");
      // Deliberately leave the body open: only the client can release this request promptly.
    }, async (origin) => {
      const controller = new AbortController();
      let rescued = false;
      const rescue = setTimeout(() => { rescued = true; controller.abort(); }, 2000);
      try {
        await expect(apiRequest(connection("openai-images", {
          baseUrl: `${origin}/v1`, discoveryTimeoutMs: 15000,
        }), "probe", "/v1", {}, controller.signal, 100)).rejects.toMatchObject({
          code: mode === "redirect" ? "unsafe_destination" : mode === "sse-error" ? "outcome_unknown" : "invalid_response",
        });
        expect(rescued).toBe(false);
        expect(calls).toBe(1);
      } finally {
        clearTimeout(rescue);
        controller.abort();
      }
    });
  });

it("releases an oversized asset response and does not follow an unapproved redirect", async () =>
  workspace(async (root) => {
    let downloads = 0;
    await server((req, res) => {
      downloads++;
      if (req.url === "/redirect") {
        res.writeHead(302, { Location: "/private" });
        res.end();
      } else {
        res.writeHead(200, { "Content-Length": "9999999" });
        res.flushHeaders();
        res.write("x");
      }
    }, async (origin) => {
      const config = configuration(root);
      config.files.assetAllowances = [{ origin, pathPrefix: "/image" }, { origin, pathPrefix: "/redirect" }];
      const controller = new AbortController();
      let rescued = false;
      const rescue = setTimeout(() => { rescued = true; controller.abort(); }, 2000);
      try {
        await expect(fetchAsset(`${origin}/image`, config, 100, controller.signal)).rejects.toMatchObject({ code: "invalid_response" });
        expect(rescued).toBe(false);
        await expect(fetchAsset(`${origin}/redirect`, config, 100)).rejects.toMatchObject({ code: "unsafe_destination" });
        expect(downloads).toBe(2);
      } finally {
        clearTimeout(rescue);
        controller.abort();
      }
    });
  }),
);
