import { describe, expect, it } from "vitest";
import { join } from "node:path";
import { Discovery, connectionPrefix } from "../../src/services/discovery.js";
import { health, listConnections } from "../../src/services/health.js";
import { apiJson, apiRequest } from "../../src/http/client.js";
import { fetchAsset } from "../../src/http/assets.js";
import {
  configuration,
  connection,
  imageBytes,
  json,
  server,
  workspace,
} from "../fixtures/runtime.js";

const config = (
  origin: string,
  overrides: Parameters<typeof connection>[1] = {},
) =>
  configuration(join(process.cwd(), ".ci-work", "discovery"), {
    local: connection("openai-images", {
      baseUrl: `${origin}/v1`,
      discoveryTimeoutMs: 2000,
      ...overrides,
    }),
  });

describe("read-only provider discovery", () => {
  it.each([
    ["openai-images", undefined, "/v1/models", { data: [{ id: "image-a" }] }],
    [
      "openai-images",
      "9router",
      "/v1/models/image",
      { data: [{ id: "image-a" }] },
    ],
    ["gemini", undefined, "/v1beta/models", { models: [{ name: "image-a" }] }],
    [
      "gemini-interactions",
      undefined,
      "/v1beta/models",
      { models: [{ name: "image-a" }] },
    ],
    [
      "openrouter-images",
      undefined,
      "/api/v1/images/models",
      { data: [{ id: "image-a" }] },
    ],
    ["comfyui", undefined, "/object_info", { "image-a": { input: {} } }],
  ] as const)(
    "%s gateway=%s uses its exact specialty discovery route",
    async (adapter, gateway, path, payload) => {
      const paths: string[] = [];
      await server(
        (request, response) => {
          paths.push(request.url ?? "");
          json(response, payload);
        },
        async (origin) => {
          const profile = connection(adapter, {
            baseUrl: origin,
            baseUrlMode: "origin",
            gateway,
            discoveryTimeoutMs: 2000,
          });
          const discovery = new Discovery(
            configuration(join(process.cwd(), ".ci-work", "routes"), {
              local: profile,
            }),
          );
          const result = await discovery.list();
          expect(result).toMatchObject({
            complete: true,
            cached: false,
            warnings: [],
          });
          expect(result.models.map((model) => model.id)).toEqual(["image-a"]);
          expect(paths).toEqual([path]);
          expect(connectionPrefix(profile)).toBe(
            adapter === "comfyui"
              ? ""
              : adapter.startsWith("gemini")
                ? "/v1beta"
                : adapter === "openrouter-images"
                  ? "/api/v1"
                  : "/v1",
          );
        },
      );
    },
  );

  it("supplements OmniRoute specialty results with paginated unified image entries only", async () => {
    const paths: string[] = [];
    await server(
      (request, response) => {
        const url = new URL(request.url ?? "/", "http://fixture.local");
        paths.push(url.pathname + url.search);
        if (url.pathname === "/v1/images/generations")
          json(response, { data: [{ id: "specialty" }] });
        else if (!url.searchParams.has("after"))
          json(response, {
            data: [
              { id: "specialty", type: "image" },
              { id: "text-model", type: "text" },
              { id: "unified-1", type: "image" },
            ],
            has_more: true,
            last_id: "cursor-1",
          });
        else json(response, { data: [{ id: "unified-2", type: "image" }] });
      },
      async (origin) => {
        const result = await new Discovery(
          config(origin, { gateway: "omniroute" }),
        ).list();
        expect(result.models.map((model) => model.id)).toEqual([
          "specialty",
          "unified-1",
          "unified-2",
        ]);
        expect(result.complete).toBe(true);
        expect(result.warnings).toHaveLength(1);
        expect(paths).toEqual([
          "/v1/images/generations",
          "/v1/models?limit=100",
          "/v1/models?after=cursor-1&limit=100",
        ]);
      },
    );
  });

  it("follows Gemini pageToken without mutating configuration or carrying cursors into refresh", async () => {
    const queries: string[] = [];
    await server(
      (request, response) => {
        const url = new URL(request.url ?? "/", "http://fixture.local");
        queries.push(url.search);
        if (url.searchParams.has("pageToken"))
          json(response, { models: [{ name: "second" }] });
        else
          json(response, {
            models: [{ name: "first" }],
            nextPageToken: "token-2",
          });
      },
      async (origin) => {
        const options = configuration(
          join(process.cwd(), ".ci-work", "gemini"),
          {
            local: connection("gemini", {
              baseUrl: `${origin}/v1beta`,
              query: { scope: "images" },
              discoveryTimeoutMs: 2000,
            }),
          },
        );
        const discovery = new Discovery(options);
        expect(
          (await discovery.list()).models.map((model) => model.id),
        ).toEqual(["first", "second"]);
        expect(
          (await discovery.list(undefined, true)).models.map(
            (model) => model.id,
          ),
        ).toEqual(["first", "second"]);
        expect(options.connections.local?.query).toEqual({ scope: "images" });
        expect(queries).toEqual([
          "?scope=images",
          "?scope=images&pageToken=token-2",
          "?scope=images",
          "?scope=images&pageToken=token-2",
        ]);
      },
    );
  });

  it("isolates catalog caches by profile and retains stale evidence after refresh failure", async () => {
    let failing = false;
    const paths: string[] = [];
    await server(
      (request, response) => {
        const path = request.url ?? "";
        paths.push(path);
        if (failing) json(response, {}, 503);
        else
          json(response, {
            data: [{ id: path.startsWith("/a/") ? "a-model" : "b-model" }],
          });
      },
      async (origin) => {
        const options = configuration(
          join(process.cwd(), ".ci-work", "profiles"),
          {
            a: connection("openai-images", {
              baseUrl: `${origin}/a/v1`,
              discoveryTimeoutMs: 2000,
            }),
            b: connection("openai-images", {
              baseUrl: `${origin}/b/v1`,
              discoveryTimeoutMs: 2000,
            }),
          },
        );
        const discovery = new Discovery(options);
        expect((await discovery.list("a")).models[0]?.id).toBe("a-model");
        expect((await discovery.list("b")).models[0]?.id).toBe("b-model");
        expect((await discovery.list("a")).cached).toBe(true);
        expect(paths).toHaveLength(2);
        failing = true;
        const stale = await discovery.list("a", true);
        expect(stale.models.map((model) => model.id)).toEqual(["a-model"]);
        expect(stale).toMatchObject({ cached: false, complete: false });
        expect(stale.warnings).toEqual([
          expect.objectContaining({ code: "upstream_error", http_status: 503 }),
          "Using stale catalog; availability is unknown.",
        ]);
      },
    );
  });

  it("bounds repeated cursors and page counts while marking partial availability", async () => {
    let calls = 0,
      repeat = true;
    await server(
      (_request, response) => {
        calls++;
        json(response, {
          data: [{ id: `model-${calls}` }],
          has_more: true,
          last_id: repeat ? "same" : `cursor-${calls}`,
        });
      },
      async (origin) => {
        const discovery = new Discovery(config(origin));
        const repeated = await discovery.list();
        expect(calls).toBe(2);
        expect(repeated.complete).toBe(false);
        expect(repeated.warnings).toContain("Repeated catalog cursor.");
        repeat = false;
        calls = 0;
        const bounded = await discovery.list(undefined, true);
        expect(calls).toBe(20);
        expect(bounded.complete).toBe(false);
        expect(bounded.models).toHaveLength(20);
        expect(bounded.warnings).toContain("Catalog page limit reached.");
      },
    );
  });

  it("reports malformed first-page discovery as incomplete rather than silently complete", async () => {
    await server(
      (_request, response) => json(response, { data: [null] }),
      async (origin) => {
        const result = await new Discovery(config(origin)).list();
        expect(result.models).toEqual([]);
        expect(result.complete).toBe(false);
        expect(result.warnings).toEqual([
          expect.objectContaining({ code: "invalid_response" }),
        ]);
      },
    );
  });

  it("uses gateway-specific model metadata endpoints with encoded IDs and keeps unknown availability", async () => {
    const paths: string[] = [];
    await server(
      (request, response) => {
        paths.push(request.url ?? "");
        json(response, { supported: true });
      },
      async (origin) => {
        const nine = await new Discovery(
          config(origin, { gateway: "9router" }),
        ).model(undefined, "codex/image model");
        expect(nine).toMatchObject({
          connection: "local",
          model_id: "codex/image model",
          account_availability: "unknown",
          advertised: { supported: true },
        });
        const options = configuration(
          join(process.cwd(), ".ci-work", "metadata"),
          {
            local: connection("openrouter-images", {
              baseUrl: `${origin}/api/v1`,
              discoveryTimeoutMs: 2000,
            }),
          },
        );
        expect(
          (
            await new Discovery(options).model(
              undefined,
              "provider/image model",
            )
          ).advertised,
        ).toEqual({ supported: true });
        expect(paths).toEqual([
          "/v1/models/info?id=codex%2Fimage+model",
          "/api/v1/images/models/provider/image%20model/endpoints",
        ]);
      },
    );
    await server(
      (_request, response) => json(response, {}, 404),
      async (origin) => {
        expect(
          (
            await new Discovery(config(origin, { gateway: "9router" })).model(
              undefined,
              "missing",
            )
          ).advertised,
        ).toBeNull();
      },
    );
  });

  it("probes readiness independently from generation and never returns authentication sources", async () =>
    workspace(async (root) => {
      await server(
        (_request, response) => json(response, { data: [] }),
        async (origin) => {
          const options = configuration(root, {
            local: connection("openai-images", {
              baseUrl: `${origin}/v1`,
              discoveryTimeoutMs: 2000,
            }),
            disabled: connection("gemini", {
              enabled: false,
              auth: {
                type: "bearer",
                secretFile: join(root, "absent-fixture-secret.txt"),
              },
            }),
          });
          const listed = await listConnections(options);
          expect(listed).toEqual([
            {
              name: "local",
              adapter: "openai-images",
              gateway: null,
              enabled: true,
              credentials_ready: true,
              default_model: null,
            },
            {
              name: "disabled",
              adapter: "gemini",
              gateway: null,
              enabled: false,
              credentials_ready: false,
              default_model: null,
            },
          ]);
          const result = await health(options, true);
          expect(result.connections).toEqual([
            {
              name: "local",
              reachable: true,
              authentication_checked: false,
              generation_verified: false,
              error: null,
            },
            {
              name: "disabled",
              reachable: null,
              authentication_checked: false,
              generation_verified: false,
              error: null,
            },
          ]);
          expect(JSON.stringify(result)).not.toContain(
            "absent-fixture-secret.txt",
          );
          expect(result.generation_verified).toBe(false);
        },
      );
    }));
});

