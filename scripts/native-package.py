"""Allowlisted native distribution archives; original executable built only in hosted CI."""
from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import importlib.util
import json
import os
from pathlib import Path
import platform
import tarfile
import zipfile

ROOT = Path(__file__).resolve().parent.parent


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", required=True)
    parser.add_argument("--prepare-only", action="store_true")
    parser.add_argument("--intermediate", action="store_true")
    parser.add_argument("--prepared", type=Path)
    args = parser.parse_args()
    if sum((args.prepare_only, args.intermediate, args.prepared is not None)) > 1:
        parser.error("Choose one preparation, internal intermediate or signed prepared-input mode")
    spec = importlib.util.spec_from_file_location("package_signing", ROOT / "scripts/native-signing.py")
    signing = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(signing)
    if os.environ.get("GITHUB_ACTIONS") != "true":
        raise RuntimeError("Release packaging is hosted CI only")
    if args.target not in ["x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc", "x86_64-unknown-linux-gnu",
                           "aarch64-unknown-linux-gnu", "x86_64-apple-darwin", "aarch64-apple-darwin"]:
        raise RuntimeError("Unknown native target")
    if args.prepared is not None:
        prepared = args.prepared.resolve(strict=True)
        if not prepared.is_relative_to(ROOT / ".ci-work/prepared"):
            raise RuntimeError("Only owned authenticated prepared staging can be packaged")
        report = signing.verify(prepared, args.target)
        manifest, _ = signing.manifest(prepared, args.target)
        if report["source_sha"] != os.environ.get("GITHUB_SHA") or manifest["version"] != "0.2.0":
            raise RuntimeError("Prepared signature belongs to another source or version")
        entries = {row["path"]: signing.read(signing.member(prepared, row["path"]), row["bytes"])
                   for row in manifest["files"]}
        for name in ("manifest.json", "manifest.sig"):
            entries[name] = signing.read(signing.member(prepared, name), 1024 * 1024)
        archive(entries, args.target)
        return
    prepare(signing, args.target, args.prepare_only, args.intermediate)


