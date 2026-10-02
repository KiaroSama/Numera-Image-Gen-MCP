import { DatabaseSync } from "node:sqlite";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { createHash, randomUUID } from "node:crypto";
import { fail } from "../errors.js";
export function fingerprint(value: unknown): string {
  const stable = (v: unknown): unknown =>
    Array.isArray(v)
      ? v.map(stable)
      : v && typeof v === "object"
        ? Object.fromEntries(
            Object.entries(v)
              .filter(([, x]) => x !== undefined)
              .sort(([a], [b]) => a.localeCompare(b, "en"))
              .map(([k, x]) => [k, stable(x)]),
          )
        : v;
  return createHash("sha256")
    .update(JSON.stringify(stable(value)), "utf8")
    .digest("hex");
}
export type Output = {
  output_id: string;
  request_id: string;
  path: string;
  mime_type: string;
  width: number;
  height: number;
  has_alpha: boolean;
  bytes: number;
  sha256: string;
};
export type Receipt = {
  request_id: string;
  connection: string;
  adapter: string;
  operation: string;
  status: string;
  requested_model: string;
  upstream_model: string | null;
  outputs: Output[];
  warnings: string[];
  deviations: string[];
  errors: unknown[];
  generation_outcome: string;
  storage_outcome: string;
  another_submission_may_charge: boolean;
  upstream_job?: { id: string; kind: string };
  upstream_request_id?: string;
  output_requirements?: {
    count: number;
    size?: string;
    output_format?: "png" | "jpeg" | "webp";
    background?: string;
    output_subdirectory?: string;
    filename_prefix?: string;
  };
  created_at: string;
  updated_at: string;
  usage?: unknown;
};
export class Store {
  private db: DatabaseSync;
  readonly owner = randomUUID();
  constructor(directory: string) {
    mkdirSync(directory, { recursive: true, mode: 0o700 });
    this.db = new DatabaseSync(join(directory, "receipts.sqlite"), {
      timeout: 3000,
    });
    this.db.exec(`PRAGMA journal_mode=WAL;PRAGMA synchronous=FULL;
    CREATE TABLE IF NOT EXISTS requests(id TEXT PRIMARY KEY,fingerprint TEXT NOT NULL,identity TEXT NOT NULL,receipt TEXT NOT NULL,owner TEXT,lease INTEGER,cancel INTEGER NOT NULL DEFAULT 0);
    CREATE TABLE IF NOT EXISTS outputs(id TEXT PRIMARY KEY,request_id TEXT NOT NULL,metadata TEXT NOT NULL);
    CREATE INDEX IF NOT EXISTS output_request ON outputs(request_id);`);
  }
  private transaction<T>(fn: () => T): T {
    this.db.exec("BEGIN IMMEDIATE");
    try {
      const result = fn();
      this.db.exec("COMMIT");
      return result;
    } catch (e) {
      this.db.exec("ROLLBACK");
      throw e;
    }
  }
  prepare(
    receipt: Receipt,
    hash: string,
    identity: string,
  ): { fresh: boolean; receipt: Receipt } {
    return this.transaction(() => {
      const old = this.db
        .prepare(
          "SELECT fingerprint,identity,receipt,lease FROM requests WHERE id=?",
        )
        .get(receipt.request_id);
      if (old) {
        if (old.fingerprint !== hash || old.identity !== identity)
          fail(
            "request_id_conflict",
            "Request ID is already bound to different effective inputs or connection identity.",
          );
        let saved = JSON.parse(String(old.receipt)) as Receipt;
        if (
          ["prepared", "submitting", "running"].includes(saved.status) &&
          Number(old.lease) < Date.now()
        ) {
          saved = {
            ...saved,
            status: saved.upstream_job ? "running" : "outcome_unknown",
            generation_outcome: saved.upstream_job ? "running" : "unknown",
          };
          this.update(saved);
        }
        return { fresh: false, receipt: saved };
      }
      this.db
        .prepare(
          "INSERT INTO requests(id,fingerprint,identity,receipt,owner,lease) VALUES(?,?,?,?,?,?)",
        )
        .run(
          receipt.request_id,
          hash,
          identity,
          JSON.stringify(receipt),
          this.owner,
          Date.now() + 3600000,
        );
      return { fresh: true, receipt };
    });
  }
  get(id: string): Receipt {
    const row = this.db
      .prepare("SELECT receipt FROM requests WHERE id=?")
      .get(id);
    if (!row) fail("invalid_input", "Unknown owned request ID.");
    return JSON.parse(String(row.receipt)) as Receipt;
  }
  update(receipt: Receipt) {
    receipt.updated_at = new Date().toISOString();
    this.db
      .prepare("UPDATE requests SET receipt=? WHERE id=?")
      .run(JSON.stringify(receipt), receipt.request_id);
  }
  admit(
    id: string,
    connection: string,
    global: number,
    perConnection: number,
  ): boolean {
    return this.transaction(() => {
      const all = this.db
        .prepare("SELECT receipt FROM requests WHERE lease>?")
        .all(Date.now())
        .map((row) => JSON.parse(String(row.receipt)) as Receipt)
        .filter((r) => r.status === "submitting" || r.status === "running");
      if (
        all.length >= global ||
        all.filter((r) => r.connection === connection).length >= perConnection
      )
        return false;
      const r = this.get(id);
      r.status = "submitting";
      this.update(r);
      return true;
    });
  }
  claimFinalization(id: string): boolean {
    return this.transaction(() => {
      const r = this.get(id);
      if (!["running", "outcome_unknown"].includes(r.status)) return false;
      r.status = "finalizing";
      this.update(r);
      return true;
    });
  }
  queued() {
    return this.db
      .prepare("SELECT receipt FROM requests")
      .all()
      .map((r) => JSON.parse(String(r.receipt)) as Receipt)
      .filter((r) => r.status === "prepared").length;
  }
  cancel(id: string) {
    this.get(id);
    this.db.prepare("UPDATE requests SET cancel=1 WHERE id=?").run(id);
  }
  cancelled(id: string) {
    return (
      Number(
        this.db.prepare("SELECT cancel FROM requests WHERE id=?").get(id)
          ?.cancel,
      ) === 1
    );
  }
  addOutput(output: Output) {
    this.db
      .prepare("INSERT INTO outputs VALUES(?,?,?)")
      .run(output.output_id, output.request_id, JSON.stringify(output));
  }
  output(id: string): Output {
    const row = this.db
      .prepare("SELECT metadata FROM outputs WHERE id=?")
      .get(id);
    if (!row) fail("invalid_input", "Unknown owned output ID.");
    return JSON.parse(String(row.metadata)) as Output;
  }
  listOutputs(offset = 0, limit = 25) {
    return this.db
      .prepare(
        "SELECT metadata FROM outputs ORDER BY rowid DESC LIMIT ? OFFSET ?",
      )
      .all(limit + 1, offset)
      .map((row) => JSON.parse(String(row.metadata)) as Output);
  }
  close() {
    this.db.close();
  }
}
