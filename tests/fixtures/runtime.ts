import { mkdir, mkdtemp, rm } from "node:fs/promises";
import {
  createServer,
  type IncomingMessage,
  type ServerResponse,
} from "node:http";
import { join, resolve } from "node:path";
import sharp from "sharp";
import {
  configSchema,
  connectionSchema,
  type Config,
  type Connection,
} from "../../src/config/schema.js";
import type { Receipt } from "../../src/jobs/store.js";

export function connection(
  adapter: Connection["adapter"] = "openai-images",
  overrides: Record<string, unknown> = {},
): Connection {
  return connectionSchema.parse({
    adapter,
    baseUrl: "https://api.example/v1",
    auth: { type: "none" },
    ...overrides,
  });
}

export function configuration(
  root: string,
  connections: Record<string, Connection> = { local: connection() },
): Config {
  return configSchema.parse({
    schemaVersion: 1,
    defaultConnection: Object.keys(connections)[0],
    outputDir: join(root, "outputs"),
    stateDir: join(root, "state"),
    logging: { directory: join(root, "logs") },
    files: { allowedInputRoots: [join(root, "inputs")] },
    connections,
  });
}

export async function workspace<T>(
  run: (root: string) => Promise<T>,
): Promise<T> {
  const parent = resolve(".ci-work");
  await mkdir(parent, { recursive: true });
  const root = await mkdtemp(join(parent, "fixture-"));
  try {
    return await run(root);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

const images = new Map<string, Promise<Buffer>>();
export function imageBytes(
  format: "png" | "jpeg" | "webp" = "png",
  alpha = true,
  width = 8,
  height = 6,
): Promise<Buffer> {
  const key = `${format}:${alpha}:${width}:${height}`;
  let bytes = images.get(key);
  if (!bytes) {
    bytes = sharp({
      create: {
        width,
        height,
        channels: alpha ? 4 : 3,
        background: { r: 24, g: 100, b: 180, alpha: 0.5 },
      },
    })
      .toFormat(format)
      .toBuffer();
    images.set(key, bytes);
  }
  return bytes.then((value) => Buffer.from(value));
}

export function receipt(id = "request-1", name = "local"): Receipt {
  return {
    request_id: id,
    connection: name,
    adapter: "openai-images",
    operation: "generate",
    status: "prepared",
    requested_model: "image-model",
    upstream_model: null,
    outputs: [],
    warnings: [],
    deviations: [],
    errors: [],
    generation_outcome: "not_submitted",
    storage_outcome: "not_started",
    another_submission_may_charge: false,
    created_at: "2026-10-02T00:00:00.000Z",
    updated_at: "2026-10-02T00:00:00.000Z",
  };
}

export async function requestBody(request: IncomingMessage): Promise<Buffer> {
  const chunks: Buffer[] = [];
  let total = 0;
  for await (const chunk of request) {
    const bytes = Buffer.isBuffer(chunk)
      ? chunk
      : Buffer.from(String(chunk), "utf8");
    total += bytes.length;
    if (total > 1024 * 1024) throw new Error("Fixture request exceeds 1 MiB.");
    chunks.push(bytes);
  }
  return Buffer.concat(chunks);
}

export function json(
  response: ServerResponse,
  body: unknown,
  status = 200,
): void {
  response.writeHead(status, {
    "Content-Type": "application/json; charset=utf-8",
  });
  response.end(JSON.stringify(body));
}

export async function server<T>(
  handler: (
    request: IncomingMessage,
    response: ServerResponse,
  ) => void | Promise<void>,
  run: (origin: string) => Promise<T>,
): Promise<T> {
  const errors: unknown[] = [];
  const http = createServer((request, response) => {
    void Promise.resolve()
      .then(() => handler(request, response))
      .catch((error) => {
        errors.push(error);
        if (!response.headersSent) response.writeHead(500);
        response.end();
      });
  });
  http.requestTimeout = 5000;
  http.headersTimeout = 5000;
  await new Promise<void>((accept, reject) => {
    http.once("error", reject);
    http.listen(0, "127.0.0.1", accept);
  });
  const address = http.address();
  if (!address || typeof address === "string")
    throw new Error("Fixture server has no TCP address.");
  try {
    const result = await run(`http://127.0.0.1:${address.port}`);
    if (errors.length) throw errors[0];
    return result;
  } finally {
    const closed = new Promise<void>((accept, reject) =>
      http.close((error) => (error ? reject(error) : accept())),
    );
    http.closeAllConnections();
    await closed;
  }
}
