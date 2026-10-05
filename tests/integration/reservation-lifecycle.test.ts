import { spawn } from "node:child_process";
import { resolve } from "node:path";
import { it, expect } from "vitest";
import { DatabaseSync } from "node:sqlite";
import { join } from "node:path";
import { Store } from "../../src/jobs/store.js";
import { receipt, workspace } from "../fixtures/runtime.js";

it("reclaims only empty orphan reservations and preserves uncertainty across reopen", async () =>
  workspace(async (root) => {
    let store = new Store(root);
    try {
      for (const id of ["old-a", "old-b"]) {
        const r = store.prepare(receipt(id), id, "identity").receipt;
        store.admit(id, "local", 10, 10);
        r.status = "submitting";
        r.generation_outcome = "unknown";
        store.update(r);
        store.reserveResults(id, 1024, 2048);
      }
      const db = new DatabaseSync(join(root, "receipts.sqlite"));
      db.exec("UPDATE requests SET lease=0");
      db.close();
      store.queued();
      store.close();
      for (let reopen = 0; reopen < 2; reopen++) {
        store = new Store(root);
        for (const id of ["old-a", "old-b"]) {
          expect(store.get(id)).toMatchObject({
            status: "outcome_unknown",
            generation_outcome: "unknown",
          });
          expect(store.prepare(receipt(id), id, "identity").fresh).toBe(false);
        }
        const r = store.prepare(
          receipt(`fresh-${reopen}`),
          "fresh",
          "identity",
        ).receipt;
        expect(store.admit(r.request_id, "local", 1, 1)).toBe(true);
        expect(() =>
          store.reserveResults(r.request_id, 2048, 2048),
        ).not.toThrow();
        r.status = "failed";
        store.update(r);
        store.release(r.request_id, r);
        store.close();
      }
    } finally {
      try {
        store.close();
      } catch {}
    }
  }));

it.each(["bytes", "url", "live", "detached"])(
  "keeps %s charges and fences expired writers",
  async (mode) =>
    workspace(async (root) => {
      const store = new Store(root),
        peer = new Store(root);
      try {
        const old = store.prepare(receipt("old"), "hash", "identity").receipt;
        store.admit("old", "local", 10, 10);
        old.status = "submitting";
        store.update(old);
        store.reserveResults("old", 1024, 2048);
        if (mode === "bytes")
          store.retainResult("old", 0, Buffer.alloc(512), old);
        if (mode === "url")
          store.retainUrl("old", 0, "https://example.com/final", old);
        if (mode === "detached") {
          old.status = "running";
          old.upstream_job = { id: "job", kind: "responses" };
          store.update(old);
          store.release("old", old);
        }
        if (mode !== "live" && mode !== "detached") {
          const db = new DatabaseSync(join(root, "receipts.sqlite"));
          db.exec("UPDATE requests SET lease=0");
          db.close();
          peer.queued();
          expect(store.owns("old", old)).toBe(false);
          expect(() =>
            store.retainResult("old", 1, Buffer.alloc(8), old),
          ).toThrow();
        }
        peer.prepare(receipt("fresh"), "fresh", "identity");
        peer.admit("fresh", "local", 10, 10);
        expect(() => peer.reserveResults("fresh", 2048, 2048)).toThrow(
          expect.objectContaining({ code: "rate_limited" }),
        );
        if (mode === "bytes" || mode === "url")
          expect(peer.pendingResults("old")).toHaveLength(1);
      } finally {
        peer.close();
        store.close();
      }
    }),
);

it("confirmed termination fences a live waiter without discarding retained bytes", async () =>
  workspace(async (root) => {
    const owner = new Store(root),
      control = new Store(root);
    try {
      const r = owner.prepare(receipt("race"), "hash", "identity").receipt;
      owner.admit("race", "local", 1, 1);
      r.status = "running";
      r.upstream_job = { id: "job", kind: "comfyui" };
      owner.update(r);
      owner.reserveResults("race", 1024, 2048);
      owner.retainResult("race", 0, Buffer.alloc(16), r);
      control.terminal(
        "race",
        "cancelled",
        { code: "provider_rejection" },
        undefined,
        r.upstream_job,
      );
      expect(owner.owns("race", r)).toBe(true);
      expect(() =>
        owner.retainResult("race", 1, Buffer.alloc(8), r),
      ).not.toThrow();
      expect(control.pendingResults("race").map((i) => i.bytes)).toEqual([
        Buffer.alloc(16),
        Buffer.alloc(8),
      ]);
      expect(control.get("race").generation_outcome).toBe("cancelled");
      owner.release("race", r);
      control.prepare(receipt("fresh"), "fresh", "identity");
      expect(control.admit("fresh", "local", 1, 1)).toBe(true);
    } finally {
      control.close();
      owner.close();
    }
  }));

