import { createRequire } from "node:module";
import { pathToFileURL } from "node:url";
import { readFile, writeFile, mkdir, realpath } from "node:fs/promises";
import { relative, isAbsolute, join } from "node:path";
import { createHash } from "node:crypto";
import { maintenance } from "./logging.mjs";

// Hosted runtime collection expands declaration loops without executing test bodies.
await maintenance("native-case-inventory", async (log) => {
  if (process.env.GITHUB_ACTIONS !== "true")
    throw new Error("Native acceptance inventory is hosted CI only.");
  const root = await realpath(process.env.GITHUB_WORKSPACE ?? process.cwd());
  const baseline = await realpath(process.env.NUMERA_ORACLE_ROOT);
  const contained = (file, parent) => {
    const path = relative(parent, file);
    return (
      path !== ".." &&
      !path.startsWith(".." + (process.platform === "win32" ? "\\" : "/")) &&
      !isAbsolute(path)
    );
  };
  if (!contained(baseline, root))
    throw new Error("Baseline collection must remain in the hosted workspace.");
  const requireBaseline = createRequire(join(baseline, "package.json"));
  log.emit("INFO", "Loading frozen collection API.");
  const { createVitest } = await import(
    pathToFileURL(requireBaseline.resolve("vitest/node")).href
  );
  log.emit("INFO", "Frozen collection API loaded.");
  let context;
  try {
    log.emit("INFO", "Creating frozen collection context.");
    context = await createVitest({
      root: baseline,
      watch: false,
      run: true,
      maxWorkers: 2,
      pool: "forks",
      includeTaskLocation: true,
      cacheDir: join(root, ".ci-cache", "case-inventory"),
      coverage: { enabled: false },
      reporters: [],
    });
    if (context.version !== "5.0.3")
      throw new Error(
        "Frozen case collection requires the baseline Vitest version.",
      );
    log.emit("INFO", "Frozen collection context created.");
    const specifications = await context.globTestSpecifications();
    log.emit("INFO", "Frozen collection specifications discovered.", {
      specifications: specifications.length,
    });
    const collected = await context.collectTests(specifications);
    log.emit("INFO", "Frozen runtime collection returned.", {
      collected_modules: collected.testModules.length,
    });
    if (collected.unhandledErrors.length || process.exitCode)
      throw new Error("Frozen baseline case collection failed.");
    const cases = [];
    const suites = [];
    const ids = new Set();
    for (const module of collected.testModules) {
      const file = await realpath(module.moduleId);
      if (!contained(file, baseline) || module.state() === "failed")
        throw new Error(
          "Baseline test module is invalid or failed collection.",
        );
      const source = relative(baseline, file).replaceAll("\\", "/");
      if (!/^tests\/(unit|contract|integration)\/.+\.test\.ts$/.test(source))
        throw new Error("Unexpected frozen baseline test module.");
      const sha256 = createHash("sha256")
        .update(await readFile(file))
        .digest("hex");
      let ordinal = 0;
      for (const test of module.children.allTests()) {
        const location = test.location;
        const mode = test.options.mode;
        if (
          !location ||
          !Number.isSafeInteger(location.line) ||
          location.line < 1 ||
          !Number.isSafeInteger(location.column) ||
          location.column < 1 ||
          !test.fullName ||
          !["run", "skip", "todo"].includes(mode)
        )
          throw new Error("Collected case has no stable declaration identity.");
        const id = createHash("sha256")
          .update(
            JSON.stringify([
              source,
              sha256,
              ordinal++,
              test.fullName,
              location,
            ]),
          )
          .digest("hex");
        if (ids.has(id)) throw new Error("Duplicate collected case identity.");
        ids.add(id);
        cases.push({
          id,
          source,
          source_sha256: sha256,
          name: test.fullName,
          location,
          mode,
          collection_state: test.result().state,
        });
      }
      if (!ordinal)
        throw new Error("Frozen suite contains no collected cases.");
      suites.push({ source, sha256, cases: ordinal });
    }
    if (suites.length !== 50 || cases.length !== 470)
      throw new Error(
        "Frozen source inventory differs from 50 suites / 470 cases.",
      );
    suites.sort((a, b) => a.source.localeCompare(b.source, "en"));
    cases.sort(
      (a, b) =>
        a.source.localeCompare(b.source, "en") ||
        a.location.line - b.location.line ||
        a.location.column - b.location.column ||
        a.id.localeCompare(b.id, "en"),
    );
    const output = join(root, ".ci-work", "parity");
    await mkdir(output, { recursive: true });
    await writeFile(
      join(output, "source-cases.json"),
      JSON.stringify(
        {
          baseline: "fc7da25c3b470199949b1b4c67127d77afa0c34a",
          runtime: process.version,
          platform: process.platform,
          architecture: process.arch,
          source_suites: suites.length,
          source_cases: cases.length,
          test_bodies_executed: false,
          case_mapping_complete: false,
          suites,
          cases,
        },
        null,
        2,
      ) + "\n",
      "utf8",
    );
    log.emit(
      "INFO",
      "Expanded frozen cases collected; native mapping remains incomplete.",
      {
        suites: suites.length,
        cases: cases.length,
        skipped: cases.filter((test) => test.collection_state === "skipped")
          .length,
      },
    );
  } finally {
    log.emit("INFO", "Closing frozen collection context.");
    await context?.close();
    log.emit("INFO", "Frozen collection context closed.");
  }
});
