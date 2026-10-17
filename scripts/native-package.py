"""Allowlisted native distribution archives; original executable built only in hosted CI."""
from __future__ import annotations

import argparse
import gzip
import hashlib
import io
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
    args = parser.parse_args()
    if os.environ.get("GITHUB_ACTIONS") != "true":
        raise RuntimeError("Release packaging is hosted CI only")
    if args.target not in ["x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc", "x86_64-unknown-linux-gnu",
                           "aarch64-unknown-linux-gnu", "x86_64-apple-darwin", "aarch64-apple-darwin"]:
        raise RuntimeError("Unknown native target")
    executable = "numera-image-gen-mcp.exe" if "windows" in args.target else "numera-image-gen-mcp"
    source = ROOT / "target/release" / executable
    if not source.is_file():
        raise RuntimeError("Verified native executable is missing")
    prefix = "numera-image-gen-mcp-0.2.0-" + args.target
    entries = {"bin/" + executable: source.read_bytes()}
    for name in ["LICENSE", "THIRD_PARTY_NOTICES.md", "README.md", ".env.example",
                 "schemas/config.schema.json", "schemas/simple-config.schema.json", "schemas/image-request.schema.json",
                 "examples/claude-desktop.json", "examples/claude-project.json", "examples/codex.toml",
                 "docs/CONFIGURATION.md", "docs/CLAUDE.md", "docs/CODEX.md", "docs/ARCHITECTURE.md", "docs/HTTP.md",
                 "docs/WINDOWS-WSL.md", "docs/COMPATIBILITY.md", "docs/IMAGE-EDITING.md", "docs/LINUX-MACOS.md",
                 "docs/REQUIREMENTS.md", "docs/REFERENCES.md", "SECURITY.md", "docs/TESTING.md",
                 "docs/PRACTICAL-TESTING.md", "docs/TEST-RESULTS.md", "docs/testing/MIGRATION-MATRIX.md",
                 "docs/testing/native-parity.json", "docs/testing/native-cases/index.json",
                 "docs/testing/native-cases/unit.jsonl", "docs/testing/native-cases/contract.jsonl", "docs/testing/native-cases/integration.jsonl",
                 "docs/assets/logo.png", "docs/licenses/supplements.json", "docs/licenses/alloc-stdlib-0.3.0/LICENSE", "docs/licenses/rmcp-3.5.1/LICENSE"]:
        entries[name] = (ROOT / name).read_bytes()
    notices = ROOT / ".ci-work/native/licenses"
    if not notices.is_dir() or not any(notices.iterdir()):
        raise RuntimeError("Resolved native dependency license inventory is required before packaging")
    for file in sorted(notices.rglob("*")):
        if file.is_file() and not file.is_symlink():
            entries["licenses/" + file.relative_to(notices).as_posix()] = file.read_bytes()
    manifest = {"version": "0.2.0", "target": args.target, "source_sha": os.environ.get("GITHUB_SHA"),
                "binary_sha256": hashlib.sha256(entries["bin/" + executable]).hexdigest(),
                "files": [{"path": name, "sha256": hashlib.sha256(data).hexdigest(), "bytes": len(data)} for name, data in sorted(entries.items())]}
    entries["manifest.json"] = (json.dumps(manifest, indent=2) + "\n").encode("utf-8")
    out = ROOT / ".ci-work/release"
    out.mkdir(parents=True, exist_ok=True)
    if "windows" in args.target:
        archive = out / (prefix + ".zip")
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as bundle:
            for name, data in sorted(entries.items()):
                info = zipfile.ZipInfo(prefix + "/" + name, date_time=(1980, 1, 1, 0, 0, 0))
                info.external_attr = (0o755 if name.startswith("bin/") else 0o644) << 16
                info.compress_type = zipfile.ZIP_DEFLATED
                bundle.writestr(info, data)
    else:
        archive = out / (prefix + ".tar.gz")
        with archive.open("wb") as raw, gzip.GzipFile(fileobj=raw, mode="wb", mtime=0) as compressed, tarfile.open(fileobj=compressed, mode="w") as bundle:
            for name, data in sorted(entries.items()):
                info = tarfile.TarInfo(prefix + "/" + name)
                info.size = len(data)
                info.mode = 0o755 if name.startswith("bin/") else 0o644
                info.mtime = 0
                bundle.addfile(info, io.BytesIO(data))
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    (out / (archive.name + ".sha256")).write_text(f"{digest}  {archive.name}\n", encoding="utf-8")
    print(json.dumps({"component": "native-package", "archive": archive.name, "sha256": digest,
                      "target": args.target, "files": len(entries), "platform": platform.system()}), flush=True)


if __name__ == "__main__":
    main()
