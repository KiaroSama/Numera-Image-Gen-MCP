import { McpServer, ResourceTemplate } from "@modelcontextprotocol/server";
import { z } from "zod";
import { randomUUID } from "node:crypto";
import { requestSchema, type Config } from "../config/schema.js";
import { Store } from "../jobs/store.js";
import { Logger } from "../logging.js";
import { Generation } from "../services/generation.js";
import { Discovery } from "../services/discovery.js";
import { health, listConnections } from "../services/health.js";
import { getJob, cancelJob } from "../services/jobs.js";
import { ownedImage } from "../files/output.js";
import { preview } from "../files/images.js";
import { safeError } from "../errors.js";
const outputSchema = z.record(z.string(), z.unknown());
const pagination = {
  limit: z.number().int().min(1).max(100).default(25),
  offset: z.number().int().min(0).max(100000).default(0),
};
export function createServer(
  config: Config,
  logger: Logger,
  store = new Store(config.stateDir),
) {
  const generation = new Generation(config, store, logger),
    discovery = new Discovery(config);
  const server = new McpServer(
    { name: "numera-image-gen-mcp", version: "0.1.0" },
    {
      instructions:
        "Discover connection/model capabilities before images. Reuse one request_id per logical operation. Never resubmit unknown outcomes. References require verified forwarding; files are reliable outputs and local paths may not be accessible to remote hosts. Do not replace the host chat model.",
    },
  );
  const wrap = async (tool: string, fn: () => Promise<unknown>) => {
    const call_id = randomUUID(),
      started = performance.now();
    logger.log("INFO", "tool", "Tool started.", { tool, call_id });
    try {
      const value = await fn(),
        structuredContent = value as Record<string, unknown>;
      logger.log("INFO", "tool", "Tool completed.", {
        tool,
        call_id,
        duration_ms: Math.round(performance.now() - started),
        warning_count: Array.isArray(structuredContent.warnings)
          ? structuredContent.warnings.length
          : 0,
      });
      return {
        content: [{ type: "text" as const, text: JSON.stringify(value) }],
        structuredContent,
      };
    } catch (e) {
      const value = { error: safeError(e) };
      logger.log("ERROR", "tool", "Tool failed.", {
        tool,
        call_id,
        duration_ms: Math.round(performance.now() - started),
        code: value.error.code,
        stage: value.error.stage,
        http_status: value.error.http_status,
      });
      return {
        isError: true,
        content: [{ type: "text" as const, text: JSON.stringify(value) }],
        structuredContent: value,
      };
    }
  };
  const readAnnotations = {
    readOnlyHint: true,
    destructiveHint: false,
    idempotentHint: true,
    openWorldHint: true,
  };
  server.registerTool(
    "health_check",
    {
      description:
        "Check local runtime/configuration/storage; optional read-only connection probe. Never verifies generation without an actual generation.",
      inputSchema: z.object({ probe: z.boolean().default(false) }).strict(),
      outputSchema,
      annotations: readAnnotations,
    },
    async ({ probe }, ctx) =>
      wrap("health_check", () => health(config, probe, ctx.mcpReq.signal)),
  );
  server.registerTool(
    "list_connections",
    {
      description:
        "List redacted configured connections and credential readiness; no provider generation.",
      inputSchema: z.object({}).strict(),
      outputSchema,
      annotations: readAnnotations,
    },
    async () =>
      wrap("list_connections", async () => ({
        connections: await listConnections(config),
      })),
  );
  server.registerTool(
    "list_models",
    {
      description:
        "List configured model IDs/names without network when present; refresh additionally discovers bounded provider catalogs with evidence. A failed catalog does not prove explicit models unavailable.",
      inputSchema: z
        .object({
          connection: z.string().optional(),
          refresh: z.boolean().default(false),
          ...pagination,
        })
        .strict(),
      outputSchema,
      annotations: readAnnotations,
    },
    async ({ connection, refresh, limit, offset }, ctx) =>
      wrap("list_models", async () => {
        const result = await discovery.list(
          connection,
          refresh,
          ctx.mcpReq.signal,
        );
        return {
          ...result,
          models: result.models.slice(offset, offset + limit),
          next_offset:
            result.models.length > offset + limit ? offset + limit : null,
        };
      }),
  );
  server.registerTool(
    "get_model_capabilities",
    {
      description:
        "Separate model capability, gateway forwarding, account availability and verification evidence for one model.",
      inputSchema: z
        .object({ connection: z.string().optional(), model: z.string().min(1) })
        .strict(),
      outputSchema,
      annotations: readAnnotations,
    },
    async ({ connection, model }, ctx) =>
      wrap("get_model_capabilities", () =>
        discovery.model(connection, model, ctx.mcpReq.signal),
      ),
  );
  for (const [name, operation] of [
    ["generate_image", "generate"],
    ["edit_image", "edit"],
  ] as const) {
    server.registerTool(
      name,
      {
        description:
          operation === "generate"
            ? "Submit one authorized image generation, saving validated original files. Reuse request_id for duplicates; never retry an unknown outcome."
            : "Edit with at least one approved reference through a known input route. Unsupported references/masks fail before submission; no paid fallback.",
        inputSchema: requestSchema,
        outputSchema,
        annotations: {
          readOnlyHint: false,
          destructiveHint: false,
          idempotentHint: false,
          openWorldHint: true,
        },
      },
      async (args, ctx) => {
        const result = await wrap(name, () =>
          generation.run(args, operation, ctx.mcpReq.signal),
        );
        const content: Array<
          | { type: "text"; text: string }
          | {
              type: "resource_link";
              uri: string;
              name: string;
              mimeType: string;
            }
          | { type: "image"; data: string; mimeType: string }
        > = [...result.content];
        const receipt = result.structuredContent as Record<string, unknown>;
        if (Array.isArray(receipt.outputs))
          for (const raw of receipt.outputs) {
            const output = raw as { output_id: string; mime_type: string };
            content.push({
              type: "resource_link",
              uri: `numera-image://outputs/${output.output_id}`,
              name: output.output_id,
              mimeType: output.mime_type,
            });
            if (
              (args.return_mode ?? config.returnMode) === "files_and_preview"
            ) {
              try {
                const { image } = await ownedImage(
                  config,
                  store,
                  output.output_id,
                );
                const p = await preview(image, config);
                const previews = (receipt.previews ?? []) as Record<
                  string,
                  unknown
                >[];
                previews.push({
                  output_id: output.output_id,
                  width: p.width,
                  height: p.height,
                  mime_type: p.mimeType,
                });
                receipt.previews = previews;
                content.push({
                  type: "image",
                  data: p.data,
                  mimeType: p.mimeType,
                });
              } catch (e) {
                logger.log("WARNING", "preview", "Preview unavailable.", {
                  error: safeError(e),
                });
              }
            }
          }
        return {
          ...result,
          isError:
            result.isError ||
            ["failed", "outcome_unknown"].includes(String(receipt.status)),
          content,
        };
      },
    );
  }
  server.registerTool(
    "get_job",
    {
      description:
        "Inspect an owned persistent receipt, optionally refresh an existing upstream job; never resubmit.",
      inputSchema: z
        .object({ request_id: z.string(), refresh: z.boolean().default(false) })
        .strict(),
      outputSchema,
      annotations: readAnnotations,
    },
    async ({ request_id, refresh }, ctx) =>
      wrap("get_job", () =>
        getJob(generation, request_id, refresh, ctx.mcpReq.signal),
      ),
  );
  server.registerTool(
    "cancel_job",
    {
      description:
        "Stop local waiting for an owned request; request targeted upstream cancellation only when supported. Does not prove a refund.",
      inputSchema: z.object({ request_id: z.string() }).strict(),
      outputSchema,
      annotations: {
        readOnlyHint: false,
        destructiveHint: false,
        idempotentHint: true,
        openWorldHint: true,
      },
    },
    async ({ request_id }, ctx) =>
      wrap("cancel_job", () =>
        cancelJob(config, generation, request_id, ctx.mcpReq.signal),
      ),
  );
  server.registerTool(
    "list_outputs",
    {
      description:
        "List only Numera-owned output records with bounded pagination.",
      inputSchema: z.object(pagination).strict(),
      outputSchema,
      annotations: { ...readAnnotations, openWorldHint: false },
    },
    async ({ limit, offset }) =>
      wrap("list_outputs", async () => {
        const outputs = store.listOutputs(offset, limit);
        return {
          outputs: outputs.slice(0, limit),
          next_offset: outputs.length > limit ? offset + limit : null,
        };
      }),
  );
  server.registerTool(
    "get_output_info",
    {
      description:
        "Verify owned image bytes/hash/metadata and optionally return a bounded derivative preview.",
      inputSchema: z
        .object({
          output_id: z.string().uuid(),
          preview: z.boolean().default(false),
        })
        .strict(),
      outputSchema,
      annotations: { ...readAnnotations, openWorldHint: false },
    },
    async ({ output_id, preview: withPreview }) => {
      const result = await wrap("get_output_info", async () => {
        const { output } = await ownedImage(config, store, output_id);
        return { ...output, verified: true };
      });
      if (withPreview && !result.isError) {
        try {
          const { image } = await ownedImage(config, store, output_id),
            p = await preview(image, config);
          return {
            ...result,
            structuredContent: {
              ...result.structuredContent,
              preview: {
                width: p.width,
                height: p.height,
                mime_type: p.mimeType,
              },
            },
            content: [
              ...result.content,
              { type: "image" as const, data: p.data, mimeType: p.mimeType },
            ],
          };
        } catch (e) {
          return {
            ...result,
            structuredContent: {
              ...result.structuredContent,
              preview_error: safeError(e),
            },
          };
        }
      }
      return result;
    },
  );
  server.registerResource(
    "output",
    new ResourceTemplate("numera-image://outputs/{id}", { list: undefined }),
    {
      description: "Validated original bytes of an owned output.",
      mimeType: "application/octet-stream",
    },
    async (uri, { id }) => {
      const { image } = await ownedImage(config, store, String(id));
      return {
        contents: [
          {
            uri: uri.href,
            mimeType: image.mime,
            blob: image.bytes.toString("base64"),
          },
        ],
      };
    },
  );
  return { server, store, generation };
}
