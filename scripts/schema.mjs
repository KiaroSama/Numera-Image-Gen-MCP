import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { maintenance, projectRoot } from "./logging.mjs";
await maintenance("schema", async (log) => {
  const { z } = await import("zod");
  const { configSchema } = await import("../dist/config/schema.js");
  await mkdir(join(projectRoot, "schemas"), { recursive: true });
  await writeFile(
    join(projectRoot, "schemas/config.schema.json"),
    JSON.stringify(z.toJSONSchema(configSchema), null, 2) + "\n",
    "utf8",
  );
  log.emit("INFO", "Configuration schema generated.");
});
