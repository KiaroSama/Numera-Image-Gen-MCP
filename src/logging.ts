import {
  mkdirSync,
  openSync,
  writeSync,
  closeSync,
  readdirSync,
  statSync,
  unlinkSync,
} from "node:fs";
import { join } from "node:path";
export const levels = ["DEBUG", "INFO", "WARNING", "ERROR"] as const;
export type Level = (typeof levels)[number];
const sensitive =
  /authorization|cookie|token|key|secret|password|signature|prompt|base64|b64_json|inlineData|data_url|path/i;
export function redact(value: unknown, depth = 0): unknown {
  if (depth > 8) return "[REDACTED]";
  if (typeof value === "string")
    return value
      .replace(/https?:\/\/[^\s"<>]+/g, (url) => {
        try {
          const u = new URL(url);
          u.search = "";
          u.username = "";
          u.password = "";
          return u.href;
        } catch {
          return "[REDACTED]";
        }
      })
      .replace(/Bearer\s+\S+/gi, "Bearer [REDACTED]");
  if (Array.isArray(value))
    return value.slice(0, 30).map((v) => redact(v, depth + 1));
  if (value && typeof value === "object")
    return Object.fromEntries(
      Object.entries(value).map(([k, v]) => [
        k,
        sensitive.test(k) ? "[REDACTED]" : redact(v, depth + 1),
      ]),
    );
  return value;
}
export class Logger {
  private fd?: number;
  readonly file?: string;
  constructor(
    directory: string,
    private level: Level = "INFO",
    retentionDays = 14,
  ) {
    try {
      mkdirSync(directory, { recursive: true, mode: 0o700 });
      const stamp = new Date()
        .toISOString()
        .replace("T", "_")
        .replaceAll(":", "-")
        .slice(0, 19);
      this.file = join(
        directory,
        `numera-image-gen-mcp_${stamp}_UTC-${process.pid}.log`,
      );
      this.fd = openSync(this.file, "ax", 0o600);
      for (const name of readdirSync(directory)) {
        if (
          /^numera-image-gen-mcp_.*\.log$/.test(name) &&
          Date.now() - statSync(join(directory, name)).mtimeMs >
            retentionDays * 86400000
        )
          unlinkSync(join(directory, name));
      }
    } catch {
      process.stderr.write(
        "WARNING: File logging unavailable; using stderr.\n",
      );
    }
  }
  log(
    level: Level,
    component: string,
    message: string,
    context: Record<string, unknown> = {},
  ) {
    if (levels.indexOf(level) < levels.indexOf(this.level)) return;
    const line =
      JSON.stringify({
        timestamp: new Date().toISOString(),
        level,
        component,
        message,
        ...(redact(context) as object),
      }) + "\n";
    if (this.fd !== undefined) {
      try {
        writeSync(this.fd, line, undefined, "utf8");
      } catch {
        this.close();
        process.stderr.write("WARNING: File logging failed.\n");
      }
    }
    process.stderr.write(line);
  }
  close() {
    if (this.fd !== undefined) {
      closeSync(this.fd);
      this.fd = undefined;
    }
  }
}
