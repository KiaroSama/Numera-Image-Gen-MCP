"""Hosted-only Rust verification, workspace-contained artifacts and bounded child ownership."""
from __future__ import annotations

import argparse
import datetime as dt
import json
import os
from pathlib import Path
import queue
import signal
import subprocess
import sys
import threading
import time
import tomllib
import uuid

ROOT = Path(__file__).resolve().parent.parent
WORK = ROOT / ".ci-work" / "native"
LOG = None


def emit(level: str, message: str, **context) -> None:
    entry = json.dumps({"timestamp": dt.datetime.now(dt.timezone.utc).isoformat(), "level": level,
                        "component": "native-ci", "message": message, **context}, ensure_ascii=False)
    print(entry, file=sys.stderr, flush=True)
    if LOG is not None and not LOG.closed:
        LOG.write(entry + "\n")
        LOG.flush()


def environment() -> dict[str, str]:
    env = os.environ.copy()
    directories = {"CARGO_HOME": ROOT / ".ci-cache/cargo", "RUSTUP_HOME": ROOT / ".ci-cache/rustup",
                   "CARGO_TARGET_DIR": ROOT / "target", "TEMP": WORK / "temp", "TMP": WORK / "temp",
                   "TMPDIR": WORK / "temp", "npm_config_cache": ROOT / ".ci-cache/npm"}
    for key, directory in directories.items():
        directory.mkdir(parents=True, exist_ok=True)
        if not directory.resolve().is_relative_to(ROOT):
            raise RuntimeError("CI managed directory escapes workspace")
        env[key] = str(directory)
    env.update({"CARGO_BUILD_JOBS": "2", "RUST_TEST_THREADS": "2", "NUMERA_TEST_WORKERS": "2",
                "CARGO_TERM_COLOR": "never", "PYTHONDONTWRITEBYTECODE": "1"})
    env["PATH"] = str(Path(env["CARGO_HOME"]) / "bin") + os.pathsep + env["PATH"]
    return env


def run(args: list[str], *, wall: int, idle: int, env: dict[str, str], cwd: Path = ROOT, capture: bool = False, capture_limit: int = 1048576) -> bytes:
    captured = bytearray()
    started = time.monotonic()
    events: queue.Queue[tuple[str, bytes | None]] = queue.Queue()
    kwargs = {"creationflags": subprocess.CREATE_NO_WINDOW | subprocess.CREATE_NEW_PROCESS_GROUP} if os.name == "nt" else {"start_new_session": True}
    child = subprocess.Popen(args, cwd=cwd, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                             stderr=subprocess.PIPE, **kwargs)
    emit("INFO", "Owned command started.", command=Path(args[0]).name, pid=child.pid, wall_seconds=wall, idle_seconds=idle)
    readers = []
    def read(stream, name):
        try:
            while chunk := stream.read1(8192):
                events.put((name, chunk))
        finally:
            stream.close()
            events.put((name, None))
    for name, stream in [("stdout", child.stdout), ("stderr", child.stderr)]:
        reader = threading.Thread(target=read, args=(stream, name), daemon=True)
        reader.start()
        readers.append(reader)
    last = started
    ended = 0
    failed = False
    try:
        while ended < 2:
            now = time.monotonic()
            if now - started > wall or now - last > idle:
                failed = True
                raise TimeoutError("Owned command exceeded wall/idle ceiling")
            try:
                name, chunk = events.get(timeout=0.1)
            except queue.Empty:
                continue
            if chunk is None:
                ended += 1
                continue
            last = time.monotonic()
            if capture and name == "stdout":
                captured.extend(chunk)
                if len(captured) > capture_limit:
                    failed = True
                    raise RuntimeError("Owned command captured output exceeds limit")
            else:
                target = sys.stdout.buffer if name == "stdout" else sys.stderr.buffer
                target.write(chunk)
                target.flush()
        code = child.wait(timeout=5)
        if code:
            failed = True
            raise RuntimeError(f"Owned command failed with exit code {code}")
    finally:
        if child.poll() is None or failed:
            if os.name == "nt":
                subprocess.run(["taskkill", "/PID", str(child.pid), "/T", "/F"], stdin=subprocess.DEVNULL,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=8,
                               creationflags=subprocess.CREATE_NO_WINDOW, check=False)
            else:
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            if child.poll() is None:
                child.kill()
        child.wait(timeout=8)
        for reader in readers:
            reader.join(timeout=5)
            if reader.is_alive():
                raise RuntimeError("Owned command output reader did not close")
        emit("INFO", "Owned command stopped.", command=Path(args[0]).name, pid=child.pid,
             exit_code=child.returncode, duration_seconds=round(time.monotonic() - started, 3), terminated=failed)
    return bytes(captured)


