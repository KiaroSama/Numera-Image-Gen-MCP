"""Authenticate current-run compiler intermediates before protected platform signing."""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import shutil
import stat
import sys
import tarfile
import zipfile

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("stage_signing", ROOT / "scripts/native-signing.py")
signing = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(signing)


def stage(download: Path, destination: Path, target: str, sha: str) -> Path:
    if target not in signing.TARGETS or not re.fullmatch(r"[a-f0-9]{40}", sha) or destination.exists():
        raise RuntimeError("Invalid or already occupied signing staging")
    prefix = "unsigned-intermediate-numera-image-gen-mcp-0.2.0-" + target
    name = prefix + (".zip" if "windows" in target else ".tar.gz")
    if {p.name for p in download.iterdir()} != {name, name + ".sha256"}:
        raise RuntimeError("Unexpected current-run intermediate members")
    payload = signing.read(download / name, signing.MAX_BYTES)
    if signing.read(download / (name + ".sha256"), 1024).decode("utf-8").split() != [hashlib.sha256(payload).hexdigest(), name]:
        raise RuntimeError("Intermediate checksum mismatch")
    entries = {}
    total = 0

    def accept(member: str, size: int, read):
        nonlocal total
        if not member.startswith(prefix + "/") or len(entries) >= 4096 or not 0 <= size <= signing.MAX_BYTES:
            raise RuntimeError("Invalid intermediate member")
        relative = signing.distribution_name(member[len(prefix) + 1:])
        if relative.casefold() in {p.casefold() for p in entries} or relative == "manifest.sig":
            raise RuntimeError("Duplicate or already signed intermediate member")
        total += size
        if total > signing.MAX_BYTES:
            raise RuntimeError("Intermediate aggregate bound")
        data = read()
        if len(data) != size:
            raise RuntimeError("Incomplete intermediate member")
        entries[relative] = data

    if "windows" in target:
        with zipfile.ZipFile(io.BytesIO(payload)) as bundle:
            for item in bundle.infolist():
                mode = item.external_attr >> 16
                if item.is_dir() or stat.S_IFMT(mode) not in (0, stat.S_IFREG):
                    raise RuntimeError("Intermediate must contain regular files only")
                accept(item.filename, item.file_size, lambda item=item: bundle.read(item))
    else:
        with tarfile.open(fileobj=io.BytesIO(payload), mode="r:gz") as bundle:
            for item in bundle:
                if not item.isfile():
                    raise RuntimeError("Intermediate must contain regular files only")
                def read(item=item):
                    with bundle.extractfile(item) as stream:
                        return stream.read(item.size + 1)
                accept(item.name, item.size, read)
    manifest = json.loads(entries["manifest.json"].decode("utf-8"), object_pairs_hook=signing.pairs)
    if (set(manifest) != {"version", "target", "source_sha", "publisher", "binary_sha256", "files"}
            or manifest["source_sha"] != sha or manifest["target"] != target
            or manifest["version"] != "0.2.0" or manifest["publisher"] != "KiaroSama"):
        raise RuntimeError("Intermediate source, version or target mismatch")
    rows = manifest["files"]
    if not isinstance(rows, list) or not rows or len(rows) > 4096:
        raise RuntimeError("Invalid intermediate file inventory")
    expected = {"manifest.json"}
    for row in rows:
        if (not isinstance(row, dict) or set(row) != {"path", "bytes", "sha256"}
                or row["path"] in expected or type(row["bytes"]) is not int
                or not isinstance(row["sha256"], str)):
            raise RuntimeError("Invalid intermediate file row")
        data = entries.get(row["path"])
        if data is None or row["bytes"] != len(data) or row["sha256"] != hashlib.sha256(data).hexdigest():
            raise RuntimeError("Intermediate file identity mismatch")
        expected.add(row["path"])
    binary = "bin/numera-image-gen-mcp" + (".exe" if "windows" in target else "")
    developer = "bin/numera-dev" + (".exe" if "windows" in target else "")
    if (set(entries) != expected or binary not in entries or developer not in entries
            or manifest["binary_sha256"] != hashlib.sha256(entries[binary]).hexdigest()
            or entries.get("docs/assets/logo.png") != (ROOT / "docs/assets/logo.png").read_bytes()):
        raise RuntimeError("Intermediate allowlist, binary or logo mismatch")
    destination.mkdir(parents=True, mode=0o700)
    try:
        for relative, data in entries.items():
            path = destination / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            with path.open("xb") as stream:
                stream.write(data)
            path.chmod(0o755 if relative.startswith("bin/") else 0o644)
        signing.manifest(destination.resolve(strict=True), target)
    except BaseException:
        shutil.rmtree(destination)
        raise
    return destination


