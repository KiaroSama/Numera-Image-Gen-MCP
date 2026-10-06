import type { DatabaseSync } from "node:sqlite";
import { fail } from "../errors.js";

export function initializeJournal(db: DatabaseSync) {
  const deadline = Date.now() + 3000;
  const wait = new Int32Array(new SharedArrayBuffer(4));
  for (;;) {
    try {
      // WAL mode is persistent. A concurrent first-open upgrade can return BUSY immediately.
      const current = db.prepare("PRAGMA journal_mode").get()?.journal_mode;
      const mode =
        current === "wal"
          ? current
          : db.prepare("PRAGMA journal_mode=WAL").get()?.journal_mode;
      if (mode !== "wal")
        fail(
          "invalid_configuration",
          "State database cannot enable durable WAL journaling.",
          "storage",
        );
      db.exec("PRAGMA synchronous=FULL");
      return;
    } catch (error) {
      const remaining = deadline - Date.now();
      if ((error as { errcode?: number }).errcode !== 5 || remaining <= 0)
        throw error;
      // Predicate/deadline-bound lock contention, never a retry of provider submission.
      Atomics.wait(wait, 0, 0, Math.min(20, remaining));
    }
  }
}