def inventory() -> None:
    manifest = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    declared = {item["path"] for item in manifest.get("test", [])}
    actual = {str(path.relative_to(ROOT)).replace("\\", "/") for path in (ROOT / "tests/rust").glob("*.rs")}
    if actual != declared:
        raise RuntimeError("Native suite discovery differs from manifest")
    for directory in [ROOT / "native", ROOT / "tests/rust"]:
        for path in directory.rglob("*.rs"):
            text = path.read_text(encoding="utf-8", errors="strict")
            if len(text.splitlines()) > 800:
                raise RuntimeError("Native source exceeds 800 lines")
    emit("INFO", "Native suite and UTF-8 inventory checked.", suites=len(actual))


def coverage_gate() -> None:
    report = json.loads((WORK / "coverage.json").read_text(encoding="utf-8"))
    summaries = [item["totals"] for item in report["data"]]
    failed = []
    for metric, floor in [("regions", 85), ("branches", 80)]:
        count = sum(s[metric]["count"] for s in summaries)
        covered = sum(s[metric]["covered"] for s in summaries)
        percent = covered / count * 100 if count else 0
        emit("INFO", "Native coverage measured.", metric=metric, count=count, covered=covered, percent=round(percent, 2))
        if count == 0 or percent < floor:
            failed.append(f"{metric} below {floor}%")
    if failed:
        raise RuntimeError("Native coverage gates failed: " + ", ".join(failed))


