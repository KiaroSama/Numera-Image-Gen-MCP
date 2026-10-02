import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, appendFileSync } from "node:fs";
import { resolve } from "node:path";
const [wall, idle, script, ...args] = process.argv.slice(2);
if (!script || !Number(wall) || !Number(idle))
  throw new Error("Expected wall ms, idle ms, script and arguments.");
const root = resolve(".ci-work");
mkdirSync(root, { recursive: true });
const child = spawn(process.execPath, [script, ...args], {
  windowsHide: true,
  stdio: ["ignore", "pipe", "pipe"],
  env: { ...process.env, TEMP: root, TMP: root, TMPDIR: root },
});
appendFileSync(
  resolve(root, "processes.jsonl"),
  JSON.stringify({
    pid: child.pid,
    started: new Date().toISOString(),
    script,
  }) + "\n",
  "utf8",
);
let progress = Date.now();
let terminated = false;
function kill() {
  if (terminated) return;
  terminated = true;
  process.stderr.write(
    "Bounded command terminated: wall or idle limit reached.\n",
  );
  if (process.platform === "win32")
    spawnSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], {
      windowsHide: true,
      stdio: "ignore",
    });
  else child.kill("SIGKILL");
}
for (const [stream, target] of [
  [child.stdout, process.stdout],
  [child.stderr, process.stderr],
])
  stream.on("data", (chunk) => {
    progress = Date.now();
    target.write(chunk);
  });
const deadline = setTimeout(kill, Number(wall));
const watchdog = setInterval(
  () => {
    if (Date.now() - progress > Number(idle)) kill();
  },
  Math.min(1000, Number(idle)),
);
for (const signal of ["SIGINT", "SIGTERM"]) process.once(signal, kill);
child.once("error", (error) => {
  process.stderr.write(error.message + "\n");
  clearTimeout(deadline);
  clearInterval(watchdog);
  process.exitCode = 1;
});
child.once("close", (code) => {
  clearTimeout(deadline);
  clearInterval(watchdog);
  process.exitCode = terminated ? 124 : (code ?? 1);
});