def tools(target: str) -> None:
    candidates = [Path(p) for p in [shutil.which("openssl"), os.environ.get("OPENSSL_ROOT_DIR")] if p]
    if sys.platform == "darwin":
        prefix = signing.RUNNER.run(["brew", "--prefix", "openssl@3"], wall=15, idle=10,
                                     env=os.environ.copy(), capture=True).decode("utf-8").strip()
        candidates.insert(0, Path(prefix) / "bin/openssl")
    openssl = None
    for candidate in candidates:
        if candidate.is_dir():
            candidate = candidate / "bin/openssl.exe"
        if candidate.is_absolute() and candidate.is_file():
            version = signing.RUNNER.run([str(candidate), "version"], wall=10, idle=5,
                env=os.environ.copy(), capture=True).decode("utf-8")
            if re.match(r"OpenSSL 3\.\d+\.\d+\b", version):
                openssl = candidate.resolve()
                break
    if openssl is None:
        raise RuntimeError("Compatible preinstalled OpenSSL 3 is required; no legacy fallback")
    values = {"NUMERA_OPENSSL": str(openssl)}
    if "apple" in target:
        signing.RUNNER.run([sys.executable, "-I", "-B", str(ROOT / "scripts/native-macos-signing.py"), "--qualify"],
                           wall=10, idle=5, env=os.environ.copy())
    if "windows" in target:
        directory = ROOT / ".ci-work/signing-tools/osslsigncode-2.14"
        directory.mkdir(parents=True, exist_ok=False)
        url = "https://github.com/mtrojnar/osslsigncode/releases/download/2.14/osslsigncode-2.14-windows-x64-mingw.zip"
        data = signing.RUNNER.run([sys.executable, "-I", "-B", str(ROOT / "scripts/native-setup.py"), "--fetch", url, "4194304"],
            wall=90, idle=35, env=os.environ.copy(), capture=True, capture_limit=4194304)
        if hashlib.sha256(data).hexdigest() != "9a1722aaf62a27852c4eb9c35749a0248065052d0ae0a93d4ed6bb49def027f2":
            raise RuntimeError("Official signing tool digest mismatch")
        allowed = {"bin/osslsigncode.exe", "bin/libcrypto-3-x64.dll", "bin/libssl-3-x64.dll", "bin/zlib1.dll",
                   "share/bash-completion/completions/osslsigncode.bash", "share/doc/osslsigncode/NEWS.md", "share/doc/osslsigncode/README.md"}
        with zipfile.ZipFile(io.BytesIO(data)) as bundle:
            if set(bundle.namelist()) != allowed or len(bundle.infolist()) != len(allowed):
                raise RuntimeError("Unexpected official signing tool archive")
            for item in bundle.infolist():
                if item.file_size > 8388608:
                    raise RuntimeError("Signing tool member exceeds bound")
                path = directory / item.filename
                path.parent.mkdir(parents=True, exist_ok=True)
                with path.open("xb") as stream:
                    stream.write(bundle.read(item))
        values["NUMERA_OSSLSIGNCODE"] = str(directory / "bin/osslsigncode.exe")
        signing.RUNNER.run([values["NUMERA_OSSLSIGNCODE"], "--version"], wall=10, idle=5, env=os.environ.copy())
    with open(os.environ["GITHUB_ENV"], "a", encoding="utf-8") as stream:
        for key, value in values.items():
            if "\n" in value or "\r" in value:
                raise RuntimeError("Invalid signing tool path")
            stream.write(f"{key}={value}\n")
    print(json.dumps({"component": "native-signing-stage", "tools_qualified": True, "target": target}), flush=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("action", choices=("stage", "tools"))
    parser.add_argument("--target", required=True, choices=sorted(signing.TARGETS))
    args = parser.parse_args()
    if (os.environ.get("GITHUB_ACTIONS") != "true" or os.environ.get("GITHUB_REPOSITORY") != "KiaroSama/Numera-Image-Gen-MCP"
            or os.environ.get("GITHUB_REF") != "refs/heads/feature/native-rust-distribution"
            or os.environ.get("GITHUB_EVENT_NAME") != "workflow_dispatch"):
        raise RuntimeError("Only the authorized manual signing route is permitted")
    if args.action == "tools":
        tools(args.target)
    else:
        stage(ROOT / ".ci-work/signing-download", ROOT / ".ci-work/prepared" / args.target,
              args.target, os.environ["GITHUB_SHA"])
        print(json.dumps({"component": "native-signing-stage", "target": args.target, "source_sha": os.environ["GITHUB_SHA"],
                          "intermediate_authenticated": True, "signed": False}), flush=True)


if __name__ == "__main__":
    main()
