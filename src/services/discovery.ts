import type { Config, Connection } from "../config/schema.js";
import { selectConnection } from "../config/load.js";
import { apiJson } from "../http/client.js";
import { array, record, safeError } from "../errors.js";
import { capabilities } from "../capabilities.js";
export type Catalog = {
  models: { id: string; metadata: Record<string, unknown> }[];
  complete: boolean;
  warnings: unknown[];
  cached: boolean;
  checked_at: string;
};
export class Discovery {
  private cache = new Map<string, { until: number; catalog: Catalog }>();
  constructor(private config: Config) {}
  async list(
    name?: string,
    refresh = false,
    signal?: AbortSignal,
  ): Promise<Catalog> {
    const [id, c] = selectConnection(this.config, name),
      old = this.cache.get(id);
    if (!refresh && old && old.until > Date.now())
      return { ...old.catalog, cached: true };
    const result: Catalog = {
      models: [],
      complete: true,
      warnings: [],
      cached: false,
      checked_at: new Date().toISOString(),
    };
    const route =
      c.gateway === "omniroute"
        ? "images/generations"
        : c.gateway === "9router"
          ? "models/image"
          : c.adapter === "openrouter-images"
            ? "images/models"
            : c.adapter === "comfyui"
              ? "object_info"
              : "models";
    const prefix =
      c.adapter === "comfyui"
        ? ""
        : c.adapter.startsWith("gemini")
          ? "/v1beta"
          : c.adapter === "openrouter-images"
            ? "/api/v1"
            : "/v1";
    let operation = route;
    let cursor: string | undefined;
    const seen = new Set<string>();
    try {
      for (let page = 0; page < 20; page++) {
        const query: Record<string, string> = cursor
          ? c.adapter.startsWith("gemini")
            ? { pageToken: cursor }
            : { after: cursor }
          : {};
        if (c.gateway === "omniroute" && operation === "models")
          query.limit = "100";
        const value = record(
          await apiJson(c, operation, prefix, { query }, signal),
        );
        if (c.adapter === "comfyui") {
          result.models = Object.keys(value).map((id) => ({
            id,
            metadata: record(value[id]),
          }));
          break;
        }
        const items = array(value.data ?? value.models);
        for (const raw of items) {
          const item = record(raw),
            model = String(item.id ?? item.name ?? "");
          if (
            !model ||
            (c.gateway === "omniroute" &&
              operation === "models" &&
              item.type !== "image")
          )
            continue;
          if (!result.models.some((m) => m.id === model))
            result.models.push({ id: model, metadata: item });
        }
        if (c.gateway === "omniroute" && operation === route) {
          operation = "models";
          result.warnings.push(
            "Image specialty catalog does not preserve pagination; explicitly supplementing with paginated unified image entries.",
          );
          continue;
        }
        const next =
          value.nextPageToken ??
          value.next_page_token ??
          (value.has_more ? value.last_id : undefined);
        if (!next) break;
        const token = String(next);
        if (seen.has(token)) {
          result.complete = false;
          result.warnings.push("Repeated catalog cursor.");
          break;
        }
        seen.add(token);
        cursor = token;
        if (c.adapter.startsWith("gemini"))
          operation = c.paths.models ?? "models";
        if (page === 19) {
          result.complete = false;
          result.warnings.push("Catalog page limit reached.");
        }
      }
    } catch (e) {
      result.complete = false;
      result.warnings.push(safeError(e));
      if (old) {
        result.models = old.catalog.models;
        result.warnings.push("Using stale catalog; availability is unknown.");
      }
    }
    this.cache.set(id, { until: Date.now() + 60000, catalog: result });
    return result;
  }
  async model(name: string | undefined, model: string, signal?: AbortSignal) {
    const [id, c] = selectConnection(this.config, name);
    const cap = capabilities(c, model);
    let advertised: unknown = null;
    try {
      if (c.gateway === "9router")
        advertised = await apiJson(
          c,
          "models/info",
          "/v1",
          { query: { id: model } },
          signal,
        );
      else if (c.adapter === "openrouter-images")
        advertised = await apiJson(
          c,
          `images/models/${model.split("/").map(encodeURIComponent).join("/")}/endpoints`,
          "/api/v1",
          {},
          signal,
        );
    } catch {}
    return { connection: id, model_id: model, ...cap, advertised };
  }
}
export function connectionPrefix(c: Connection) {
  return c.adapter === "comfyui"
    ? ""
    : c.adapter.startsWith("gemini")
      ? "/v1beta"
      : c.adapter === "openrouter-images"
        ? "/api/v1"
        : "/v1";
}
