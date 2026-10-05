import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, appendFileSync, readdirSync, readFileSync } from "node:fs";
import { setTimeout as pause } from "node:timers/promises";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { maintenance, projectRoot } from "./logging.mjs";

function windowsSweep(pid, started) {
  // PID and creation-time checks keep recycled IDs and pre-existing processes out of cleanup.
  const script = `$ErrorActionPreference='Stop'; $all=@(Get-CimInstance Win32_Process); $ids=[Collections.Generic.HashSet[int]]::new(); [void]$ids.Add(${pid}); $owned=@(); do { $added=$false; foreach($p in $all) { if($p.ProcessId -ne ${pid} -and $ids.Contains([int]$p.ParentProcessId) -and $p.CreationDate.ToUniversalTime() -ge [DateTime]::Parse('${started}').ToUniversalTime() -and $ids.Add([int]$p.ProcessId)) { $owned+= $p; $added=$true } } } while($added); foreach($p in $owned) { $current=Get-CimInstance Win32_Process -Filter ('ProcessId='+$p.ProcessId); if($current -and $current.CreationDate -eq $p.CreationDate) { & taskkill /PID $p.ProcessId /T /F 2>$null | Out-Null } }; $alive=@(); foreach($p in $owned) { $current=Get-CimInstance Win32_Process -Filter ('ProcessId='+$p.ProcessId); if($current -and $current.CreationDate -eq $p.CreationDate) { $alive+=$p.ProcessId } }; @{ owned=@($owned | ForEach-Object { [int]$_.ProcessId }); alive=$alive } | ConvertTo-Json -Compress`;
  const result = spawnSync(
    "pwsh",
    [
      "-NoProfile",
      "-NonInteractive",
      "-EncodedCommand",
      Buffer.from(script, "utf16le").toString("base64"),
    ],
    {
      windowsHide: true,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
      timeout: 8000,
      killSignal: "SIGKILL",
      maxBuffer: 1024 * 1024,
    },
  );
  if (result.status !== 0) return { verified: false, owned: [], alive: [] };
  try {
    return { verified: true, ...JSON.parse(result.stdout) };
  } catch {
    return { verified: false, owned: [], alive: [] };
  }
}
function linuxGroup(pid) {
  const alive = [];
  for (const name of readdirSync("/proc")) {
    if (!/^\d+$/.test(name)) continue;
    try {
      const stat = readFileSync(`/proc/${name}/stat`, "utf8");
      const fields = stat.slice(stat.lastIndexOf(")") + 2).split(" ");
      if (Number(fields[2]) === pid && fields[0] !== "Z")
        alive.push(Number(name));
    } catch (error) {
      if (error.code !== "ENOENT" && error.code !== "ESRCH") throw error;
    }
  }
  return alive;
}
export async function runBounded(
  command,
  args,
  {
    wall,
    idle,
    cwd = projectRoot,
    env = process.env,
    log,
    capture = false,
  } = {},
) {
  if (
    ![wall, idle].every(
      (value) => Number.isSafeInteger(value) && value > 0 && value <= 3600000,
    )
  )
    throw Object.assign(
      new Error("Positive wall and idle ceilings up to one hour are required."),
      { code: "INVALID_BOUNDS" },
    );
  const root = resolve(cwd, ".ci-work");
  mkdirSync(root, { recursive: true });
  const started = new Date().toISOString();
  const record = (event) =>
    appendFileSync(
      resolve(root, "processes.jsonl"),
      JSON.stringify({ started, ...event }) + "\n",
      "utf8",
    );
  const child = spawn(command, args, {
    cwd,
    windowsHide: true,
    detached: process.platform !== "win32",
    stdio: ["ignore", "pipe", "pipe"],
    env: { ...env, TEMP: root, TMP: root, TMPDIR: root },
  });
  record({
    event: "start",
    pid: child.pid,
    owner_pid: process.pid,
    wall_ms: wall,
    idle_ms: idle,
  });
  log?.emit("DEBUG", "Bounded child started.", {
    pid: child.pid,
    wall_ms: wall,
    idle_ms: idle,
  });
  let progress = Date.now(),
    terminated = false,
    failed = false,
    stdout = "",
    stderr = "";
  function kill(reason) {
    if (terminated || !child.pid) return;
    terminated = true;
    log?.emit("ERROR", "Bounded command terminated.", {
      reason,
      pid: child.pid,
    });
    record({ event: "terminate", pid: child.pid, reason });
    if (process.platform === "win32") {
      // Parent is still held by its ChildProcess handle; taskkill walks live descendants.
      spawnSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], {
        windowsHide: true,
        stdio: "ignore",
        timeout: 5000,
        killSignal: "SIGKILL",
      });
    } else {
      try {
        process.kill(-child.pid, "SIGKILL");
      } catch (error) {
        if (error.code !== "ESRCH") failed = true;
      }
    }
    try {
      child.kill("SIGKILL");
    } catch {
      failed = true;
    }
  }
  const deadline = setTimeout(() => kill("wall"), wall);
  const watchdog = setInterval(
    () => {
      if (Date.now() - progress >= idle) kill("idle");
    },
    Math.min(250, idle),
  );
  const interrupt = () => kill("cancelled");
  for (const signal of ["SIGINT", "SIGTERM"]) process.once(signal, interrupt);
  for (const [stream, target, key] of [
    [child.stdout, process.stdout, "stdout"],
    [child.stderr, process.stderr, "stderr"],
  ]) {
    stream.setEncoding("utf8");
    stream.on("data", (chunk) => {
      progress = Date.now();
      if (capture) {
        if (key === "stdout") stdout += chunk;
        else stderr += chunk;
        if (stdout.length + stderr.length > 2 * 1024 * 1024)
          kill("output_limit");
      } else target.write(chunk);
    });
  }
  let hardStop;
  child.once("error", () => {
    failed = true;
    log?.emit("ERROR", "Bounded child could not start.");
  });
  // Joining close is bounded too: leaked inherited pipes must not keep the supervisor alive forever.
  const result = await new Promise((accept) => {
    child.once("close", (code) => accept(code));
    hardStop = setTimeout(() => {
      kill("cleanup_deadline");
      child.stdout.destroy();
      child.stderr.destroy();
      accept(null);
    }, wall + 15000);
  });
  clearTimeout(deadline);
  clearTimeout(hardStop);
  clearInterval(watchdog);
  for (const signal of ["SIGINT", "SIGTERM"])
    process.removeListener(signal, interrupt);
  let verified = true,
    survivors = [];
  if (child.pid) {
    if (process.platform === "win32") {
      const sweep = windowsSweep(child.pid, started);
      verified = sweep.verified;
      survivors = sweep.alive;
      if (sweep.owned.length)
        record({
          event: "descendant_cleanup",
          pid: child.pid,
          descendants: sweep.owned,
        });
      if (sweep.owned.length && !terminated) failed = true;
    } else {
      try {
        process.kill(-child.pid, "SIGKILL");
      } catch (error) {
        if (error.code !== "ESRCH") verified = false;
      }
      if (process.platform === "linux") {
        try {
          const until = Date.now() + 2000;
          while (
            (survivors = linuxGroup(child.pid)).length &&
            Date.now() < until
          )
            await pause(20);
        } catch {
          verified = false;
        }
      } else {
        const until = Date.now() + 2000;
        while (true) {
          try {
            process.kill(-child.pid, 0);
          } catch (error) {
            if (error.code !== "ESRCH") verified = false;
            break;
          }
          if (Date.now() >= until) {
            verified = false;
            break;
          }
          await pause(20);
        }
      }
    }
    try {
      process.kill(child.pid, 0);
      verified = false;
    } catch (error) {
      if (error.code !== "ESRCH") verified = false;
    }
  }
  const code = terminated
    ? 124
    : failed || !verified || survivors.length
      ? 1
      : (result ?? 1);
  record({
    event: "finish",
    pid: child.pid,
    finished: new Date().toISOString(),
    exit_code: code,
    cleanup_verified: verified && !survivors.length,
  });
  log?.emit(code ? "ERROR" : "INFO", "Bounded child finished.", {
    pid: child.pid,
    exit_code: code,
    cleanup_verified: verified && !survivors.length,
  });
  return { code, stdout, stderr };
}
if (
  process.argv[1] &&
  resolve(fileURLToPath(import.meta.url)) === resolve(process.argv[1])
)
  await maintenance("bounded", async (log) => {
    const [wall, idle, script, ...args] = process.argv.slice(2);
    if (!script)
      throw Object.assign(
        new Error("Expected wall ms, idle ms, script and arguments."),
        { code: "INVALID_REQUEST" },
      );
    const command = script === "--exec" ? args.shift() : process.execPath;
    const commandArgs = script === "--exec" ? args : [resolve(script), ...args];
    const result = await runBounded(command, commandArgs, {
      wall: Number(wall),
      idle: Number(idle),
      cwd: process.cwd(),
      log,
    });
    process.exitCode = result.code;
  });
