import { expect, it, vi } from "vitest";
import { readFile, readdir, writeFile, mkdir } from "node:fs/promises";
import { join, resolve } from "node:path";
import { spawn } from "node:child_process";
import { DatabaseSync } from "node:sqlite";
import * as filesystem from "node:fs/promises";
import { Store } from "../../src/jobs/store.js";
import { Generation } from "../../src/services/generation.js";
import { getJob } from "../../src/services/jobs.js";
import { Logger } from "../../src/logging.js";
import { requestSchema } from "../../src/config/schema.js";
import { inspectImage } from "../../src/files/images.js";
import { saveImage } from "../../src/files/output.js";
import {
  configuration,
  imageBytes,
  receipt,
  workspace,
  server,
  connection,
  json,
} from "../fixtures/runtime.js";

vi.mock("node:fs/promises", async (original) => {
  const actual = await original<typeof filesystem>();
  return {
    ...actual,
    open: vi.fn(actual.open),
    link: vi.fn(actual.link),
    unlink: vi.fn(actual.unlink),
  };
});

it("retains completed inline bytes after abort and recovers locally without a connection", async () =>
  workspace(async (root) => {
    const config = configuration(root),
      store = new Store(config.stateDir),
      logger = new Logger(config.logging.directory, "ERROR");
    const bytes = await imageBytes();
    try {
      const initial = receipt("abort-inline");
      store.prepare(initial, "hash", "identity");
      const generation = new Generation(config, store, logger);
      const first = await generation.finish(
        initial,
        { images: [{ bytes }], upstreamModel: null, warnings: [] },
        requestSchema.parse({ prompt: "draw" }),
        AbortSignal.abort(),
      );
      expect(first.generation_outcome).toBe("completed");
      expect(first.outputs).toHaveLength(0);
      expect(store.pendingResults(initial.request_id)).toHaveLength(1);
      const withoutConnection = {
        ...config,
        connections: {},
        defaultConnection: undefined,
      };
      const done = await getJob(
        new Generation(withoutConnection, store, logger),
        initial.request_id,
        true,
      );
      expect(done.status).toBe("completed");
      expect(done.outputs).toHaveLength(1);
      expect(await readFile(done.outputs[0]!.path)).toEqual(bytes);
      expect(store.pendingResults(initial.request_id)).toHaveLength(0);
      expect(store.listOutputs()).toHaveLength(1);
    } finally {
      store.close();
      logger.close();
    }
  }));

it("fences an expired writer and final catch after takeover, while renewal keeps live work owned", async () =>
  workspace(async (root) => {
    const first = new Store(join(root, "state")),
      second = new Store(join(root, "state"));
    const clock = vi.spyOn(Date, "now");
    try {
      clock.mockReturnValue(1000);
      first.prepare(receipt("lease"), "hash", "identity");
      expect(first.admit("lease", "local", 1, 1)).toBe(true);
      clock.mockReturnValue(1000 + Store.leaseMs - 1);
      expect(first.renew("lease")).toBe(true);
      clock.mockReturnValue(1000 + Store.leaseMs + 1);
      expect(second.claimFinalization("lease")).toBe(false);
      const stale = first.get("lease");
      clock.mockReturnValue(1000 + Store.leaseMs * 2 + 1);
      expect(second.claimFinalization("lease")).toBe(true);
      expect(() => first.update({ ...stale, status: "failed" })).toThrow(
        expect.objectContaining({ code: "outcome_unknown" }),
      );
      expect(first.renew("lease")).toBe(false);
      expect(second.get("lease").status).toBe("finalizing");
    } finally {
      clock.mockRestore();
      first.close();
      second.close();
    }
  }));

it("reclaims expired prepared queue entries without replaying paid work", async () =>
  workspace(async (root) => {
    const store = new Store(join(root, "state")),
      clock = vi.spyOn(Date, "now");
    try {
      clock.mockReturnValue(1000);
      for (let index = 0; index < 20; index++)
        store.prepare(receipt(`dead-${index}`), `hash-${index}`, "identity");
      store.prepare(receipt("dead-prepared"), "hash", "identity");
      clock.mockReturnValue(1000 + Store.leaseMs + 1);
      expect(store.queued()).toBe(0);
      expect(store.get("dead-prepared")).toMatchObject({
        status: "failed",
        generation_outcome: "not_submitted",
      });
      expect(
        store.prepare(receipt("dead-prepared"), "hash", "identity").fresh,
      ).toBe(false);
      store.prepare(receipt("fresh"), "fresh-hash", "identity");
      expect(store.queued()).toBe(1);
      expect(store.admit("fresh", "local", 1, 1)).toBe(true);
    } finally {
      clock.mockRestore();
      store.close();
    }
  }));

