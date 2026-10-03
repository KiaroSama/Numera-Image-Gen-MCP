import type { Connection } from "../config/schema.js";
import { credentials } from "../config/credentials.js";
import { fingerprint } from "../jobs/store.js";

export async function connectionIdentity(name: string, connection: Connection) {
  return fingerprint({
    connection: name,
    adapter: connection.adapter,
    gateway: connection.gateway,
    origin: new URL(connection.baseUrl).origin,
    base: connection.baseUrl,
    auth: connection.auth,
    account_fingerprint: fingerprint(await credentials(connection)),
  });
}
