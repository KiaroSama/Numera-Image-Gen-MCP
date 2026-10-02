import { mkdirSync, openSync, writeSync, closeSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { randomUUID } from "node:crypto";

export const projectRoot = resolve(
  dirname(fileURLToPath(import.meta.url)),
  "..",
);
export function createLog(name) {
  const run = randomUUID();
  let fd;
  function emit(level, message, context = {}) {
    const entry =
      JSON.stringify({
        timestamp: new Date().toISOString(),
        level,
        component: name.toUpperCase(),
        run_id: run,
        message,
        ...context,
      }) + "\n";
    try {
      if (fd !== undefined) writeSync(fd, entry, null, "utf8");
    } catch {
      close();
      process.stderr.write("WARNING: File logging unavailable.\n");
    }
    process.stderr.write(entry);
  }
  function close() {
    if (fd !== undefined) {
      const current = fd;
      fd = undefined;
      try {
        closeSync(current);
      } catch {}
    }
  }
  try {
    const directory = resolve(
      process.env.NUMERA_LOG_DIR ?? resolve(projectRoot, "logs"),
    );
    mkdirSync(directory, { recursive: true });
    const stamp = new Date()
      .toISOString()
      .slice(0, 19)
      .replace("T", "_")
      .replaceAll(":", "-");
    fd = openSync(
      resolve(directory, `${name}_${stamp}_UTC-${run}.log`),
      "wx",
      0o600,
    );
  } catch {
    process.stderr.write("WARNING: File logging unavailable.\n");
  }
  return {
    emit,
    close,
    failure(error) {
      // Error text, stacks and native command output may contain credentials or private paths.
      emit("ERROR", "Maintenance operation failed.", {
        code:
          typeof error?.code === "string" &&
          /^[A-Z][A-Z0-9_]{0,40}$/.test(error.code)
            ? error.code
            : "OPERATION_FAILED",
      });
    },
  };
}
export async function maintenance(name, run) {
  const log = createLog(name);
  const started = Date.now();
  log.emit("INFO", "Maintenance operation started.", { pid: process.pid });
  try {
    await run(log);
  } catch (error) {
    log.failure(error);
    process.exitCode = 1;
  } finally {
    log.emit("INFO", "Maintenance operation finished.", {
      exit_code: process.exitCode ?? 0,
      duration_ms: Date.now() - started,
    });
    log.close();
  }
}