describe("bounded local HTTP and asset contracts", () => {
  it.each([
    [401, "authentication_error"],
    [403, "permission_denied"],
    [404, "model_unavailable"],
    [429, "rate_limited"],
    [500, "upstream_error"],
  ] as const)("maps HTTP %s to %s without retrying", async (status, code) => {
    let calls = 0;
    await server(
      (_request, response) => {
        calls++;
        json(response, { error: "private body never emitted" }, status);
      },
      async (origin) => {
        await expect(
          apiRequest(config(origin).connections.local!, "models", "/v1"),
        ).rejects.toMatchObject({ code, httpStatus: status });
        expect(calls).toBe(1);
      },
    );
  });

  it("rejects API redirects, malformed JSON, excessive bodies and cancelled requests", async () => {
    await server(
      (request, response) => {
        if (request.url?.endsWith("/redirect")) {
          response.writeHead(302, { Location: "/v1/final" });
          response.end();
        } else if (request.url?.endsWith("/malformed")) {
          response.writeHead(200, { "Content-Type": "application/json" });
          response.end("{");
        } else {
          response.writeHead(200, {
            "Content-Type": "application/octet-stream",
          });
          response.end(Buffer.alloc(5));
        }
      },
      async (origin) => {
        const profile = config(origin).connections.local!;
        await expect(
          apiRequest(profile, "redirect", "/v1"),
        ).rejects.toMatchObject({ code: "unsafe_destination" });
        await expect(
          apiJson(profile, "malformed", "/v1"),
        ).rejects.toMatchObject({ code: "invalid_response" });
        await expect(
          apiRequest(profile, "bytes", "/v1", {}, undefined, 4),
        ).rejects.toMatchObject({ code: "invalid_response" });
        await expect(
          apiRequest(profile, "bytes", "/v1", {}, AbortSignal.abort()),
        ).rejects.toMatchObject({ code: "cancelled" });
      },
    );
  });

  it("allows only configured loopback asset prefixes, revalidates redirects and returns exact bytes", async () => {
    const fixture = await imageBytes();
    const paths: string[] = [];
    await server(
      (request, response) => {
        const path = request.url ?? "";
        paths.push(path);
        if (path.endsWith("/redirect")) {
          response.writeHead(302, { Location: "/private/image.png" });
          response.end();
        } else if (path.endsWith("/loop")) {
          response.writeHead(302, { Location: "/assets/loop" });
          response.end();
        } else if (path.endsWith("/missing")) json(response, {}, 404);
        else {
          response.writeHead(200, { "Content-Type": "image/png" });
          response.end(fixture);
        }
      },
      async (origin) => {
        const options = config(origin);
        await expect(
          fetchAsset(`${origin}/assets/image.png`, options, 1000),
        ).rejects.toMatchObject({ code: "unsafe_destination" });
        options.files.assetAllowances = [{ origin, pathPrefix: "/assets" }];
        expect(
          await fetchAsset(
            `${origin}/assets/image.png`,
            options,
            fixture.length,
          ),
        ).toEqual(fixture);
        await expect(
          fetchAsset(`${origin}/assets-other/image.png`, options, 1000),
        ).rejects.toMatchObject({ code: "unsafe_destination" });
        await expect(
          fetchAsset(`${origin}/assets/redirect`, options, 1000),
        ).rejects.toMatchObject({ code: "unsafe_destination" });
        expect(paths).not.toContain("/private/image.png");
        await expect(
          fetchAsset(`${origin}/assets/missing`, options, 1000),
        ).rejects.toMatchObject({ code: "input_file_error" });
        await expect(
          fetchAsset(`${origin}/assets/loop`, options, 1000),
        ).rejects.toMatchObject({ code: "unsafe_destination" });
        await expect(
          fetchAsset(`${origin}/assets/image.png`, options, fixture.length - 1),
        ).rejects.toMatchObject({ code: "invalid_response" });
        for (const unsafe of [
          "not a URL",
          "file:///image.png",
          `${origin}/assets/image.png#fragment`,
          origin.replace("http://", "http://user:password@") +
            "/assets/image.png",
        ])
          await expect(fetchAsset(unsafe, options, 1000)).rejects.toMatchObject(
            { code: "unsafe_destination" },
          );
      },
    );
  });
});
