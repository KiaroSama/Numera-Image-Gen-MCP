import { DatabaseSync } from "node:sqlite";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";
import { createHash, randomUUID } from "node:crypto";
import { fail } from "../errors.js";
import type { snapshotRequirements } from "../services/output-requirements.js";
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
    upscale_source?: { width: number; height: number };
    output_format?: "png" | "jpeg" | "webp";
    background?: string;
    output_subdirectory?: string;
    filename_prefix?: string;
  } & Partial<ReturnType<typeof snapshotRequirements>>;
  created_at: string;
  updated_at: string;
  usage?: unknown;
};
export type ResultItem = {
  index: number;
  output_id: string;
  target: string | null;
  bytes: Buffer | null;
  url: string | null;
  sha256: string | null;
  phase: string;
};
const writerGeneration = Symbol("request-writer-generation");
const busy = ["prepared", "submitting", "running", "finalizing"];
export class Store {
  static readonly leaseMs = 30000;
  private db: DatabaseSync;
  private generations = new Map<string, number>();
  private stamp(receipt: Receipt, generation: number): Receipt {
    Object.defineProperty(receipt, writerGeneration, {
      value: generation,
      enumerable: true,
      configurable: true,
    });
    return receipt;
  }
  readonly owner = randomUUID();
  constructor(directory: string) {
    mkdirSync(directory, { recursive: true, mode: 0o700 });
    const path = join(directory, "receipts.sqlite"),
      existed = existsSync(path);
    this.db = new DatabaseSync(path, { timeout: 3000 });
    try {
      const version = Number(
        this.db.prepare("PRAGMA user_version").get()?.user_version,
      );
      if (version > 2)
        fail(
          "invalid_configuration",
          "State schema is newer than this build; use the matching Numera version without downgrading state.",
          "storage",
        );
      this.db.exec("PRAGMA journal_mode=WAL;PRAGMA synchronous=FULL;");
      chmodSync(path, 0o600);
      for (const suffix of ["-wal", "-shm"])
        if (existsSync(path + suffix)) chmodSync(path + suffix, 0o600);
      const columns = this.db.prepare("PRAGMA table_info(requests)").all();
      if (
        columns.some((column) => column.name === "generation") !==
        columns.some((column) => column.name === "reservation")
      )
        fail(
          "invalid_configuration",
          "State schema migration is incomplete; preserve the database and restore a verified backup.",
          "storage",
        );
      if (
        existed &&
        columns.length &&
        !columns.some((column) => column.name === "generation")
      ) {
        // VACUUM INTO reads a consistent SQLite snapshot, including committed WAL pages.
        const backup = join(
          directory,
          `receipts-before-journal-${randomUUID()}.sqlite`,
        );
        this.db.exec(`VACUUM INTO '${backup.replace(/'/g, "''")}'`);
        chmodSync(backup, 0o600);
        writeFileSync(
          backup + ".json",
          JSON.stringify({
            schema_version: version,
            target_schema_version: 2,
            created_at: new Date().toISOString(),
            sha256: createHash("sha256")
              .update(readFileSync(backup))
              .digest("hex"),
          }) + "\n",
          { encoding: "utf8", mode: 0o600, flag: "wx" },
        );
      }
      this.transaction(() => {
        this.db
          .exec(`CREATE TABLE IF NOT EXISTS requests(id TEXT PRIMARY KEY,fingerprint TEXT NOT NULL,identity TEXT NOT NULL,receipt TEXT NOT NULL,owner TEXT,lease INTEGER,cancel INTEGER NOT NULL DEFAULT 0,generation INTEGER NOT NULL DEFAULT 1,reservation INTEGER NOT NULL DEFAULT 0);
          CREATE TABLE IF NOT EXISTS outputs(id TEXT PRIMARY KEY,request_id TEXT NOT NULL,metadata TEXT NOT NULL);
          CREATE INDEX IF NOT EXISTS output_request ON outputs(request_id);
          CREATE TABLE IF NOT EXISTS continuation(id TEXT PRIMARY KEY,scope TEXT NOT NULL,value TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS result_journal(request_id TEXT NOT NULL,item INTEGER NOT NULL CHECK(item>=0 AND item<10),output_id TEXT NOT NULL UNIQUE,target TEXT,bytes BLOB,url TEXT,sha256 TEXT,phase TEXT NOT NULL,PRIMARY KEY(request_id,item));`);
        if (
          columns.length &&
          !columns.some((column) => column.name === "generation")
        )
          this.db.exec(
            "ALTER TABLE requests ADD COLUMN generation INTEGER NOT NULL DEFAULT 1;ALTER TABLE requests ADD COLUMN reservation INTEGER NOT NULL DEFAULT 0;",
          );
        this.db.exec("PRAGMA user_version=2;");
      });
    } catch (error) {
      this.db.close();
      throw error;
    }
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
  private row(id: string) {
    const row = this.db.prepare("SELECT * FROM requests WHERE id=?").get(id);
    if (!row) fail("invalid_input", "Unknown owned request ID.");
    return row;
  }
  private fence(id: string, receipt?: Receipt) {
    const row = this.row(id);
    if (receipt && !this.owns(id, receipt))
      fail(
        "outcome_unknown",
        "Request writer generation changed; stale writes are fenced.",
        "storage",
      );
    if (
      row.owner !== this.owner ||
      Number(row.generation) !== this.generations.get(id) ||
      Number(row.lease) <= Date.now()
    )
      fail(
        "outcome_unknown",
        "Request writer lease changed; stale writes are fenced. Recover the existing request, never resubmit.",
        "storage",
      );
    return row;
  }
  owns(id: string, receipt?: Receipt): boolean {
    const row = this.row(id);
    const generation = receipt
      ? (receipt as Receipt & { [writerGeneration]?: number })[writerGeneration]
      : this.generations.get(id);
    return (
      row.owner === this.owner &&
      Number(row.generation) === generation &&
      Number(row.lease) > Date.now()
    );
  }
  renew(id: string, receipt?: Receipt): boolean {
    const generation = receipt
      ? (receipt as Receipt & { [writerGeneration]?: number })[writerGeneration]
      : this.generations.get(id);
    if (generation === undefined) return false;
    return (
      this.db
        .prepare(
          "UPDATE requests SET lease=? WHERE id=? AND owner=? AND generation=? AND lease>?",
        )
        .run(Date.now() + Store.leaseMs, id, this.owner, generation, Date.now())
        .changes === 1
    );
  }
  release(id: string, receipt?: Receipt) {
    const generation = receipt
      ? (receipt as Receipt & { [writerGeneration]?: number })[writerGeneration]
      : this.generations.get(id);
    if (generation !== undefined)
      this.db
        .prepare(
          "UPDATE requests SET owner=NULL,lease=0 WHERE id=? AND owner=? AND generation=?",
        )
        .run(id, this.owner, generation);
    if (this.generations.get(id) === generation) this.generations.delete(id);
  }
  private expire(id?: string) {
    const rows = this.db
      .prepare(
        `SELECT * FROM requests WHERE lease<=? AND owner IS NOT NULL${id ? " AND id=?" : ""}`,
      )
      .all(...(id ? [Date.now(), id] : [Date.now()]));
    for (const row of rows) {
      const receipt = JSON.parse(String(row.receipt)) as Receipt;
      if (busy.includes(receipt.status)) {
        const pending = this.db
          .prepare(
            "SELECT 1 FROM result_journal WHERE request_id=? AND phase!='committed' LIMIT 1",
          )
          .get(String(row.id));
        if (receipt.status === "prepared") {
          receipt.status = "failed";
          receipt.generation_outcome = "not_submitted";
        } else if (!pending && !receipt.outputs.length) {
          receipt.status = receipt.upstream_job ? "running" : "outcome_unknown";
          receipt.generation_outcome = receipt.upstream_job
            ? "running"
            : "unknown";
        }
        receipt.updated_at = new Date().toISOString();
      }
      this.db
        .prepare(
          "UPDATE requests SET receipt=?,owner=NULL,lease=0,generation=generation+1,reservation=CASE WHEN ? THEN reservation ELSE 0 END WHERE id=? AND generation=? AND lease<=?",
        )
        .run(
          JSON.stringify(receipt),
          receipt.status !== "failed" ? 1 : 0,
          String(row.id),
          Number(row.generation),
          Date.now(),
        );
    }
  }
  prepare(
    receipt: Receipt,
    hash: string,
    identity: string,
  ): { fresh: boolean; receipt: Receipt } {
    return this.transaction(() => {
      const old = this.db
        .prepare("SELECT fingerprint,identity FROM requests WHERE id=?")
        .get(receipt.request_id);
      if (old) {
        if (old.fingerprint !== hash || old.identity !== identity)
          fail(
            "request_id_conflict",
            "Request ID is already bound to different effective inputs or connection identity.",
          );
        this.expire(receipt.request_id);
        return { fresh: false, receipt: this.get(receipt.request_id) };
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
          Date.now() + Store.leaseMs,
        );
      this.generations.set(receipt.request_id, 1);
      this.stamp(receipt, 1);
      return { fresh: true, receipt };
    });
  }
  get(id: string): Receipt {
    const row = this.row(id);
    return this.stamp(
      JSON.parse(String(row.receipt)) as Receipt,
      Number(row.generation),
    );
  }
  identity(id: string): string {
    return String(this.row(id).identity);
  }
  update(receipt: Receipt) {
    this.fence(receipt.request_id);
    if (
      (receipt as Receipt & { [writerGeneration]?: number })[
        writerGeneration
      ] !== this.generations.get(receipt.request_id)
    )
      fail(
        "outcome_unknown",
        "Receipt belongs to an earlier writer generation; stale writes are fenced.",
        "storage",
      );
    receipt.updated_at = new Date().toISOString();
    this.db
      .prepare(
        "UPDATE requests SET receipt=?,reservation=CASE WHEN ? THEN reservation ELSE 0 END WHERE id=? AND owner=? AND generation=?",
      )
      .run(
        JSON.stringify(receipt),
        busy.includes(receipt.status) ? 1 : 0,
        receipt.request_id,
        this.owner,
        this.generations.get(receipt.request_id)!,
      );
  }
  admit(
    id: string,
    connection: string,
    global: number,
    perConnection: number,
  ): boolean {
    return this.transaction(() => {
      this.expire();
      const all = this.db
        .prepare(
          "SELECT receipt FROM requests WHERE lease>? OR (owner IS NULL AND json_extract(receipt,'$.status')='running' AND json_extract(receipt,'$.upstream_job') IS NOT NULL)",
        )
        .all(Date.now())
        .map((row) => JSON.parse(String(row.receipt)) as Receipt)
        .filter((r) =>
          ["submitting", "running", "finalizing"].includes(r.status),
        );
      if (
        all.length >= global ||
        all.filter((r) => r.connection === connection).length >= perConnection
      )
        return false;
      const r = this.get(id);
      if (r.status !== "prepared" || !this.owns(id)) return false;
      r.status = "submitting";
      this.update(r);
      return true;
    });
  }
  claimFinalization(id: string, expected?: Receipt): boolean {
    return this.transaction(() => {
      this.expire(id);
      const row = this.row(id),
        r = JSON.parse(String(row.receipt)) as Receipt;
      if (expected && !this.owns(id, expected)) return false;
      if (row.owner && !this.owns(id)) return false;
      if (r.status === "finalizing" && row.owner) return false;
      if (!row.owner && r.status === "prepared") return false;
      if (
        r.status === "submitting" &&
        row.owner &&
        !expected &&
        !this.hasPendingResults(id)
      )
        return false;
      if (
        ["completed", "partial", "failed", "cancelled"].includes(r.status) &&
        !this.hasPendingResults(id)
      )
        return false;
      const generation = Number(row.generation) + 1;
      r.status = "finalizing";
      r.updated_at = new Date().toISOString();
      this.db
        .prepare(
          "UPDATE requests SET receipt=?,owner=?,generation=?,lease=? WHERE id=? AND generation=?",
        )
        .run(
          JSON.stringify(r),
          this.owner,
          generation,
          Date.now() + Store.leaseMs,
          id,
          Number(row.generation),
        );
      this.generations.set(id, generation);
      return true;
    });
  }
  queued() {
    return this.transaction(() => {
      this.expire();
      return this.db
        .prepare("SELECT receipt FROM requests")
        .all()
        .map((r) => JSON.parse(String(r.receipt)) as Receipt)
        .filter((r) => r.status === "prepared").length;
    });
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
  reserveResults(id: string, bytes: number, budget: number) {
    this.transaction(() => {
      this.fence(id);
      if (
        !Number.isSafeInteger(bytes) ||
        bytes <= 0 ||
        !Number.isSafeInteger(budget) ||
        budget <= 0
      )
        fail(
          "invalid_configuration",
          "Result journal limits must be positive safe integers.",
        );
      const used = this.db
        .prepare(
          `SELECT COALESCE(SUM(MAX(reservation,COALESCE((SELECT SUM(COALESCE(length(bytes),0)+COALESCE(length(url),0)) FROM result_journal WHERE request_id=requests.id),0))),0) AS used FROM requests WHERE id!=?`,
        )
        .get(id);
      if (Number(used?.used) + bytes > budget)
        fail(
          "rate_limited",
          "Private result journal capacity is full; recover retained outputs before authorizing more image work.",
          "storage",
        );
      this.db
        .prepare("UPDATE requests SET reservation=? WHERE id=?")
        .run(bytes, id);
    });
  }
  private result(id: string, index: number) {
    if (!Number.isInteger(index) || index < 0 || index >= 10)
      fail(
        "invalid_response",
        "Provider returned too many final image items.",
        "response",
      );
    return this.db
      .prepare("SELECT * FROM result_journal WHERE request_id=? AND item=?")
      .get(id, index);
  }
  retainResult(id: string, index: number, bytes: Buffer, expected?: Receipt) {
    this.transaction(() => {
      const request = this.fence(id, expected),
        old = this.result(id, index),
        hash = createHash("sha256").update(bytes).digest("hex");
      if (old?.sha256 && old.sha256 !== hash)
        fail(
          "invalid_response",
          "An existing result item changed its image bytes.",
          "response",
        );
      if (old?.phase === "committed") return;
      const total =
        Number(
          this.db
            .prepare(
              "SELECT COALESCE(SUM(COALESCE(length(bytes),(SELECT json_extract(metadata,'$.bytes') FROM outputs WHERE id=output_id),0)+COALESCE(length(url),0)),0) AS bytes FROM result_journal WHERE request_id=? AND item!=?",
            )
            .get(id, index)?.bytes,
        ) + bytes.length;
      if (!bytes.length || total > Number(request.reservation))
        fail(
          "invalid_response",
          "Aggregate output bytes exceeded.",
          "response",
        );
      this.db
        .prepare(
          "INSERT INTO result_journal(request_id,item,output_id,bytes,sha256,phase) VALUES(?,?,?,?,?,'retained') ON CONFLICT(request_id,item) DO UPDATE SET bytes=excluded.bytes,url=NULL,sha256=excluded.sha256,phase=CASE WHEN result_journal.target IS NULL THEN 'retained' ELSE 'publishing' END",
        )
        .run(
          id,
          index,
          old ? String(old.output_id) : randomUUID(),
          bytes,
          hash,
        );
      const receipt = this.get(id);
      receipt.generation_outcome = "completed";
      receipt.updated_at = new Date().toISOString();
      this.db
        .prepare(
          "UPDATE requests SET receipt=? WHERE id=? AND owner=? AND generation=?",
        )
        .run(
          JSON.stringify(receipt),
          id,
          this.owner,
          this.generations.get(id)!,
        );
    });
  }
  retainUrl(id: string, index: number, url: string, expected?: Receipt) {
    this.transaction(() => {
      this.fence(id, expected);
      const old = this.result(id, index);
      if (old?.bytes || old?.phase === "committed") return;
      if (Buffer.byteLength(url, "utf8") > 16384)
        fail(
          "invalid_response",
          "Image continuation URL exceeds private state limit.",
        );
      const total =
        Number(
          this.db
            .prepare(
              "SELECT COALESCE(SUM(COALESCE(length(bytes),(SELECT json_extract(metadata,'$.bytes') FROM outputs WHERE id=output_id),0)+COALESCE(length(url),0)),0) AS bytes FROM result_journal WHERE request_id=? AND item!=?",
            )
            .get(id, index)?.bytes,
        ) + Buffer.byteLength(url, "utf8");
      if (total > Number(this.row(id).reservation))
        fail(
          "invalid_response",
          "Aggregate output bytes exceeded.",
          "response",
        );
      this.db
        .prepare(
          "INSERT INTO result_journal(request_id,item,output_id,url,phase) VALUES(?,?,?,?,'waiting') ON CONFLICT(request_id,item) DO UPDATE SET url=excluded.url",
        )
        .run(id, index, old ? String(old.output_id) : randomUUID(), url);
    });
  }
  needsLocalRecovery(id: string): boolean {
    const receipt = this.get(id);
    return (
      this.hasPendingResults(id) ||
      (receipt.generation_outcome === "completed" &&
        receipt.outputs.length > 0 &&
        !["completed", "partial", "failed"].includes(receipt.status))
    );
  }
  hasPendingResults(id: string): boolean {
    return !!this.db
      .prepare(
        "SELECT 1 FROM result_journal WHERE request_id=? AND phase!='committed' LIMIT 1",
      )
      .get(id);
  }
  pendingResults(id: string): ResultItem[] {
    return this.db
      .prepare(
        "SELECT * FROM result_journal WHERE request_id=? AND phase!='committed' ORDER BY item",
      )
      .all(id)
      .map((r) => ({
        index: Number(r.item),
        output_id: String(r.output_id),
        target: r.target === null ? null : String(r.target),
        bytes: r.bytes === null ? null : Buffer.from(r.bytes as Uint8Array),
        url: r.url === null ? null : String(r.url),
        sha256: r.sha256 === null ? null : String(r.sha256),
        phase: String(r.phase),
      }));
  }
  publication(
    id: string,
    index: number,
    target: (outputId: string) => string,
    expected?: Receipt,
  ): ResultItem {
    return this.transaction(() => {
      this.fence(id, expected);
      const row = this.result(id, index);
      if (!row?.sha256)
        fail(
          "output_file_error",
          "Stable publication requires retained, validated image bytes.",
          "storage",
        );
      const path =
        row.target === null
          ? target(String(row.output_id))
          : String(row.target);
      this.db
        .prepare(
          "UPDATE result_journal SET target=?,phase=CASE WHEN phase='committed' THEN phase ELSE 'publishing' END WHERE request_id=? AND item=?",
        )
        .run(path, id, index);
      return {
        index,
        output_id: String(row.output_id),
        target: path,
        bytes: row.bytes === null ? null : Buffer.from(row.bytes as Uint8Array),
        url: null,
        sha256: String(row.sha256),
        phase: String(row.phase),
      };
    });
  }
  commitResult(
    id: string,
    index: number,
    output: Output,
    deviations: string[] = [],
    expected?: Receipt,
  ) {
    this.transaction(() => {
      this.fence(id, expected);
      const item = this.result(id, index);
      if (
        !item ||
        item.output_id !== output.output_id ||
        item.sha256 !== output.sha256 ||
        item.target !== output.path
      )
        fail(
          "output_file_error",
          "Published output does not match its retained journal identity.",
          "storage",
        );
      if (item.phase === "committed") return;
      this.addOutput(output);
      const receipt = this.get(id);
      receipt.outputs.push(output);
      receipt.deviations.push(...deviations);
      this.update(receipt);
      this.db
        .prepare(
          "UPDATE result_journal SET bytes=NULL,url=NULL,phase='committed' WHERE request_id=? AND item=?",
        )
        .run(id, index);
    });
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
  saveContinuation(id: string, scope: string, value: unknown) {
    this.fence(id);
    const json = JSON.stringify(value);
    if (Buffer.byteLength(json, "utf8") > 1024 * 1024)
      fail(
        "invalid_response",
        "Continuation metadata exceeds private state limit.",
      );
    this.db
      .prepare(
        "INSERT INTO continuation VALUES(?,?,?) ON CONFLICT(id) DO UPDATE SET scope=excluded.scope,value=excluded.value",
      )
      .run(id, scope, json);
  }
  continuation(id: string, scope: string): unknown {
    const row = this.db
      .prepare("SELECT scope,value FROM continuation WHERE id=?")
      .get(id);
    if (!row) return null;
    if (row.scope !== scope)
      fail(
        "permission_denied",
        "Continuation scope does not match the selected connection/account/model.",
      );
    return JSON.parse(String(row.value));
  }
  close() {
    this.db.close();
  }
}