def prepare(signing, target: str, prepare_only: bool, intermediate: bool = False) -> None:
    executable = "numera-image-gen-mcp.exe" if "windows" in target else "numera-image-gen-mcp"
    source = ROOT / "target/release" / executable
    if not source.is_file():
        raise RuntimeError("Verified native executable is missing")
    developer = "numera-dev.exe" if "windows" in target else "numera-dev"
    developer_source = ROOT / "target/release" / developer
    if not developer_source.is_file():
        raise RuntimeError("Verified native developer executable is missing")
    entries = {"bin/" + executable: source.read_bytes(), "bin/" + developer: developer_source.read_bytes()}
    for name in ["LICENSE", "THIRD_PARTY_NOTICES.md", "README.md", ".env.example",
                 "schemas/config.schema.json", "schemas/simple-config.schema.json", "schemas/image-request.schema.json",
                 "examples/claude-desktop.json", "examples/claude-project.json", "examples/codex.toml",
                 "docs/CONFIGURATION.md", "docs/CLAUDE.md", "docs/CODEX.md", "docs/ARCHITECTURE.md", "docs/HTTP.md",
                 "docs/WINDOWS-WSL.md", "docs/COMPATIBILITY.md", "docs/IMAGE-EDITING.md", "docs/LINUX-MACOS.md",
                 "docs/REQUIREMENTS.md", "docs/REFERENCES.md", "SECURITY.md", "docs/TESTING.md",
                 "docs/PRACTICAL-TESTING.md", "docs/TEST-RESULTS.md", "docs/testing/MIGRATION-MATRIX.md",
                 "docs/testing/native-parity.json", "docs/testing/native-cases/index.json",
                 "docs/testing/native-cases/unit.jsonl", "docs/testing/native-cases/contract.jsonl", "docs/testing/native-cases/integration.jsonl",
                 "docs/assets/logo.png", "docs/DEVELOPER-TOOLS.md", "docs/SIGNING.md",
                 "docs/signing/publisher.json", "docs/signing/publisher-key.der", "docs/signing/publisher-cert.der",
                 "docs/licenses/supplements.json", "docs/licenses/alloc-stdlib-0.3.0/LICENSE", "docs/licenses/rmcp-3.5.1/LICENSE"]:
        entries[name] = (ROOT / name).read_bytes()
    notices = ROOT / ".ci-work/native/licenses"
    if not notices.is_dir() or not any(notices.iterdir()):
        raise RuntimeError("Resolved native dependency license inventory is required before packaging")
    for file in sorted(notices.rglob("*")):
        if file.is_file() and not file.is_symlink():
            entries["licenses/" + file.relative_to(notices).as_posix()] = file.read_bytes()
    manifest = {"version": "0.2.0", "target": target, "source_sha": os.environ.get("GITHUB_SHA"), "publisher": "KiaroSama",
                "binary_sha256": hashlib.sha256(entries["bin/" + executable]).hexdigest(),
                "files": [{"path": name, "sha256": hashlib.sha256(data).hexdigest(), "bytes": len(data)} for name, data in sorted(entries.items())]}
    entries["manifest.json"] = (json.dumps(manifest, indent=2) + "\n").encode("utf-8")
    prepared = ROOT / ".ci-work/prepared" / target
    prepared.mkdir(parents=True, exist_ok=False)
    for name, data in entries.items():
        signing.distribution_name(name)
        file = prepared / name
        file.parent.mkdir(parents=True, exist_ok=True)
        with file.open("xb") as stream:
            stream.write(data)
        file.chmod(0o755 if name.startswith("bin/") else 0o644)
    if prepare_only:
        print(json.dumps({"component": "native-package", "target": target, "prepared": True,
                          "completed_build": False, "signing_required": True}), flush=True)
        return
    if intermediate:
        archive(entries, target, intermediate=True)
        return
    signing.sign(prepared, target)
    entries = {name: signing.read(prepared / name, signing.MAX_BYTES) for name in entries}
    entries["manifest.sig"] = signing.read(prepared / "manifest.sig", 1024)
    archive(entries, target)


def archive(entries: dict[str, bytes], target: str, *, intermediate: bool = False) -> None:
    if "manifest.sig" not in entries and not intermediate:
        raise RuntimeError("Unsigned final distribution is forbidden")
    if intermediate and os.environ.get("GITHUB_ACTIONS") != "true":
        raise RuntimeError("Internal compiler intermediates are hosted CI only")
    prefix = ("unsigned-intermediate-" if intermediate else "") + "numera-image-gen-mcp-0.2.0-" + target
    out = ROOT / (".ci-work/intermediate" if intermediate else ".ci-work/release")
    out.mkdir(parents=True, exist_ok=True)
    if "windows" in target:
        archive = out / (prefix + ".zip")
        with zipfile.ZipFile(archive, "x", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as bundle:
            for name, data in sorted(entries.items()):
                info = zipfile.ZipInfo(prefix + "/" + name, date_time=(1980, 1, 1, 0, 0, 0))
                info.external_attr = (0o755 if name.startswith("bin/") else 0o644) << 16
                info.compress_type = zipfile.ZIP_DEFLATED
                bundle.writestr(info, data)
    else:
        archive = out / (prefix + ".tar.gz")
        with archive.open("xb") as raw, gzip.GzipFile(fileobj=raw, mode="wb", mtime=0) as compressed, tarfile.open(fileobj=compressed, mode="w") as bundle:
            for name, data in sorted(entries.items()):
                info = tarfile.TarInfo(prefix + "/" + name)
                info.size = len(data)
                info.mode = 0o755 if name.startswith("bin/") else 0o644
                info.mtime = 0
                bundle.addfile(info, io.BytesIO(data))
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    (out / (archive.name + ".sha256")).write_text(f"{digest}  {archive.name}\n", encoding="utf-8")
    print(json.dumps({"component": "native-package", "archive": archive.name, "sha256": digest,
                      "target": target, "files": len(entries), "platform": platform.system()}), flush=True)


if __name__ == "__main__":
    main()
