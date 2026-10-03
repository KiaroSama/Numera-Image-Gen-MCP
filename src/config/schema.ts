import { z } from "zod";
const positive = z.number().int().positive();
const options = z.record(z.string(), z.unknown());
export const support = z.enum(["supported", "unsupported", "unknown"]);
export const authSchema = z
  .object({
    type: z.enum(["none", "bearer", "header"]),
    apiKey: z.string().min(1).max(65536).optional(),
    secretEnv: z.string().optional(),
    secretFile: z.string().optional(),
    secretDpapiFile: z.string().optional(),
    header: z.string().optional(),
    origin: z.string().optional(),
  })
  .strict();
export const connectionSchema = z
  .object({
    enabled: z.boolean().default(true),
    adapter: z.enum([
      "openai-images",
      "gemini",
      "gemini-interactions",
      "openai-responses",
      "openrouter-images",
      "chat-images",
      "comfyui",
    ]),
    gateway: z.enum(["omniroute", "9router"]).optional(),
    baseUrl: z.string().url(),
    baseUrlMode: z.enum(["api-prefix", "origin"]).default("api-prefix"),
    auth: authSchema,
    gatewayVersion: z.string().optional(),
    defaultModel: z.string().min(1).optional(),
    configuredModels: z
      .array(
        z
          .object({
            id: z.string().min(1).max(300),
            name: z.string().min(1).max(100),
          })
          .strict(),
      )
      .max(100)
      .default([]),
    orchestrationModel: z.string().optional(),
    allowedModels: z.array(z.string()).optional(),
    deniedModels: z.array(z.string()).default([]),
    paths: z.record(z.string(), z.string()).default({}),
    query: z.record(z.string(), z.string()).default({}),
    defaults: options.default({}),
    extensions: options.default({}),
    providerOptionKeys: z.array(z.string()).default([]),
    requestTimeoutMs: positive.max(3600000).default(300000),
    discoveryTimeoutMs: positive.max(60000).default(15000),
    maxConcurrentRequests: positive.max(16).default(2),
    modelOverrides: z
      .record(
        z.string(),
        z
          .object({
            defaults: options.default({}),
            capabilities: options.default({}),
            maxReferences: positive.max(32).optional(),
            maxCount: positive.max(10).optional(),
            supportedParameters: z.array(z.string()).optional(),
            evidence: z.string().optional(),
          })
          .strict(),
      )
      .default({}),
    edit: z
      .object({
        mode: z.enum(["multipart", "json", "generation"]),
        encoding: z.enum([
          "image",
          "image[]",
          "images",
          "image_urls",
          "input_references",
        ]),
        maxReferences: positive.max(32).default(1),
        masks: z.boolean().default(false),
        maskPolarity: z
          .enum(["transparent-edit", "white-edit", "black-edit"])
          .optional(),
      })
      .strict()
      .optional(),
    chatImageOutput: z.boolean().default(false),
    proxy: z
      .object({ url: z.string().url(), secretEnv: z.string().optional() })
      .strict()
      .optional(),
    workflow: z
      .object({
        graph: options,
        bindings: z.record(
          z.string(),
          z.object({ node: z.string(), input: z.string() }).strict(),
        ),
        outputNodes: z.array(z.string()).min(1),
        requireGpu: z.boolean().default(true),
      })
      .strict()
      .optional(),
  })
  .strict();
export const configSchema = z
  .object({
    schemaVersion: z.literal(1),
    defaultConnection: z.string().optional(),
    outputDir: z.string(),
    stateDir: z.string(),
    logging: z
      .object({
        directory: z.string(),
        level: z.enum(["DEBUG", "INFO", "WARNING", "ERROR"]).default("INFO"),
        logPrompts: z.literal(false).default(false),
        retentionDays: positive.default(14),
      })
      .strict(),
    files: z
      .object({
        allowedInputRoots: z.array(z.string()).default([]),
        assetAllowances: z
          .array(
            z
              .object({
                origin: z.string().url(),
                pathPrefix: z.string().startsWith("/"),
              })
              .strict(),
          )
          .default([]),
        maxInputBytes: positive.default(20 * 1024 * 1024),
        maxOutputBytes: positive.default(50 * 1024 * 1024),
        maxAggregateBytes: positive.default(100 * 1024 * 1024),
        maxPixels: positive.default(64000000),
        maxReferences: positive.max(32).default(14),
        previewMaxBytes: positive.default(262144),
        previewMaxDimension: positive.default(512),
      })
      .strict()
      .prefault({}),
    maxConcurrentRequests: positive.max(32).default(4),
    maxQueuedRequests: positive.max(128).default(16),
    returnMode: z.enum(["files", "files_and_preview"]).default("files"),
    connections: z.record(
      z.string().regex(/^[a-zA-Z0-9_-]{1,64}$/),
      connectionSchema,
    ),
  })
  .strict();
export type Config = z.infer<typeof configSchema>;
export type Connection = z.infer<typeof connectionSchema>;
export const sourceSchema = z.discriminatedUnion("type", [
  z
    .object({ type: z.literal("output_id"), output_id: z.string().uuid() })
    .strict(),
  z.object({ type: z.literal("path"), path: z.string().min(1) }).strict(),
  z.object({ type: z.literal("url"), url: z.string().url() }).strict(),
  z
    .object({ type: z.literal("data_url"), data_url: z.string().max(30000000) })
    .strict(),
]);
export const requestSchema = z
  .object({
    prompt: z
      .string()
      .min(1)
      .max(100000)
      .refine((s) => s.trim().length > 0),
    connection: z.string().optional(),
    model: z.string().min(1).max(300).optional(),
    request_id: z
      .string()
      .regex(/^[a-zA-Z0-9_-]{1,100}$/)
      .optional(),
    count: z.number().int().min(1).max(10).default(1),
    size: z.string().optional(),
    aspect_ratio: z.string().optional(),
    image_size: z.string().optional(),
    quality: z.string().optional(),
    output_format: z.enum(["png", "jpeg", "webp"]).optional(),
    background: z.string().optional(),
    seed: z.number().int().optional(),
    negative_prompt: z.string().optional(),
    provider_options: options.default({}),
    reference_images: z.array(sourceSchema).max(32).default([]),
    mask: sourceSchema.optional(),
    edit_region: z
      .object({
        x: z.number().int().nonnegative().max(64000000),
        y: z.number().int().nonnegative().max(64000000),
        width: positive.max(64000000),
        height: positive.max(64000000),
      })
      .strict()
      .optional(),
    target_language: z.string().min(1).max(100).regex(/\S/).optional(),
    output_subdirectory: z.string().optional(),
    filename_prefix: z.string().optional(),
    return_mode: z.enum(["files", "files_and_preview"]).optional(),
    wait: z.boolean().default(true),
  })
  .strict();
export type ImageRequest = z.infer<typeof requestSchema>;
