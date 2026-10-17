"""Install verified official Rust toolchains only inside a hosted project workspace."""
from __future__ import annotations

import hashlib
import os
from pathlib import Path
import platform
import sys
import time
import urllib.error
import urllib.request

import importlib.util

_spec = importlib.util.spec_from_file_location("native_ci", Path(__file__).with_name("native-ci.py"))
_ci = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_ci)
environment, run, emit, ROOT = _ci.environment, _ci.run, _ci.emit, _ci.ROOT


def fetch(url: str, limit: int) -> bytes:
    deadline = time.monotonic() + 90
    for attempt in range(1, 4):
        data = bytearray()
        try:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise RuntimeError("Official download exceeded its wall ceiling")
            with urllib.request.urlopen(url, timeout=min(30, remaining)) as response:
                length = response.headers.get("Content-Length")
                if length is not None and (not length.isdecimal() or int(length) > limit):
                    raise RuntimeError("Official download has an invalid or oversized length")
                while chunk := response.read1(min(65536, limit + 1 - len(data))):
                    data.extend(chunk)
                    if len(data) > limit:
                        raise RuntimeError("Official download exceeds bound")
                    if len(data) // (1024 * 1024) != (len(data) - len(chunk)) // (1024 * 1024):
                        emit("INFO", "Official download progress.", attempt=attempt, bytes=len(data))
                    if time.monotonic() >= deadline:
                        raise RuntimeError("Official download exceeded its wall ceiling")
                if length is not None and len(data) != int(length):
                    raise RuntimeError("Official download is incomplete")
            emit("INFO", "Official download received; checksum validation is still required.",
                 attempt=attempt, bytes=len(data))
            return bytes(data)
        except (TimeoutError, urllib.error.URLError) as error:
            retryable = isinstance(error, TimeoutError) or isinstance(getattr(error, "reason", None), TimeoutError)
            emit("WARNING", "Official download transport failed.", attempt=attempt,
                 received_bytes=len(data), retryable=retryable, error_type=type(error).__name__)
            if not retryable or attempt == 3 or time.monotonic() >= deadline:
                raise RuntimeError("Official download transport failed within its bounds") from None
    raise RuntimeError("Official download attempts exhausted")


def main() -> None:
    if os.environ.get("GITHUB_ACTIONS") != "true":
        raise RuntimeError("Rust toolchain installation is hosted CI only")
    if len(sys.argv) == 4 and sys.argv[1] == "--fetch":
        sys.stdout.buffer.write(fetch(sys.argv[2], int(sys.argv[3])))
        return
    env = environment()
    def download(url: str, limit: int) -> bytes:
        return run([sys.executable, "-I", "-B", str(Path(__file__).resolve()), "--fetch", url, str(limit)],
                   wall=90, idle=35, env=env, capture=True, capture_limit=limit)
    machine = platform.machine().lower()
    arch = "aarch64" if machine in ["arm64", "aarch64"] else "x86_64" if machine in ["amd64", "x86_64"] else None
    system = {"Windows": "pc-windows-msvc", "Linux": "unknown-linux-gnu", "Darwin": "apple-darwin"}.get(platform.system())
    if arch is None or system is None:
        raise RuntimeError("Unsupported hosted target")
    target = arch + "-" + system
    name = "rustup-init.exe" if os.name == "nt" else "rustup-init"
    url = f"https://static.rust-lang.org/rustup/archive/1.29.1/{target}/{name}"
    data = download(url, 64 * 1024 * 1024)
    expected = download(url + ".sha256", 4096).decode("ascii").split()[0]
    if len(expected) != 64 or hashlib.sha256(data).hexdigest() != expected:
        raise RuntimeError("Official rustup checksum mismatch")
    installer = ROOT / ".ci-work/native/toolchain" / name
    installer.parent.mkdir(parents=True, exist_ok=True)
    installer.write_bytes(data)
    installer.chmod(0o700)
    run([str(installer), "-y", "--no-modify-path", "--profile", "minimal", "--default-toolchain", "1.99.1"], wall=600, idle=180, env=env)
    rustup = str(Path(env["CARGO_HOME"]) / "bin" / ("rustup.exe" if os.name == "nt" else "rustup"))
    run([rustup, "component", "add", "rustfmt", "clippy", "--toolchain", "1.99.1"], wall=180, idle=90, env=env)
    if "--coverage" in sys.argv:
        run([rustup, "toolchain", "install", "nightly-2026-10-10", "--profile", "minimal", "--component", "llvm-tools-preview"], wall=600, idle=180, env=env)
    with open(os.environ["GITHUB_PATH"], "a", encoding="utf-8") as file:
        file.write(str(Path(env["CARGO_HOME"]) / "bin") + "\n")
    with open(os.environ["GITHUB_ENV"], "a", encoding="utf-8") as file:
        for key in ["CARGO_HOME", "RUSTUP_HOME", "CARGO_TARGET_DIR", "TEMP", "TMP", "TMPDIR", "npm_config_cache"]:
            file.write(f"{key}={env[key]}\n")
    emit("INFO", "Official Rust setup completed inside workspace.", target=target, rust="1.99.1", rustup="1.29.1")


if __name__ == "__main__":
    main()
