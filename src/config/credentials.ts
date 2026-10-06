import { readFile, stat } from "node:fs/promises";
import { isAbsolute } from "node:path";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import type { Connection } from "./schema.js";
import { fail } from "../errors.js";
// Each operation binds identity checks and network requests to the same private headers.
// Weak keys keep secret values out of serialized configuration, receipts and persistent state.
const snapshots = new WeakMap<Connection, Readonly<Record<string, string>>>();

export async function snapshotCredentials(
  connection: Connection,
): Promise<Connection> {
  const snapshot = { ...connection, auth: { ...connection.auth } };
  snapshots.set(snapshot, Object.freeze(await credentials(connection)));
  return snapshot;
}

export async function credentials(
  connection: Connection,
  env: NodeJS.ProcessEnv = process.env,
): Promise<Record<string, string>> {
  const pinned = snapshots.get(connection);
  if (pinned) return { ...pinned };
  const auth = connection.auth;
  if (auth.type === "none") return {};
  let key: string | undefined;
  if (auth.apiKey !== undefined) key = auth.apiKey;
  else if (auth.secretEnv) key = env[auth.secretEnv];
  else if (auth.secretFile) {
    if (!isAbsolute(auth.secretFile))
      fail("invalid_configuration", "Secret file path must be absolute.");
    try {
      const info = await stat(auth.secretFile);
      if (
        !info.isFile() ||
        info.size > 65536 ||
        (process.platform !== "win32" && (info.mode & 0o077) !== 0)
      )
        fail("permission_denied", "Secret file must be private and bounded.");
      key = (await readFile(auth.secretFile, "utf8")).trim();
    } catch {
      fail(
        "missing_credentials",
        "Cannot read the declared private secret file.",
      );
    }
  } else if (auth.secretDpapiFile) {
    if (process.platform !== "win32" || !isAbsolute(auth.secretDpapiFile))
      fail(
        "invalid_configuration",
        "DPAPI requires an absolute Windows secret file.",
      );
    const script =
      '$ErrorActionPreference="Stop"; [Console]::OutputEncoding=[Text.UTF8Encoding]::new($false); $s=Get-Content -LiteralPath $env:NUMERA_DPAPI_FILE -Raw -Encoding utf8 | ConvertTo-SecureString; $p=[Runtime.InteropServices.Marshal]::SecureStringToBSTR($s); try {[Console]::Out.Write([Runtime.InteropServices.Marshal]::PtrToStringBSTR($p))} finally {[Runtime.InteropServices.Marshal]::ZeroFreeBSTR($p)}';
    try {
      const result = await promisify(execFile)(
        "pwsh",
        ["-NoProfile", "-NonInteractive", "-Command", script],
        {
          windowsHide: true,
          encoding: "utf8",
          timeout: 10000,
          maxBuffer: 65536,
          env: { ...env, NUMERA_DPAPI_FILE: auth.secretDpapiFile },
        },
      );
      key = result.stdout.trim();
    } catch {
      fail(
        "missing_credentials",
        "Cannot decrypt the declared DPAPI secret for this Windows user.",
      );
    }
  }
  if (!key || /[\r\n\x00]/.test(key))
    fail(
      "missing_credentials",
      "The selected connection secret is missing or invalid. Set its declared secret source.",
    );
  return {
    [auth.type === "bearer" ? "Authorization" : auth.header!]:
      auth.type === "bearer" ? `Bearer ${key}` : key,
  };
}