def main() -> None:
    global LOG
    parser = argparse.ArgumentParser()
    parser.add_argument("phase", choices=["stable", "coverage", "coverage-gate", "inventory"])
    args = parser.parse_args()
    if os.environ.get("GITHUB_ACTIONS") != "true":
        raise RuntimeError("Native compilation and verification are hosted CI only")
    WORK.mkdir(parents=True, exist_ok=True)
    stamp = dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%d_%H-%M-%S")
    with (WORK / f"native-ci_{stamp}_UTC-{uuid.uuid4()}.log").open("x", encoding="utf-8") as log:
        LOG = log
        env = environment()
        inventory()
        if args.phase == "inventory":
            return
        if args.phase == "coverage-gate":
            coverage_gate()
            return
        if not (ROOT / "Cargo.lock").exists() or os.environ.get("NUMERA_RESOLVE_HTTP_LOCK") == "true":
            run(["cargo", "generate-lockfile"], wall=180, idle=90, env=env)
        if args.phase == "stable":
            failures = []
            if os.environ.get("NUMERA_COLLECT_SOURCE_CASES") == "true":
                try:
                    run(["node", "scripts/native-case-inventory.mjs"], wall=60, idle=30, env=env)
                    run([sys.executable, "-I", "-B", "scripts/native-case-ledger.py", "--source", env["NUMERA_ORACLE_ROOT"],
                         "--collection", str(ROOT / ".ci-work/parity/source-cases.json")], wall=30, idle=15, env=env)
                except (RuntimeError, TimeoutError) as error:
                    failures.append("case-inventory")
                    emit("ERROR", "Expanded source case collection failed.", error_type=type(error).__name__)
            for command, wall, idle in [
                ([sys.executable, "-I", "-B", "tests/tooling/native_ci_contracts.py"], 30, 15),
                ([sys.executable, "-I", "-B", "tests/tooling/native_case_ledger_contracts.py"], 30, 15),
                (["node", "--test", "--test-timeout=5000", "tests/tooling/native_host_fixture_contracts.mjs"], 10, 5),
                (["cargo", "fmt", "--check"], 30, 20),
                (["cargo", "clippy", "--locked", "--all-targets", "--", "-D", "warnings"], 900, 180),
                (["cargo", "test", "--locked", "--all-targets", "--no-fail-fast", "--message-format=json-render-diagnostics", "--", "--test-threads=2"], 900, 180),
                (["cargo", "build", "--locked", "--release", "--message-format=json-render-diagnostics"], 900, 360 if sys.platform == "darwin" else 180),
                (["node", "scripts/native-interop.mjs"], 180, 45),
                (["node", "scripts/native-http-interop.mjs"], 90, 30),
            ]:
                try:
                    run(command, wall=wall, idle=idle, env=env)
                except (RuntimeError, TimeoutError) as error:
                    gate = command[-1] if command[0] == sys.executable else command[1]
                    failures.append(gate)
                    emit("ERROR", "Required native gate failed.", gate=gate, error_type=type(error).__name__)
            if "fmt" in failures:
                # Keep the failed check failed; generated formatting is reviewed and committed later.
                run(["cargo", "fmt"], wall=30, idle=20, env=env)
                import zipfile
                with zipfile.ZipFile(WORK / "formatting.zip", "w", compression=zipfile.ZIP_DEFLATED) as archive:
                    for directory in [ROOT / "native", ROOT / "tests/rust"]:
                        for file in sorted(directory.rglob("*.rs")):
                            archive.write(file, file.relative_to(ROOT).as_posix())
            if failures:
                raise RuntimeError("Required native gates failed: " + ", ".join(failures))
        else:
            run(["cargo", "install", "cargo-llvm-cov", "--version", "0.9.1", "--locked", "--root", str(ROOT / ".ci-cache/cargo")], wall=600, idle=180, env=env)
            coverage = ["cargo", "+nightly-2026-10-10", "llvm-cov"]
            exported = run(coverage + ["show-env", "--branch"], wall=30, idle=20, env=env, capture=True)
            import shlex
            instrumented = env.copy()
            for line in exported.decode("utf-8").splitlines():
                fields = shlex.split(line)
                if len(fields) != 1 or "=" not in fields[0]:
                    raise RuntimeError("Invalid coverage environment assignment")
                key, value = fields[0].split("=", 1)
                if not key.replace("_", "").isalnum() or not key.isupper():
                    raise RuntimeError("Invalid coverage environment key")
                instrumented[key] = value
            for key in ["CARGO_LLVM_COV_TARGET_DIR", "CARGO_LLVM_COV_BUILD_DIR"]:
                if not Path(instrumented[key]).resolve().is_relative_to(ROOT):
                    raise RuntimeError("Instrumented build path escapes workspace")
            if not Path(instrumented["LLVM_PROFILE_FILE"]).resolve().is_relative_to(ROOT):
                raise RuntimeError("Coverage profile path escapes workspace")
            instrumented["CARGO_TARGET_DIR"] = instrumented["CARGO_LLVM_COV_TARGET_DIR"]
            profile_root = Path(instrumented["CARGO_LLVM_COV_TARGET_DIR"])
            if profile_root.exists() and any(profile_root.rglob("*.profraw")):
                raise RuntimeError("Coverage job requires a fresh workspace without prior profiles")
            failures = []
            instrumented["NUMERA_NATIVE_BINARY"] = str(Path(instrumented["CARGO_LLVM_COV_TARGET_DIR"]) / "debug/numera-image-gen-mcp")
            for command, wall, idle in [
                (["cargo", "+nightly-2026-10-10", "test", "--locked", "--all-targets", "--no-fail-fast", "--", "--test-threads=2"], 1200, 180),
                (["cargo", "+nightly-2026-10-10", "build", "--locked"], 600, 180),
                (["node", "scripts/native-interop.mjs"], 180, 45),
                (["node", "scripts/native-http-interop.mjs"], 90, 30),
                (coverage + ["report", "--branch", "--json", "--output-path", str(WORK / "coverage.json")], 60, 30),
            ]:
                try:
                    run(command, wall=wall, idle=idle, env=instrumented)
                except (RuntimeError, TimeoutError) as error:
                    failures.append(command[1])
                    emit("ERROR", "Required instrumented gate failed.", gate=command[1], error_type=type(error).__name__)
            # Preserve measured evidence after a failed suite; it never makes that suite passing.
            coverage_gate()
            if failures:
                raise RuntimeError("Required instrumented gates failed: " + ", ".join(failures))


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        emit("ERROR", "Native CI failed.", error_type=type(error).__name__)
        raise SystemExit(1) from None