it("reserves bounded journal capacity before submission and releases committed bytes", async () =>
  workspace(async (root) => {
    const store = new Store(join(root, "state"));
    try {
      store.prepare(receipt("a"), "a", "identity");
      store.prepare(receipt("b"), "b", "identity");
      store.reserveResults("a", 100, 100);
      expect(() => store.reserveResults("b", 100, 100)).toThrow(
        expect.objectContaining({ code: "rate_limited" }),
      );
      expect(store.get("b").generation_outcome).toBe("not_submitted");
      store.update({ ...store.get("a"), status: "failed" });
      expect(() => store.reserveResults("b", 100, 100)).not.toThrow();
    } finally {
      store.close();
    }
  }));

it("reconciles published files by journal hash after receipt commit failure without duplicate output", async () =>
  workspace(async (root) => {
    const config = configuration(root),
      store = new Store(config.stateDir),
      logger = new Logger(config.logging.directory, "ERROR");
    const bytes = await imageBytes(),
      image = await inspectImage(bytes, config);
    try {
      const initial = receipt("published");
      initial.output_requirements = {
        count: 1,
        output_subdirectory: "published",
      };
      store.prepare(initial, "hash", "identity");
      store.reserveResults(
        "published",
        config.files.maxAggregateBytes,
        config.files.maxJournalBytes,
      );
      store.retainResult("published", 0, image.bytes);
      expect(store.claimFinalization("published")).toBe(true);
      const commit = vi
        .spyOn(store, "commitResult")
        .mockImplementationOnce(() => {
          throw new Error("fixture SQL failure");
        });
      await expect(
        saveImage(
          image,
          config,
          store,
          "published",
          0,
          "published",
          undefined,
          { stable: true },
        ),
      ).rejects.toMatchObject({ code: "output_file_error" });
      commit.mockRestore();
      expect(
        (await readdir(join(config.outputDir, "published"))).filter(
          (name) => !name.startsWith("."),
        ),
      ).toHaveLength(1);
      expect(store.get("published").outputs).toHaveLength(0);
      store.release("published");
      const done = await getJob(
        new Generation(config, store, logger),
        "published",
        true,
      );
      expect(done.outputs).toHaveLength(1);
      expect(store.listOutputs()).toHaveLength(1);
      expect(await readFile(done.outputs[0]!.path)).toEqual(bytes);
      expect(
        (await readdir(join(config.outputDir, "published"))).filter(
          (name) => !name.startsWith("."),
        ),
      ).toHaveLength(1);
      expect(store.pendingResults("published")).toHaveLength(0);
    } finally {
      store.close();
      logger.close();
    }
  }));

it("rejects an existing stable filename whose bytes do not match the retained image", async () =>
  workspace(async (root) => {
    const config = configuration(root),
      store = new Store(config.stateDir);
    try {
      store.prepare(receipt("mismatch"), "hash", "identity");
      const image = await inspectImage(await imageBytes(), config);
      store.reserveResults(
        "mismatch",
        config.files.maxAggregateBytes,
        config.files.maxJournalBytes,
      );
      store.retainResult("mismatch", 0, image.bytes);
      store.claimFinalization("mismatch");
      const commit = vi
        .spyOn(store, "commitResult")
        .mockImplementationOnce(() => {
          throw new Error("fixture SQL failure");
        });
      await expect(
        saveImage(image, config, store, "mismatch", 0, undefined, undefined, {
          stable: true,
        }),
      ).rejects.toBeDefined();
      commit.mockRestore();
      const item = store.pendingResults("mismatch")[0]!;
      await writeFile(item.target!, Buffer.from("not the image", "utf8"));
      await expect(
        saveImage(image, config, store, "mismatch", 0, undefined, undefined, {
          stable: true,
        }),
      ).rejects.toMatchObject({ code: "output_file_error" });
      expect(store.get("mismatch").outputs).toHaveLength(0);
      expect(store.listOutputs()).toHaveLength(0);
      expect(store.pendingResults("mismatch")).toHaveLength(1);
    } finally {
      store.close();
    }
  }));

