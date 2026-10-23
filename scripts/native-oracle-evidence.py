"""Retain only synthetic oracle inputs and exact execution provenance, never private fixture state."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import zipfile

BASELINE = "fc7da25c3b470199949b1b4c67127d77afa0c34a"
INPUTS = (
    "config.json", "policy.json", "normalization-acceptance.json", "canonical.json",
    "adapters.json", "base64.json", "image-acceptance.json", "images.json", "animated-webp.json",
    "output-requirements.json", "stream-acceptance.json", "streams.json", "raw-json.json",
    "raw-js-schema.json", "sqlite-utf16-text.json",
)
LIMIT = 16 * 1024 * 1024


def read(root: Path, name: str) -> bytes:
    file = root / name
    if file.is_symlink() or not file.resolve().is_relative_to(root.resolve()) or not file.is_file():
        raise RuntimeError("Oracle evidence file is missing or escapes its root")
    if file.stat().st_size > LIMIT:
        raise RuntimeError("Oracle evidence file exceeds its byte bound")
    return file.read_bytes()


def hashes(root: Path, names: list[str]) -> dict[str, str]:
    if len(names) > 4096:
        raise RuntimeError("Oracle evidence inventory exceeds its file bound")
    return {name: hashlib.sha256(read(root, name)).hexdigest() for name in sorted(set(names))}


def files(root: Path, directory: str, suffixes: set[str]) -> list[str]:
    return [file.relative_to(root).as_posix() for file in (root / directory).rglob("*")
            if file.is_file() and file.suffix in suffixes]


def dependencies(root: Path, baseline: Path) -> dict:
    native = ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml"]
    native += files(root, "native", {".rs"}) + files(root, "tests/rust", {".rs"})
    native += files(root, "schemas", {".json"})
    generators = [file.relative_to(root).as_posix() for file in (root / "scripts").glob("rust-*oracle.mjs")]
    baseline_files = ["package.json", "package-lock.json", "node_modules/sharp/package.json",
                      "node_modules/zod/package.json"]
    for directory, suffixes in [("src", {".ts", ".json"}), ("dist", {".js", ".json"}),
                                ("tests", {".ts"}), ("node_modules/sharp/lib", {".js"})]:
        baseline_files += files(baseline, directory, suffixes)
    harness = [file.relative_to(root).as_posix() for file in (root / "scripts").glob("native-*")
               if file.is_file() and file.suffix in {".py", ".mjs"}]
    return {"native_files": hashes(root, native), "generators": hashes(root, generators),
            "harness_files": hashes(root, harness), "baseline_files": hashes(baseline, baseline_files)}


def archive_path(root: Path) -> Path:
    return root / ".ci-work/native/oracle-evidence.zip"


def capture(root: Path, baseline: Path, sha: str, baseline_sha: str) -> Path:
    archive = archive_path(root)
    if archive.exists():
        raise RuntimeError("Oracle evidence already exists")
    if not re.fullmatch(r"[0-9a-f]{40}", sha) or baseline_sha != BASELINE:
        raise RuntimeError("Oracle evidence commit identity is invalid")
    inputs = {}
    payloads = {}
    for name in INPUTS:
        data = read(root, ".ci-work/parity/" + name)
        json.loads(data.decode("utf-8"))
        payloads[name] = data
        inputs[name] = {"sha256": hashlib.sha256(data).hexdigest(), "bytes": len(data)}
    manifest = {"schema_version": 1, "source_sha": sha, "baseline": baseline_sha,
                "platform": platform.system(), "machine": platform.machine(),
                "inputs": inputs, **dependencies(root, baseline)}
    archive.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(archive, "x", compression=zipfile.ZIP_DEFLATED) as bundle:
        bundle.writestr("provenance.json", json.dumps(manifest, ensure_ascii=False, sort_keys=True).encode("utf-8"))
        for name, data in payloads.items():
            bundle.writestr(name, data)
    return archive


def verify(root: Path, baseline: Path, sha: str) -> None:
    archive = archive_path(root)
    if archive.is_symlink() or archive.stat().st_size > (len(INPUTS) + 1) * LIMIT:
        raise RuntimeError("Oracle evidence archive is invalid")
    with zipfile.ZipFile(archive) as bundle:
        members = bundle.infolist()
        if len(members) != len(INPUTS) + 1 or {item.filename for item in members} != {"provenance.json", *INPUTS}:
            raise RuntimeError("Oracle evidence archive has unexpected members")
        if any(item.file_size > LIMIT for item in members):
            raise RuntimeError("Oracle evidence archive member exceeds its bound")
        manifest = json.loads(bundle.read("provenance.json").decode("utf-8"))
        if manifest["schema_version"] != 1 or manifest["source_sha"] != sha or manifest["baseline"] != BASELINE:
            raise RuntimeError("Oracle evidence identity changed")
        for name in INPUTS:
            data = read(root, ".ci-work/parity/" + name)
            if (bundle.read(name) != data or manifest["inputs"][name] !=
                    {"sha256": hashlib.sha256(data).hexdigest(), "bytes": len(data)}):
                raise RuntimeError("Oracle evidence input changed")
        for key, value in dependencies(root, baseline).items():
            if manifest[key] != value:
                raise RuntimeError("Oracle evidence dependency changed")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("phase", choices=["capture", "verify"])
    args = parser.parse_args()
    if os.environ.get("GITHUB_ACTIONS") != "true":
        raise RuntimeError("Actual oracle evidence capture is hosted CI only")
    root = Path(__file__).resolve().parent.parent
    baseline = Path(os.environ["NUMERA_ORACLE_ROOT"]).resolve()
    sha = os.environ["GITHUB_SHA"]
    for directory, expected in ((root, sha), (baseline, BASELINE)):
        actual = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=directory, timeout=8).decode("utf-8").strip()
        if actual != expected:
            raise RuntimeError("Oracle evidence checkout identity differs")
    if args.phase == "capture":
        capture(root, baseline, sha, BASELINE)
    verify(root, baseline, sha)
    print(json.dumps({"component": "native-oracle-evidence", "phase": args.phase,
                      "source_sha": sha, "baseline": BASELINE, "inputs": len(INPUTS), "verified": True}), flush=True)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, RuntimeError, zipfile.BadZipFile, subprocess.SubprocessError):
        print(json.dumps({"level": "ERROR", "component": "native-oracle-evidence",
                          "message": "Hosted oracle evidence validation failed."}), flush=True)
        raise SystemExit(1) from None
