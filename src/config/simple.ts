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
          })
          .strict(),
      )
      .min(1)
      .max(100),
    profile: z
      .enum(["omniroute", "9router", "openai-images"])
      .default("omniroute"),
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
        adapter: "openai-images",
        ...(c.profile === "openai-images" ? {} : { gateway: c.profile }),
        baseUrl: c.api_endpoint,
        auth: {
          type: "bearer",
          apiKey: c.api_key,
          origin: new URL(c.api_endpoint).origin,
        },
        defaultModel: c.models[0]!.id,
        configuredModels: c.models,
      },
    },
  };
}