it.each([
  "preintent",
  "prepared",
  "submitted",
  "finalizing",
  "running",
  "retained",
  "published",
  "committed",
])(
  "recovers an actual killed child process at %s without paid replay",
  async (phase) =>
    workspace(async (root) => {
      const config = configuration(root),
        bytes = await imageBytes();
      const configPath = join(root, "config.json"),
        imagePath = join(root, "sample.png");
      await writeFile(configPath, JSON.stringify(config), "utf8");
      await writeFile(imagePath, bytes);
      const child = spawn(
        process.execPath,
        [
          resolve("tests/fixtures/result-crash.mjs"),
          configPath,
          imagePath,
          phase,
        ],
        {
          windowsHide: true,
          stdio: ["ignore", "pipe", "pipe", "ipc"],
          env: {
            PATH: process.env.PATH ?? "",
            SystemRoot: process.env.SystemRoot ?? "",
          },
        },
      );
      let output = "";
      child.stdout!.on("data", (chunk: Buffer) => {
        output = (output + chunk.toString("utf8")).slice(-8192);
      });
      child.stderr!.on("data", (chunk: Buffer) => {
        output = (output + chunk.toString("utf8")).slice(-8192);
      });
      let checkpointReached = false;
      child.once("message", (message) => {
        checkpointReached = (message as { ready?: boolean }).ready === true;
        child.kill("SIGKILL");
      });
      const closed = new Promise<void>((accept, reject) => {
        child.once("error", reject);
        child.once("close", () => accept());
      });
      const deadline = setTimeout(() => child.kill("SIGKILL"), 5000);
      try {
        await closed;
        expect(checkpointReached, output).toBe(true);
        expect(child.signalCode === "SIGKILL" || child.exitCode !== 0).toBe(
          true,
        );
      } finally {
        clearTimeout(deadline);
        if (child.exitCode === null && child.signalCode === null) {
          child.kill("SIGKILL");
          await closed;
        }
      }
      const store = new Store(config.stateDir),
        logger = new Logger(config.logging.directory, "ERROR");
      const clock = vi.spyOn(Date, "now");
      try {
        clock.mockReturnValue(Date.now() + Store.leaseMs + 1);
        if (phase === "preintent") {
          expect(() => store.get("child-crash")).toThrow(
            expect.objectContaining({ code: "invalid_input" }),
          );
          expect(store.queued()).toBe(0);
        } else if (phase === "running") {
          store.queued();
          expect(store.get("child-crash")).toMatchObject({
            status: "running",
            upstream_job: { id: "existing-owned-job", kind: "responses" },
          });
          expect(
            (
              await getJob(
                new Generation(config, store, logger),
                "child-crash",
                false,
              )
            ).status,
          ).toBe("running");
          expect(
            store.prepare(receipt("child-crash"), "hash", "identity").fresh,
          ).toBe(false);
        } else if (["prepared", "submitted", "finalizing"].includes(phase)) {
          store.queued();
          expect(store.get("child-crash")).toMatchObject({
            status: phase === "prepared" ? "failed" : "outcome_unknown",
            generation_outcome:
              phase === "prepared" ? "not_submitted" : "unknown",
          });
          expect(
            store.prepare(receipt("child-crash"), "hash", "identity").fresh,
          ).toBe(false);
          expect(store.listOutputs()).toHaveLength(0);
        } else {
          const done = await getJob(
            new Generation(
              { ...config, connections: {}, defaultConnection: undefined },
              store,
              logger,
            ),
            "child-crash",
            true,
          );
          expect(done).toMatchObject({
            status: "completed",
            generation_outcome: "completed",
            errors: [],
          });
          expect(done.outputs).toHaveLength(1);
          expect(await readFile(done.outputs[0]!.path)).toEqual(bytes);
          expect(store.listOutputs()).toHaveLength(1);
          expect(store.pendingResults("child-crash")).toHaveLength(0);
          expect(
            (await readdir(join(config.outputDir, "crash"))).filter(
              (name) => !name.startsWith("."),
            ),
          ).toHaveLength(1);
        }
      } finally {
        clock.mockRestore();
        store.close();
        logger.close();
      }
      const reopened = new Store(config.stateDir);
      try {
        if (phase === "preintent")
          expect(() => reopened.get("child-crash")).toThrow();
        else
          expect(
            reopened.prepare(receipt("child-crash"), "hash", "identity").fresh,
          ).toBe(false);
        if (["retained", "published", "committed"].includes(phase))
          expect(reopened.get("child-crash").outputs).toHaveLength(1);
      } finally {
        reopened.close();
      }
    }),
);

