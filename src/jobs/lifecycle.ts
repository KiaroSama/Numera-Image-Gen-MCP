import type { DatabaseSync } from "node:sqlite";
import type { Receipt } from "./store.js";
import { fail } from "../errors.js";

export type TerminalState = "failed" | "cancelled" | "incomplete";
export const busy = ["prepared", "submitting", "running", "finalizing"];

export function expireRequests(db: DatabaseSync, id?: string) {
  db.exec(
    "UPDATE requests SET reservation=0 WHERE owner IS NULL AND json_extract(receipt, '$.status')='outcome_unknown' AND json_extract(receipt, '$.upstream_job') IS NULL AND NOT EXISTS (SELECT 1 FROM result_journal WHERE request_id=requests.id AND phase!='committed')",
  );
  const rows = db
    .prepare(
      `SELECT * FROM requests WHERE lease<=? AND owner IS NOT NULL${id ? " AND id=?" : ""}`,
    )
    .all(...(id ? [Date.now(), id] : [Date.now()]));
  for (const row of rows) {
    const receipt = JSON.parse(String(row.receipt)) as Receipt;
    const retained = !!db
      .prepare(
        "SELECT 1 FROM result_journal WHERE request_id=? AND phase!='committed' LIMIT 1",
      )
      .get(String(row.id));
    if (busy.includes(receipt.status) && !receipt.upstream_terminal) {
      if (receipt.status === "prepared") {
        receipt.status = "failed";
        receipt.generation_outcome = "not_submitted";
      } else if (!retained && !receipt.outputs.length) {
        receipt.status = receipt.upstream_job ? "running" : "outcome_unknown";
        receipt.generation_outcome = receipt.upstream_job
          ? "running"
          : "unknown";
      }
      receipt.updated_at = new Date().toISOString();
    }
    const reserve =
      retained || (receipt.upstream_job && busy.includes(receipt.status));
    db.prepare(
      "UPDATE requests SET receipt=?,owner=NULL,lease=0,generation=generation+1,reservation=CASE WHEN ? THEN reservation ELSE 0 END WHERE id=? AND generation=? AND lease<=?",
    ).run(
      JSON.stringify(receipt),
      reserve ? 1 : 0,
      String(row.id),
      Number(row.generation),
      Date.now(),
    );
  }
}

export function updateReceipt(
  db: DatabaseSync,
  receipt: Receipt,
  owner: string,
  generation: number,
) {
  receipt.updated_at = new Date().toISOString();
  db.prepare(
    "UPDATE requests SET receipt=?,reservation=CASE WHEN ? THEN reservation ELSE 0 END WHERE id=? AND owner=? AND generation=?",
  ).run(
    JSON.stringify(receipt),
    busy.includes(receipt.status) ? 1 : 0,
    receipt.request_id,
    owner,
    generation,
  );
}

export function reserveResults(
  db: DatabaseSync,
  id: string,
  bytes: number,
  budget: number,
) {
  if (![bytes, budget].every((n) => Number.isSafeInteger(n) && n > 0))
    fail(
      "invalid_configuration",
      "Result journal limits must be positive safe integers.",
    );
  const used = db
    .prepare(
      `SELECT COALESCE(SUM(MAX(reservation,COALESCE((SELECT SUM(COALESCE(length(bytes),0)+COALESCE(length(CAST(url AS BLOB)),0)) FROM result_journal WHERE request_id=requests.id),0))),0) AS used FROM requests WHERE id!=?`,
    )
    .get(id);
  const retained = Number(
    db
      .prepare(
        "SELECT COALESCE(SUM(COALESCE(length(bytes),0)+COALESCE(length(CAST(url AS BLOB)),0)),0) AS used FROM result_journal WHERE request_id=?",
      )
      .get(id)?.used,
  );
  if (Number(used?.used) + Math.max(bytes, retained) > budget)
    fail(
      "rate_limited",
      "Private result journal capacity is full; recover retained outputs before authorizing more image work.",
      "storage",
    );
  db.prepare("UPDATE requests SET reservation=? WHERE id=?").run(
    Math.max(bytes, retained),
    id,
  );
}

export function terminateRequest(
  db: DatabaseSync,
  id: string,
  state: TerminalState,
  error: unknown,
  job?: { id: string; kind: string },
  keepWriter = false,
) {
  const row = db.prepare("SELECT * FROM requests WHERE id=?").get(id);
  if (!row) fail("invalid_input", "Unknown owned request ID.");
  const receipt = JSON.parse(String(row.receipt)) as Receipt;
  if (
    job &&
    (receipt.upstream_job?.id !== job.id ||
      receipt.upstream_job.kind !== job.kind)
  )
    fail(
      "permission_denied",
      "Terminal result does not match the existing job.",
    );
  if (
    receipt.upstream_terminal ||
    (!receipt.upstream_terminal && receipt.generation_outcome === "completed")
  )
    return;
  receipt.upstream_terminal = state;
  receipt.generation_outcome = state;
  receipt.status = receipt.outputs.length
    ? "partial"
    : state === "cancelled"
      ? "cancelled"
      : "failed";
  receipt.errors.push(error);
  receipt.updated_at = new Date().toISOString();
  db.prepare(
    "UPDATE requests SET receipt=?,reservation=CASE WHEN ? THEN reservation ELSE 0 END,owner=CASE WHEN ? THEN owner ELSE NULL END,lease=CASE WHEN ? THEN lease ELSE 0 END,generation=generation+CASE WHEN ? THEN 0 ELSE 1 END WHERE id=? AND generation=?",
  ).run(
    JSON.stringify(receipt),
    keepWriter ? 1 : 0,
    keepWriter ? 1 : 0,
    keepWriter ? 1 : 0,
    keepWriter ? 1 : 0,
    id,
    Number(row.generation),
  );
}