it(
  "serializes concurrent reservations across two actual Store processes",
  async () =>
    workspace(async (root) => {
      const children = ["one", "two"].map((id) =>
        spawn(
          process.execPath,
          [resolve("tests/fixtures/lifecycle-store.mjs"), root, id, "reserve"],
          { windowsHide: true, stdio: ["ignore", "pipe", "pipe", "ipc"] },
        ),
      );
      const closed = children.map(
        (child) =>
          new Promise<void>((done) => child.once("close", () => done())),
      );
      const deadlines = children.map((child) =>
        setTimeout(() => child.kill("SIGKILL"), 5000),
      );
      try {
        await Promise.all(
          children.map(
            (child) =>
              new Promise<void>((done, reject) => {
                child.once("error", reject);
                child.once("close", () =>
                  reject(new Error("Fixture exited before ready.")),
                );
                child.once("message", () => done());
              }),
          ),
        );
        const results = children.map(
          (child) =>
            new Promise<{ reserved: boolean; code?: string }>(
              (done, reject) => {
                child.once("close", () =>
                  reject(new Error("Fixture exited before result.")),
                );
                child.once("message", (m) =>
                  done(m as { reserved: boolean; code?: string }),
                );
              },
            ),
        );
        children.forEach((child) => child.send("reserve"));
        const values = await Promise.all(results);
        await Promise.all(closed);
        expect(values.filter((v) => v.reserved)).toHaveLength(1);
        expect(values.find((v) => !v.reserved)?.code).toBe("rate_limited");
        const db = new DatabaseSync(join(root, "receipts.sqlite"), {
          readOnly: true,
        });
        try {
          expect(
            db.prepare("SELECT SUM(reservation) AS n FROM requests").get()?.n,
          ).toBe(2048);
        } finally {
          db.close();
        }
      } finally {
        deadlines.forEach(clearTimeout);
        children.forEach((child) => {
          if (child.exitCode === null && child.signalCode === null)
            child.kill("SIGKILL");
        });
        await Promise.all(closed);
      }
    }),
  15000,
);

it("completed output wins a later cancellation acknowledgement without state resurrection", async () =>
  workspace(async (root) => {
    const owner = new Store(root),
      control = new Store(root);
    try {
      const r = owner.prepare(receipt("done"), "hash", "identity").receipt;
      owner.admit("done", "local", 1, 1);
      r.status = "completed";
      r.generation_outcome = "completed";
      r.upstream_job = { id: "job", kind: "comfyui" };
      owner.update(r);
      owner.release("done", r);
      const first = control.terminal(
        "done",
        "cancelled",
        { code: "provider_rejection" },
        undefined,
        r.upstream_job,
      );
      expect(first.status).toBe("completed");
      expect(first.upstream_terminal).toBeUndefined();
      expect(first.errors).toEqual([]);
    } finally {
      control.close();
      owner.close();
    }
  }));

it("owned terminal completion holds result budget until retention and then relinquishes unused capacity", async () =>
  workspace(async (root) => {
    const owner = new Store(root),
      peer = new Store(root);
    try {
      const r = owner.prepare(receipt("terminal"), "hash", "identity").receipt;
      owner.admit("terminal", "local", 1, 1);
      r.status = "running";
      r.upstream_job = { id: "job", kind: "responses" };
      owner.update(r);
      owner.reserveResults("terminal", 1024, 1024);
      const terminal = owner.terminal(
        "terminal",
        "failed",
        { code: "provider_rejection" },
        r,
        r.upstream_job,
      );
      expect(owner.owns("terminal", terminal)).toBe(true);
      peer.prepare(receipt("fresh"), "fresh", "identity");
      expect(peer.admit("fresh", "local", 1, 1)).toBe(true);
      expect(() => peer.reserveResults("fresh", 1024, 1024)).toThrow(
        expect.objectContaining({ code: "rate_limited" }),
      );
      owner.retainResult("terminal", 0, Buffer.alloc(32), terminal);
      owner.release("terminal", terminal);
      expect(() => peer.reserveResults("fresh", 992, 1024)).not.toThrow();
      expect(owner.pendingResults("terminal")[0]?.bytes).toEqual(
        Buffer.alloc(32),
      );
    } finally {
      peer.close();
      owner.close();
    }
  }));

it("in-flight known completion wins cancellation before the first image download", async () =>
  workspace(async (root) => {
    const owner = new Store(root),
      control = new Store(root);
    try {
      const r = owner.prepare(receipt("finishing"), "hash", "identity").receipt;
      owner.admit("finishing", "local", 1, 1);
      r.status = "finalizing";
      r.generation_outcome = "completed";
      r.upstream_job = { id: "job", kind: "comfyui" };
      owner.update(r);
      owner.reserveResults("finishing", 1024, 2048);
      expect(
        control.terminal(
          "finishing",
          "cancelled",
          { code: "provider_rejection" },
          undefined,
          r.upstream_job,
        ).generation_outcome,
      ).toBe("completed");
      expect(owner.owns("finishing", r)).toBe(true);
      owner.retainResult("finishing", 0, Buffer.alloc(16), r);
      expect(owner.pendingResults("finishing")).toHaveLength(1);
    } finally {
      control.close();
      owner.close();
    }
  }));

it("owned empty terminal release frees the unused reservation without erasing its receipt", async () =>
  workspace(async (root) => {
    const owner = new Store(root),
      peer = new Store(root);
    try {
      const r = owner.prepare(receipt("empty"), "hash", "identity").receipt;
      owner.admit("empty", "local", 1, 1);
      owner.reserveResults("empty", 1024, 1024);
      const done = owner.terminal(
        "empty",
        "failed",
        { code: "provider_rejection" },
        r,
      );
      owner.release("empty", done);
      peer.prepare(receipt("fresh"), "fresh", "identity");
      expect(() => peer.reserveResults("fresh", 1024, 1024)).not.toThrow();
      expect(peer.get("empty").status).toBe("failed");
    } finally {
      peer.close();
      owner.close();
    }
  }));