it("migrates legacy receipts with a consistent backup of live WAL content", async () =>
  workspace(async (root) => {
    const directory = join(root, "state");
    await mkdir(directory);
    const legacy = new DatabaseSync(join(directory, "receipts.sqlite"));
    legacy.exec(
      "PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE requests(id TEXT PRIMARY KEY,fingerprint TEXT NOT NULL,identity TEXT NOT NULL,receipt TEXT NOT NULL,owner TEXT,lease INTEGER,cancel INTEGER NOT NULL DEFAULT 0);",
    );
    const initial = {
      ...receipt("legacy"),
      status: "completed",
      generation_outcome: "completed",
    };
    legacy
      .prepare(
        "INSERT INTO requests(id,fingerprint,identity,receipt) VALUES(?,?,?,?)",
      )
      .run("legacy", "hash", "identity", JSON.stringify(initial));
    let store: Store | undefined, backup: DatabaseSync | undefined;
    try {
      store = new Store(directory);
      expect(store.get("legacy")).toMatchObject(initial);
      const files = (await readdir(directory)).filter(
        (name) =>
          name.startsWith("receipts-before-journal-") &&
          name.endsWith(".sqlite"),
      );
      expect(files).toHaveLength(1);
      backup = new DatabaseSync(join(directory, files[0]!), { readOnly: true });
      expect(
        JSON.parse(
          String(
            backup
              .prepare("SELECT receipt FROM requests WHERE id='legacy'")
              .get()?.receipt,
          ),
        ),
      ).toEqual(initial);
      expect(store.prepare(receipt("legacy"), "hash", "identity").fresh).toBe(
        false,
      );
    } finally {
      backup?.close();
      store?.close();
      legacy.close();
    }
  }));

it.each(["write", "sync", "link", "unlink", "receipt"])(
  "retains bytes across %s failure and recovers one stable output",
  async (phase) =>
    workspace(async (root) => {
      const config = configuration(root),
        store = new Store(config.stateDir),
        logger = new Logger(config.logging.directory, "ERROR");
      const bytes = await imageBytes();
      try {
        const initial = receipt(`fault-${phase}`);
        initial.output_requirements = {
          count: 1,
          output_subdirectory: "faults",
        };
        store.prepare(initial, "hash", "identity");
        const generation = new Generation(config, store, logger);
        let fault: { mockRestore(): void };
        if (phase === "receipt")
          fault = vi.spyOn(store, "update").mockImplementationOnce(() => {
            throw new Error("Controlled receipt SQL failure");
          });
        else if (phase === "link" || phase === "unlink")
          fault = vi
            .spyOn(filesystem, phase)
            .mockRejectedValueOnce(new Error("Controlled filesystem failure"));
        else {
          const original = filesystem.open;
          fault = vi
            .spyOn(filesystem, "open")
            .mockImplementationOnce(async (...args) => {
              const handle = await original(...args);
              vi.spyOn(
                handle,
                phase === "write" ? "writeFile" : "sync",
              ).mockRejectedValueOnce(
                new Error("Controlled file handle failure"),
              );
              return handle;
            });
        }
        try {
          if (phase === "receipt")
            await expect(
              generation.finish(
                initial,
                { images: [{ bytes }], upstreamModel: null, warnings: [] },
                requestSchema.parse({ prompt: "draw" }),
                new AbortController().signal,
              ),
            ).rejects.toThrow("Controlled receipt SQL failure");
          else
            expect(
              (
                await generation.finish(
                  initial,
                  { images: [{ bytes }], upstreamModel: null, warnings: [] },
                  requestSchema.parse({ prompt: "draw" }),
                  new AbortController().signal,
                )
              ).outputs,
            ).toHaveLength(0);
        } finally {
          fault.mockRestore();
        }
        expect(store.pendingResults(initial.request_id)).toHaveLength(1);
        const done = await getJob(generation, initial.request_id, true);
        expect(done.status).toBe("completed");
        expect(done.outputs).toHaveLength(1);
        expect(await readFile(done.outputs[0]!.path)).toEqual(bytes);
        expect(store.listOutputs()).toHaveLength(1);
        expect(store.pendingResults(initial.request_id)).toHaveLength(0);
      } finally {
        vi.restoreAllMocks();
        store.close();
        logger.close();
      }
    }),
);

