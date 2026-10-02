import type { FormData } from "undici";
import type { Connection, ImageRequest } from "../config/schema.js";
import type { Image } from "../files/images.js";
export type Operation = "generate" | "edit";
export type Prepared = {
  operation: string;
  prefix: string;
  body: string | FormData;
  headers: Record<string, string>;
  query?: Record<string, string>;
};
export type ResultItem = {
  base64?: string;
  url?: string;
  bytes?: Buffer;
  error?: string;
};
export type Normalized = {
  images: ResultItem[];
  upstreamModel: string | null;
  upstreamId?: string;
  usage?: unknown;
  job?: { id: string; kind: string };
  continuation?: unknown;
  warnings: string[];
};
export type AdapterInput = {
  connection: Connection;
  request: ImageRequest;
  model: string;
  operation: Operation;
  references: Image[];
  mask?: Image;
};
export const mimeData = (image: Image) =>
  `data:${image.mime};base64,${image.bytes.toString("base64")}`;
export const jsonRequest = (
  operation: string,
  prefix: string,
  body: unknown,
): Prepared => ({
  operation,
  prefix,
  body: JSON.stringify(body),
  headers: { "Content-Type": "application/json" },
});
