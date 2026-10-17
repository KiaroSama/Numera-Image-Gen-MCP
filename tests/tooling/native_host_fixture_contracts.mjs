import assert from "node:assert/strict";
import { request } from "node:http";
import { test } from "node:test";
import { provider } from "../../scripts/native-host-fixture.mjs";
import { probeStages } from "../../scripts/native-terminal-acceptance.mjs";

test("probe diagnostics expose only fixed stages and numeric durations", () => {
  const before = "Earlier capture\n";
  const privateText = "synthetic-private";
  const rows = [
    {
      component: "tool",
      tool: "generate_image",
      message: "Tool started.",
      call_id: privateText,
      timestamp: "2026-10-17T00:00:00.000Z",
    },
    {
      component: "generation",
      message: "Submission intent persisted.",
      requestId: privateText,
      timestamp: "2026-10-17T00:00:00.600Z",
      before_submit_ms: 1000,
      prepare_ms: 900,
      admission_ms: 80,
      reserve_ms: 20,
      update_ms: 1,
    },
    {
      component: "http",
      message: "Submission transport prepared.",
      client_build_ms: 900,
      pre_send_ms: 901,
      timeout_ms: 700,
      timestamp: "2026-10-17T00:00:00.635Z",
      url: privateText,
      authorization: privateText,
    },
    {
      component: "http",
      message: "Submission transport prepared.",
      client_build_ms: -1,
      pre_send_ms: Infinity,
      timeout_ms: privateText,
    },
    {
      component: "tool",
      tool: "generate_image",
      message: "Tool completed.",
      duration_ms: 701,
      timestamp: "2026-10-17T00:00:01.572Z",
      prompt: privateText,
    },
    {
      component: "tool",
      tool: "generate_image",
      message: "Tool failed.",
      duration_ms: privateText,
    },
    { component: "tool", tool: "generate_image", message: "constructor" },
    { component: "tool", tool: "get_job", message: "Tool started." },
    null,
  ];
  const after =
    before + rows.map((row) => JSON.stringify(row)).join("\n") + "\n";
  const projected = probeStages(before, after + '{"unfinished":');
  assert.deepEqual(projected, {
    capture: "available_rows_only",
    stages: [
      { stage: "started", elapsed_from_first_ms: 0 },
      {
        stage: "intent",
        before_submit_ms: 1000,
        prepare_ms: 900,
        admission_ms: 80,
        reserve_ms: 20,
        update_ms: 1,
        elapsed_from_first_ms: 600,
      },
      {
        stage: "transport_prepared",
        client_build_ms: 900,
        pre_send_ms: 901,
        timeout_ms: 700,
        elapsed_from_first_ms: 635,
      },
      { stage: "transport_prepared" },
      { stage: "completed", duration_ms: 701, elapsed_from_first_ms: 1572 },
      { stage: "failed" },
    ],
  });
  assert.equal(JSON.stringify(projected).includes(privateText), false);
  assert.equal(JSON.stringify(projected).includes("2026-10-17"), false);
  assert.deepEqual(
    probeStages(
      "",
      JSON.stringify({
        component: "generation",
        message: "Submission intent persisted.",
        before_submit_ms: privateText,
        prepare_ms: -1,
        admission_ms: Infinity,
        reserve_ms: {},
        update_ms: null,
      }) + "\n",
    ).stages,
    [{ stage: "intent" }],
  );
  const timed = (timestamp) =>
    JSON.stringify({
      component: "generation",
      message: "Submission intent persisted.",
      timestamp,
    }) + "\n";
  for (const timestamp of [
    null,
    0,
    {},
    privateText,
    "2026-99-17T00:00:00.000Z",
    "2026-10-17T00:00:00Z",
  ])
    assert.deepEqual(probeStages("", timed(timestamp)).stages, [
      { stage: "intent" },
    ]);
  assert.deepEqual(
    probeStages(
      "",
      [
        "2026-10-17T00:00:00.600Z",
        "2026-10-17T00:00:00.000Z",
        "2026-10-17T00:00:00.635Z",
      ]
        .map(timed)
        .join(""),
    ).stages,
    [
      { stage: "intent", elapsed_from_first_ms: 0 },
      { stage: "intent" },
      { stage: "intent", elapsed_from_first_ms: 35 },
    ],
  );
  assert.equal(probeStages("", after.repeat(3)).stages.length, 8);
  assert.deepEqual(probeStages(before, after.slice(before.length)), {
    capture: "unobserved",
    stages: [],
  });
});

async function bounded(promise, ms = 1000) {
  let timer;
  try {
    return await Promise.race([
      promise,
      new Promise((_, reject) => {
        timer = setTimeout(
          () => reject(new Error("Owned helper contract timed out.")),
          ms,
        );
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

for (const runFails of [false, true]) {
  test(
    `late provider-handler error fails cleanup with run failure=${runFails}`,
    { timeout: 5000 },
    async () => {
      const late = new Error("Controlled late handler failure");
      const original = new Error("Controlled original scenario failure");
      let enter;
      const entered = new Promise((accept) => {
        enter = accept;
      });
      let client;
      let clientClosed;
      await assert.rejects(
        provider(
          async (_incoming, response) => {
            const closed = new Promise((accept) =>
              response.once("close", accept),
            );
            enter();
            await bounded(closed);
            throw late;
          },
          async (origin) => {
            client = request(origin);
            client.on("error", () => {});
            clientClosed = new Promise((accept) =>
              client.once("close", accept),
            );
            client.end();
            await bounded(entered);
            if (runFails) throw original;
            return "Scenario body passed before late failure";
          },
          bounded,
        ),
        (error) => {
          assert.ok(error instanceof AggregateError);
          assert.ok(error.errors.includes(late));
          if (runFails) assert.equal(error.errors[0], original);
          return true;
        },
      );
      await bounded(clientClosed);
      assert.equal(client.destroyed, true);
    },
  );
}