it("fences a stale same-instance publication after a new writer generation claims recovery", async () =>
  workspace(async (root) => {
    const config = configuration(root),
      store = new Store(config.stateDir),
      clock = vi.spyOn(Date, "now");
    try {
      clock.mockReturnValue(1000);
      const initial = receipt("same-instance");
      store.prepare(initial, "hash", "identity");
      store.reserveResults(
        initial.request_id,
        config.files.maxAggregateBytes,
        config.files.maxJournalBytes,
      );
      const image = await inspectImage(await imageBytes(), config);
      store.retainResult(initial.request_id, 0, image.bytes);
      store.claimFinalization(initial.request_id);
      const stale = store.get(initial.request_id);
      clock.mockReturnValue(1000 + Store.leaseMs + 1);
      expect(store.claimFinalization(initial.request_id)).toBe(true);
      await expect(
        saveImage(
          image,
          config,
          store,
          initial.request_id,
          0,
          "fenced",
          undefined,
          { stable: true, receipt: stale },
        ),
      ).rejects.toMatchObject({ code: "outcome_unknown" });
      expect(() =>
        store.retainResult(initial.request_id, 0, image.bytes, stale),
      ).toThrow(expect.objectContaining({ code: "outcome_unknown" }));
      expect(() => store.update({ ...stale, status: "failed" })).toThrow(
        expect.objectContaining({ code: "outcome_unknown" }),
      );
      store.release(initial.request_id, stale);
      expect(store.owns(initial.request_id)).toBe(true);
      expect(store.listOutputs()).toHaveLength(0);
    } finally {
      clock.mockRestore();
      store.close();
    }
  }));

it("one shared claim settles an original asynchronous waiter racing get_job without another POST", async ({
  signal,
}) =>
  workspace(async (root) => {
    const bytes = await imageBytes();
    let posts = 0,
      historyCalls = 0;
    let firstHistory: (() => void) | undefined;
    const bounded = AbortSignal.any([signal, AbortSignal.timeout(3000)]);
    let firstSeen!: () => void;
    const seen = new Promise<void>((accept) => {
      firstSeen = accept;
    });
    await server(
      (req, res) => {
        const path = new URL(req.url!, "http://fixture").pathname;
        if (path === "/object_info")
          json(res, {
            Text: { input: { required: { text: ["STRING"] } } },
            Save: {
              output_node: true,
              input: { required: { text: ["STRING"] } },
            },
          });
        else if (path === "/system_stats")
          json(res, { devices: [{ type: "cuda" }] });
        else if (path === "/prompt") {
          posts++;
          json(res, { prompt_id: "race" });
        } else if (path === "/history/race") {
          historyCalls++;
          const respond = () =>
            json(res, {
              race: {
                status: { completed: true, status_str: "success" },
                outputs: {
                  save: {
                    images: [
                      { filename: "final.png", subfolder: "", type: "output" },
                    ],
                  },
                },
              },
            });
          if (historyCalls === 1) {
            let responded = false;
            firstHistory = () => {
              if (!responded) {
                responded = true;
                respond();
              }
            };
            firstSeen();
          } else respond();
        } else if (path === "/view") {
          res.writeHead(200, { "Content-Type": "image/png" });
          res.end(bytes);
        } else json(res, {}, 404);
      },
      async (origin) => {
        const c = connection("comfyui", {
          baseUrl: origin,
          baseUrlMode: "origin",
          defaultModel: "workflow",
          workflow: {
            graph: {
              text: { class_type: "Text", inputs: { text: "" } },
              save: { class_type: "Save", inputs: { text: "output" } },
            },
            bindings: { prompt: { node: "text", input: "text" } },
            outputNodes: ["save"],
          },
        });
        const config = configuration(root, { local: c }),
          store = new Store(config.stateDir),
          logger = new Logger(config.logging.directory, "ERROR");
        const generation = new Generation(config, store, logger);
        let original: Promise<ReturnType<Store["get"]>> | undefined;
        try {
          original = generation.run(
            { prompt: "draw", request_id: "race" },
            "generate",
            bounded,
          );
          void original.catch(firstSeen);
          await new Promise<void>((resolve, reject) => {
            const abort = () => reject(bounded.reason);
            bounded.addEventListener("abort", abort, { once: true });
            void seen.then(() => {
              bounded.removeEventListener("abort", abort);
              resolve();
            });
          });
          const recovered = await getJob(generation, "race", true, bounded);
          firstHistory!();
          await original;
          expect(recovered.status).toBe("completed");
          expect(store.get("race").status).toBe("completed");
          expect(store.get("race").outputs).toHaveLength(1);
          expect(store.listOutputs()).toHaveLength(1);
          expect(posts).toBe(1);
        } finally {
          firstHistory?.();
          await original?.catch(() => {});
          store.close();
          logger.close();
        }
      },
    );
  }));
