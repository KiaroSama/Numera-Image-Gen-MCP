import { it, expect } from "vitest";
import { apiRequest } from "../../src/http/client.js";
import { buildRequest, normalizeResponse } from "../../src/adapters/index.js";
import { requestSchema } from "../../src/config/schema.js";
import {
  connection,
  server,
  json,
  requestBody,
  imageBytes,
} from "../fixtures/runtime.js";
import { inspectImage } from "../../src/files/images.js";
import { configuration } from "../fixtures/runtime.js";
import { resolve } from "node:path";

it.each([
  "openai-images",
  "gemini",
  "gemini-interactions",
  "openai-responses",
  "openrouter-images",
  "chat-images",
] as const)(
  "%s crosses exact HTTP boundary and parses final bytes",
  async (adapter) => {
    const signal = AbortSignal.timeout(5000);
    const bytes = await imageBytes();
    let seen = false;
    await server(
      async (req, res) => {
        seen = true;
        expect(req.method).toBe("POST");
        const body = JSON.parse(
          (await requestBody(req)).toString("utf8"),
        ) as Record<string, unknown>;
        expect(JSON.stringify(body)).toContain("اصل Unicode 123");
        expect(req.headers.authorization).toBe("Bearer fixture-key");
        if (adapter === "gemini") {
          expect(req.url).toBe("/v1beta/models/family%2Fimage:generateContent");
          json(res, {
            candidates: [
              {
                content: {
                  parts: [
                    {
                      inlineData: {
                        data: bytes.toString("base64"),
                        mimeType: "image/png",
                      },
                    },
                  ],
                },
              },
            ],
          });
        } else if (adapter === "gemini-interactions") {
          expect(req.url).toBe("/v1beta/interactions");
          expect(body.store).toBe(false);
          json(res, {
            status: "completed",
            steps: [
              {
                type: "model_output",
                content: [{ type: "image", data: bytes.toString("base64") }],
              },
            ],
          });
        } else if (adapter === "openai-responses") {
          expect(req.url).toBe("/v1/responses");
          expect(body.model).toBe("orchestrator");
          expect(body.tools).toEqual([
            expect.objectContaining({
              model: "family/image",
              type: "image_generation",
            }),
          ]);
          json(res, {
            status: "completed",
            output: [
              {
                type: "image_generation_call",
                status: "completed",
                result: bytes.toString("base64"),
              },
            ],
          });
        } else if (adapter === "chat-images") {
          expect(req.url).toBe("/v1/chat/completions");
          json(res, {
            choices: [
              {
                message: {
                  images: [
                    {
                      image_url: {
                        url:
                          "data:image/png;base64," + bytes.toString("base64"),
                      },
                    },
                  ],
                },
              },
            ],
          });
        } else {
          expect(req.url).toBe(
            adapter === "openrouter-images"
              ? "/api/v1/images"
              : "/v1/images/generations",
          );
          json(res, { data: [{ b64_json: bytes.toString("base64") }] });
        }
      },
      async (origin) => {
        const c = connection(adapter, {
          baseUrl: origin,
          baseUrlMode: "origin",
          auth: { type: "bearer", secretEnv: "NUMERA_WIRE_FIXTURE" },
          orchestrationModel: "orchestrator",
          chatImageOutput: true,
        });
        const old = process.env.NUMERA_WIRE_FIXTURE;
        process.env.NUMERA_WIRE_FIXTURE = "fixture-key";
        try {
          const built = buildRequest({
            connection: c,
            request: requestSchema.parse({ prompt: "اصل Unicode 123" }),
            model: "family/image",
            operation: "generate",
            references: [],
          });
          const response = await apiRequest(
            c,
            built.operation,
            built.prefix,
            { method: "POST", body: built.body, headers: built.headers },
            signal,
          );
          expect(
            normalizeResponse(c, response.bytes, response.mime).images,
          ).toHaveLength(1);
        } finally {
          if (old === undefined) delete process.env.NUMERA_WIRE_FIXTURE;
          else process.env.NUMERA_WIRE_FIXTURE = old;
        }
      },
    );
    expect(seen).toBe(true);
  },
);
it("multipart edits preserve both original file and alpha mask bytes at HTTP boundary", async ({
  signal,
}) => {
  const bytes = await imageBytes(),
    image = await inspectImage(bytes, configuration(resolve(".ci-work/wire")));
  await server(
    async (req, res) => {
      expect(req.url).toBe("/v1/images/edits");
      expect(req.headers["content-type"]).toContain(
        "multipart/form-data; boundary=",
      );
      const body = await requestBody(req);
      expect(body.includes(bytes)).toBe(true);
      expect(body.toString("latin1")).toContain('name="mask"');
      json(res, { data: [{ b64_json: bytes.toString("base64") }] });
    },
    async (origin) => {
      const c = connection("openai-images", {
        baseUrl: origin + "/v1",
        edit: {
          mode: "multipart",
          encoding: "image",
          maxReferences: 1,
          masks: true,
          maskPolarity: "transparent-edit",
        },
      });
      const built = buildRequest({
        connection: c,
        request: requestSchema.parse({ prompt: "edit" }),
        model: "opaque",
        operation: "edit",
        references: [image],
        mask: image,
      });
      await apiRequest(
        c,
        built.operation,
        built.prefix,
        { method: "POST", body: built.body, headers: built.headers },
        signal,
      );
    },
  );
});
