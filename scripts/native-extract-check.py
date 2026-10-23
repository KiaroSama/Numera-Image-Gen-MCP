"""Hosted allowlist/hash/extracted-executable verification; no source/runtime installation."""
from __future__ import annotations
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import tarfile
import uuid
import zipfile

ROOT = Path(__file__).resolve().parent.parent
MAX_ARCHIVE_BYTES = 256 * 1024 * 1024


def check(archive: Path, runner, *, intermediate: bool = False) -> None:
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    checksum = Path(str(archive) + ".sha256").read_text(encoding="utf-8").split()
    if checksum != [digest, archive.name]:
        raise RuntimeError("Archive checksum does not match its sidecar")
    workspace = ROOT / ".ci-work" / "extracted" / str(uuid.uuid4())
    workspace.mkdir(parents=True)
    entries: dict[str, bytes] = {}
    if archive.name.endswith(".zip"):
        with zipfile.ZipFile(archive) as source:
            for item in source.infolist():
                if item.is_dir() or item.file_size > MAX_ARCHIVE_BYTES or (item.external_attr >> 16) & 0o170000 == 0o120000:
                    raise RuntimeError("Archive must contain bounded regular files only")
                if item.filename in entries:
                    raise RuntimeError("Duplicate archive member")
                entries[item.filename] = source.read(item)
    else:
        with tarfile.open(archive, "r:gz") as source:
            for item in source:
                if not item.isfile() or item.size > MAX_ARCHIVE_BYTES or item.name in entries:
                    raise RuntimeError("Archive must contain unique bounded regular files only")
                stream = source.extractfile(item)
                if stream is None:
                    raise RuntimeError("Archive member cannot be read")
                with stream:
                    entries[item.name] = stream.read(MAX_ARCHIVE_BYTES + 1)
    if not entries or sum(map(len, entries.values())) > MAX_ARCHIVE_BYTES:
        raise RuntimeError("Archive aggregate exceeds bound")
    prefixes = set()
    for name in entries:
        path = PurePosixPath(name)
        if path.is_absolute() or ".." in path.parts or "\\" in name or ":" in name or len(path.parts) < 2:
            raise RuntimeError("Unsafe archive path")
        prefixes.add(path.parts[0])
    if len(prefixes) != 1:
        raise RuntimeError("Archive must have one installation root")
    prefix = prefixes.pop()
    manifest = json.loads(entries[prefix + "/manifest.json"].decode("utf-8"))
    if manifest["source_sha"] != os.environ["GITHUB_SHA"]:
        raise RuntimeError("Archive source SHA differs from verified candidate")
    expected = {prefix + "/" + row["path"]: row for row in manifest["files"]}
    metadata = {prefix + "/manifest.json"} | (set() if intermediate else {prefix + "/manifest.sig"})
    if set(expected) | metadata != set(entries):
        raise RuntimeError("Archive members differ from allowlisted manifest")
    protected = {"secrets.md", "config.local.json", "http-token", "http.json"}
    for name, data in entries.items():
        relative = PurePosixPath(name).relative_to(prefix)
        if any(p in {".ai", ".claude", ".specify", "specs", "state", "outputs", "logs", "node_modules"} for p in relative.parts) or relative.name in protected or (relative.name.startswith(".env") and relative.name != ".env.example"):
            raise RuntimeError("Archive contains excluded private or runtime state")
        if name in expected and (len(data) != expected[name]["bytes"] or hashlib.sha256(data).hexdigest() != expected[name]["sha256"]):
            raise RuntimeError("Archive member hash differs from manifest")
        target = workspace / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
        target.chmod(0o755 if relative.parts[0] == "bin" else 0o644)
    installed = workspace / prefix
    executable = installed / "bin" / ("numera-image-gen-mcp.exe" if os.name == "nt" else "numera-image-gen-mcp")
    if hashlib.sha256(executable.read_bytes()).hexdigest() != manifest["binary_sha256"]:
        raise RuntimeError("Extracted executable hash differs")
    spec = importlib.util.spec_from_file_location("extracted_signing", ROOT / "scripts/native-signing.py")
    signing = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(signing)
    if intermediate:
        if not archive.name.startswith("unsigned-intermediate-") or not archive.resolve().is_relative_to(ROOT / ".ci-work/intermediate"):
            raise RuntimeError("Only explicitly labeled owned compiler intermediates may skip final signing")
        signing.manifest(installed, manifest["target"])
    else:
        signature_report = signing.verify(installed, manifest["target"])
        if signature_report["source_sha"] != os.environ["GITHUB_SHA"]:
            raise RuntimeError("Extracted signature source identity differs")
    env = runner.environment()
    temp = workspace / "temp"
    temp.mkdir()
    env.update({"TEMP": str(temp), "TMP": str(temp), "TMPDIR": str(temp), "NUMERA_NATIVE_BINARY": str(executable)})
    runner.run([str(executable), "--version"], wall=10, idle=5, env=env, cwd=ROOT)
    developer = installed / "bin" / ("numera-dev.exe" if os.name == "nt" else "numera-dev")
    runner.run([str(developer), "--version"], wall=10, idle=5, env=env, cwd=ROOT)
    # The actual same SDK fixtures exercise only the extracted binary, not its source build tree.
    runner.run(["node", str(ROOT / "scripts/native-interop.mjs")], wall=180, idle=45, env=env, cwd=ROOT)
    runner.run(["node", str(ROOT / "scripts/native-http-interop.mjs")], wall=90, idle=30, env=env, cwd=ROOT)
    shutil.rmtree(workspace)
    print(json.dumps({"component": "native-extract", "archive": archive.name, "verified": True,
                      "completed_signed_build": not intermediate, "internal_intermediate": intermediate}))


def main() -> None:
    if os.environ.get("GITHUB_ACTIONS") != "true":
        raise RuntimeError("Native extracted distribution checks are hosted CI only")
    spec = importlib.util.spec_from_file_location("native_ci", ROOT / "scripts/native-ci.py")
    runner = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(runner)
    parser = argparse.ArgumentParser()
    parser.add_argument("--intermediate", action="store_true")
    args = parser.parse_args()
    directory = ROOT / (".ci-work/intermediate" if args.intermediate else ".ci-work/release")
    archives = sorted(p for p in directory.iterdir() if p.name.endswith((".zip", ".tar.gz")))
    if not archives:
        raise RuntimeError("No native archive to verify")
    for archive in archives:
        check(archive, runner, intermediate=args.intermediate)


if __name__ == "__main__":
    main()
