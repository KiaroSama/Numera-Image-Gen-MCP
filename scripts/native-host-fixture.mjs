import assert from "node:assert/strict";
import { createServer } from "node:http";
import { writeFile } from "node:fs/promises";
import { join } from "node:path";

export async function provider(respond, run, bounded) {
  const sockets = new Set();
  const errors = [];
  const pending = new Set();
  const server = createServer((request, response) => {
    const task = Promise.resolve()
      .then(() => respond(request, response))
      .catch((error) => {
        errors.push(error);
        response.destroy();
      });
    pending.add(task);
    void task.finally(() => pending.delete(task));
  });
  server.requestTimeout = 5000;
  server.headersTimeout = 5000;
  server.on("connection", (socket) => {
    sockets.add(socket);
    socket.once("close", () => sockets.delete(socket));
  });
  let result;
  try {
    await bounded(
      new Promise((accept, reject) => {
        server.once("error", reject);
        server.listen(0, "127.0.0.1", accept);
      }),
    );
    const origin = `http://127.0.0.1:${server.address().port}`;
    result = await run(origin);
  } catch (error) {
    errors.unshift(error);
  } finally {
    const closed = server.listening
      ? new Promise((accept, reject) =>
          server.close((error) => (error ? reject(error) : accept())),
        )
      : Promise.resolve();
    for (const socket of sockets) socket.destroy();
    const cleanup = await Promise.allSettled([
      bounded(Promise.all([...pending]), 6000),
      bounded(closed),
    ]);
    errors.push(
      ...cleanup
        .filter((entry) => entry.status === "rejected")
        .map((entry) => entry.reason),
    );
  }
  // Handler failures can arrive while draining; check only after every cleanup attempt.
  if (errors.length)
    throw new AggregateError(errors, "Owned provider scenario failed.");
  return result;
}

export function json(response, value, status = 200) {
  response.writeHead(status, {
    "Content-Type": "application/json; charset=utf-8",
  });
  response.end(JSON.stringify(value));
}

export async function call(host, name, arguments_, bounded) {
  host.fixtureSignal.throwIfAborted();
  return bounded(
    host.client.callTool(
      { name, arguments: arguments_ },
      { timeout: 5000, signal: host.fixtureSignal },
    ),
    6000,
  );
}

export async function job(host, id, refresh, bounded) {
  const result = await call(
    host,
    "get_job",
    { request_id: id, refresh },
    bounded,
  );
  assert.notEqual(result.isError, true, JSON.stringify(result));
  assert.ok(result.structuredContent);
  return result.structuredContent;
}

export async function pair(root, config, context, instances) {
  const file = join(root, "config.json");
  await writeFile(file, JSON.stringify(config), "utf8");
  const hosts = [];
  try {
    // Sequential handshakes preserve the source's initialization schedule.
    for (let index = 0; index < 2; index++) {
      context.signal.throwIfAborted();
      let tail = "";
      let signalReady;
      let rejectReady;
      const ready = new Promise((accept, reject) => {
        signalReady = accept;
        rejectReady = reject;
      });
      // Readiness can reject during connect; observe it before the later await.
      void ready.catch(() => {});
      const controller = new AbortController();
      const timer = setTimeout(
        () =>
          controller.abort(
            new Error("Terminal host exceeded source lifetime."),
          ),
        45000,
      );
      const signal = AbortSignal.any([context.signal, controller.signal]);
      const aborted = () => rejectReady(signal.reason);
      signal.addEventListener("abort", aborted, { once: true });
      const cleanup = () => {
        clearTimeout(timer);
        signal.removeEventListener("abort", aborted);
      };
      let host;
      try {
        host = await context.start(
          file,
          "auto",
          { signal },
          {
            clientInfo: { name: `terminal-host-${index}`, version: "1" },
            env: { NUMERA_LOG_DIR: config.logging.directory },
            onStderr(chunk) {
              tail += chunk.toString("utf8");
              if (tail.length > 1048576) {
                const error = new Error(
                  "Terminal host diagnostic line exceeded one MiB.",
                );
                rejectReady(error);
                controller.abort(error);
                tail = "";
                return;
              }
              for (let end; (end = tail.indexOf("\n")) >= 0;) {
                const line = tail.slice(0, end);
                tail = tail.slice(end + 1);
                try {
                  const event = JSON.parse(line);
                  if (
                    event.component === "MCP" &&
                    event.message === "Stdio server ready."
                  )
                    signalReady(event.run_id);
                } catch {
                  // Non-JSON diagnostics are not a readiness event.
                }
              }
            },
          },
        );
      } catch (error) {
        cleanup();
        throw error;
      }
      host.fixtureSignal = signal;
      host.fixtureCleanup = cleanup;
      hosts.push(host);
      signal.throwIfAborted();
      const runId = await context.bounded(ready);
      assert.match(runId, /^[a-f0-9-]{36}$/);
      assert.equal(instances.has(runId), false);
      instances.add(runId);
    }
    assert.notEqual(hosts[0].pid, hosts[1].pid);
    return hosts;
  } catch (error) {
    await close(hosts, context.closeHost);
    throw error;
  }
}

export async function close(hosts, closeHost) {
  const results = await Promise.allSettled(
    hosts.map(async (host) => {
      try {
        await closeHost(host);
        host.fixtureSignal.throwIfAborted();
      } finally {
        host.fixtureCleanup();
      }
    }),
  );
  const failures = results.filter((result) => result.status === "rejected");
  if (failures.length)
    throw new AggregateError(
      failures.map((result) => result.reason),
      "Terminal host cleanup failed.",
    );
}

export async function ownedScenario(context, action, ms = 60000) {
  const controller = new AbortController();
  const timer = setTimeout(() => {
    controller.abort(
      new Error(`Native host scenario exceeded its ${ms}ms ceiling.`),
    );
  }, ms);
  try {
    // Await the operation itself: cancellation must settle its host/provider finally blocks.
    await action({ ...context, signal: controller.signal });
    controller.signal.throwIfAborted();
  } finally {
    clearTimeout(timer);
  }
}
