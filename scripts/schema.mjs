import { z } from "zod";
import { configSchema } from "../dist/config/schema.js";
import { mkdir, writeFile } from "node:fs/promises";
await mkdir("schemas", { recursive: true });
await writeFile(
  "schemas/config.schema.json",
  JSON.stringify(z.toJSONSchema(configSchema), null, 2) + "\n",
  "utf8",
);
process.stderr.write("INFO: Configuration schema generated.\n");
