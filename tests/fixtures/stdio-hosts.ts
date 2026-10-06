import { Client } from "@modelcontextprotocol/client";
import { StdioClientTransport } from "@modelcontextprotocol/client/stdio";
import { writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { expect, vi } from "vitest";
import type { Config } from "../../src/config/schema.js";
import type { Receipt } from "../../src/jobs/store.js";
import { Store } from "../../src/jobs/store.js";
import { connectionIdentity } from "../../src/services/connection-identity.js";
import { receipt } from "./runtime.js";

export function gate() {
  let resolve!: () => void;
  const promise = new Promise<void>((accept) => (resolve = accept));
  return { resolve, promise };
}

export async function bounded<T>(promise: Promise<T>, ms = 5000): Promise<T> {
  let timer: NodeJS.Timeout | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<never>((_, reject) => {
        timer = setTimeout(
          () => reject(new Error("Fixture event timed out.")),
          ms,
        );
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

export async function seedJobs(
  config: Config,
  jobs: { id: string; connection: string; kind: string; identity?: string }[],
) {
  const store = new Store(config.stateDir);
  try {
    for (const job of jobs) {
      const c = config.connections[job.connection]!;
      const r = store.prepare(
        { ...receipt(job.id, job.connection), adapter: c.adapter },
        job.id,
        job.identity ?? (await connectionIdentity(job.connection, c)),
      ).receipt;
      expect(store.admit(job.id, job.connection, 32, 1)).toBe(true);
      r.status = "running";
      r.generation_outcome = "running";
      r.upstream_job = { id: job.id, kind: job.kind };
      store.update(r);
      store.release(job.id, r);
    }
  } finally {
    store.close();
  }
}

export async function startHosts(root: string, config: Config) {
  const file = join(root, "config.json");
  await writeFile(file, JSON.stringify(config), "utf8");
  const hosts: {
    client: Client;
    transport: StdioClientTransport;
    pid: number;
    deadline: NodeJS.Timeout;
    closed: Promise<void>;
    stderr: () => string;
  }[] = [];
  const close = async () => {
    const errors: unknown[] = [];
    for (const host of hosts) {
      try {
        await host.client.close();
        await bounded(host.closed, 5000);
        await vi.waitFor(
          () => {
            expect(() => process.kill(host.pid, 0)).toThrow();
          },
          { timeout: 2000, interval: 20 },
        );
      } catch (error) {
        errors.push(error);
        if (host.transport.pid === host.pid) {
          try {
            process.kill(host.pid, "SIGKILL");
          } catch {}
          await bounded(host.closed, 2000).catch(() => {});
        }
      } finally {
        clearTimeout(host.deadline);
        await host.transport.close();
      }
    }
    if (errors.length) throw errors[0];
  };
  try {
    // Sequential startup isolates schema initialization; calls race only after both handshakes.
    for (let index = 0; index < 2; index++) {
      const transport = new StdioClientTransport({
        command: process.execPath,
        args: [resolve("dist/index.js"), "--config", file],
        env: {
          PATH: process.env.PATH ?? "",
          NUMERA_LOG_DIR: config.logging.directory,
        },
        stderr: "pipe",
      });
      let stderr = "";
      transport.stderr!.on("data", (bytes: Buffer) => {
        stderr = (stderr + bytes.toString("utf8")).slice(-4096);
      });
      const client = new Client({
        name: `terminal-host-${index}`,
        version: "1",
      });
      const closed = gate();
      transport.onclose = closed.resolve;
      const deadline = setTimeout(() => {
        const pid = transport.pid;
        if (pid !== null) {
          try {
            process.kill(pid, "SIGKILL");
          } catch {}
        }
      }, 45000);
      try {
        await bounded(client.connect(transport));
      } catch (error) {
        clearTimeout(deadline);
        const failedPid = transport.pid;
        await transport.close();
        if (failedPid !== null) {
          await bounded(closed.promise, 5000);
          await vi.waitFor(
            () => expect(() => process.kill(failedPid, 0)).toThrow(),
            { timeout: 2000, interval: 20 },
          );
        }
        throw new Error(`Stdio handshake failed: ${stderr}`, { cause: error });
      }
      const pid = transport.pid!;
      expect(pid).toBeGreaterThan(0);
      hosts.push({
        client,
        transport,
        pid,
        deadline,
        closed: closed.promise,
        stderr: () => stderr,
      });
    }
    expect(hosts[0]!.pid).not.toBe(hosts[1]!.pid);
    return { hosts, close };
  } catch (error) {
    await close();
    throw error;
  }
}

export async function tool(
  client: Client,
  name: string,
  args: Record<string, unknown>,
) {
  return client.callTool({ name, arguments: args }, { timeout: 5000 });
}
export async function job(
  client: Client,
  id: string,
  refresh = false,
): Promise<Receipt> {
  const result = await tool(client, "get_job", { request_id: id, refresh });
  expect(result.isError, JSON.stringify(result)).not.toBe(true);
  return result.structuredContent as Receipt;
}
