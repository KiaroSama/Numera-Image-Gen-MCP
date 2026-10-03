import { dirname, join } from "node:path";
import { z } from "zod";
import { fail } from "../errors.js";
export const simpleConfigSchema = z
  .object({
    api_endpoint: z.string().url(),
    api_key: z.string().min(1).max(65536),
    models: z
      .array(
        z
          .object({
            id: z.string().min(1).max(300),
            name: z.string().min(1).max(100),
            output_format: z.enum(["png", "jpeg", "webp"]).optional(),
            size: z.string().min(1).max(100).regex(/\S/).optional(),
            quality: z.string().min(1).max(100).regex(/\S/).optional(),
          })
          .strict(),
      )
      .min(1)
      .max(100),
    orchestration_model: z.string().min(1).max(300).regex(/\S/).optional(),
    profile: z
      .enum(["omniroute", "9router", "openai-images", "openai-responses"])
      .default("openai-images"),
  })
  .strict();
export function expandSimple(
  raw: unknown,
  file: string,
): Record<string, unknown> {
  const parsed = simpleConfigSchema.safeParse(raw);
  if (!parsed.success)
    fail(
      "invalid_configuration",
      "Simple configuration requires api_endpoint, api_key and models with id/name.",
    );
  const c = parsed.data;
  if (
    !c.api_key.trim() ||
    /[\r\n\x00]/.test(c.api_key) ||
    c.models.some((m) => !m.id.trim() || !m.name.trim()) ||
    new Set(c.models.map((m) => m.id)).size !== c.models.length
  )
    fail(
      "invalid_configuration",
      "Key and model IDs/names must be nonempty; model IDs must be unique.",
    );
  if (
    (c.profile === "openai-responses") !==
    (c.orchestration_model !== undefined)
  )
    fail(
      "invalid_configuration",
      "orchestration_model is required only for the openai-responses profile.",
    );
  const root = dirname(file);
  return {
    schemaVersion: 1,
    defaultConnection: "default",
    outputDir: join(root, "outputs"),
    stateDir: join(root, "state"),
    logging: { directory: join(root, "logs") },
    files: { allowedInputRoots: [join(root, "inputs")] },
    connections: {
      default: {
        adapter: c.profile === "openai-responses" ? c.profile : "openai-images",
        orchestrationModel: c.orchestration_model,
        ...(["omniroute", "9router"].includes(c.profile)
          ? { gateway: c.profile }
          : {}),
        baseUrl: c.api_endpoint,
        auth: {
          type: "bearer",
          apiKey: c.api_key,
          origin: new URL(c.api_endpoint).origin,
        },
        defaultModel: c.models[0]!.id,
        configuredModels: c.models.map(({ id, name }) => ({ id, name })),
        modelOverrides: Object.fromEntries(
          c.models.map(({ id, name: _name, ...defaults }) => [
            id,
            { defaults },
          ]),
        ),
      },
    },
  };
}
