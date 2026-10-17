"""Preserve actual locked runtime dependency notices from the hosted Cargo registry."""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import importlib.util
import re
import sys
import tomllib

ROOT = Path(__file__).resolve().parent.parent


class NoticeError(RuntimeError):
    def __init__(self, code: str, package: str):
        super().__init__(code)
        self.code = code
        self.package = package if re.fullmatch(r"[A-Za-z0-9_.+@-]{1,160}", package) else "unknown"


def notices(package: dict, output: Path, locked: dict, supplements: dict) -> dict:
    identity = package["name"] + "@" + package["version"]
    directory = Path(package["manifest_path"]).parent.resolve()
    if not directory.is_relative_to((ROOT / ".ci-cache/cargo").resolve()):
        raise NoticeError("registry_path", identity)
    if not package.get("license") and not package.get("license_file"):
        raise NoticeError("license_declaration", identity)
    candidates = set()
    for file in directory.rglob("*"):
        if file.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "COPYRIGHT", "NOTICE", "UNLICENSE")):
            if file.is_symlink() or not file.resolve().is_relative_to(directory):
                raise NoticeError("notice_path", identity)
            if file.is_file():
                candidates.add(file)
    if package.get("license_file"):
        file = directory / package["license_file"]
        if file.is_symlink() or not file.resolve().is_relative_to(directory) or not file.is_file():
            raise NoticeError("declared_notice_path", identity)
        candidates.add(file)
    names = []
    hashes = {}
    target_root = output / (package["name"] + "-" + package["version"])
    for file in sorted(candidates):
        if file.stat().st_size > 1024 * 1024:
            raise NoticeError("notice_size", identity)
        data = file.read_bytes()
        relative = file.relative_to(directory)
        target = target_root / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
        names.append(relative.as_posix())
        hashes[relative.as_posix()] = hashlib.sha256(data).hexdigest()
    provenance = None
    if not names:
        supplement = supplements.get(identity)
        if supplement is None:
            raise NoticeError("missing_notice", identity)
        lock = locked.get(identity)
        vcs = directory / ".cargo_vcs_info.json"
        if lock is None or not vcs.is_file() or vcs.is_symlink():
            raise NoticeError("supplement_source", identity)
        source = json.loads(vcs.read_text(encoding="utf-8"))
        if (lock.get("checksum") != supplement["crate_sha256"] or
                source.get("git", {}).get("sha1") != supplement["source_revision"] or
                package.get("license") != supplement["license"]):
            raise NoticeError("supplement_identity", identity)
        file = ROOT / supplement["file"]
        if file.is_symlink() or not file.resolve().is_relative_to((ROOT / "docs/licenses").resolve()):
            raise NoticeError("supplement_path", identity)
        data = file.read_bytes()
        if not data or len(data) > 1024 * 1024 or hashlib.sha256(data).hexdigest() != supplement["sha256"]:
            raise NoticeError("supplement_hash", identity)
        target_root.mkdir(parents=True, exist_ok=True)
        (target_root / "LICENSE").write_bytes(data)
        names = ["LICENSE"]
        hashes = {"LICENSE": supplement["sha256"]}
        provenance = {key: supplement[key] for key in ["crate_sha256", "source_revision", "source_url"]}
    return {"name": package["name"], "version": package["version"], "license": package.get("license"),
            "repository": package.get("repository"), "notices": names, "notice_sha256": hashes,
            **({"supplement": provenance} if provenance else {})}


def main() -> None:
    if os.environ.get("GITHUB_ACTIONS") != "true":
        raise RuntimeError("License resolution is hosted CI only")
    command = ["cargo", "metadata", "--locked", "--format-version", "1", "--filter-platform", os.environ["NUMERA_NATIVE_TARGET"]]
    spec = importlib.util.spec_from_file_location("native_ci", ROOT / "scripts/native-ci.py")
    runner = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(runner)
    response = runner.run(command, cwd=ROOT, env=runner.environment(), wall=120, idle=45,
                          capture=True, capture_limit=16 * 1024 * 1024)
    metadata = json.loads(response.decode("utf-8"))
    packages = {item["id"]: item for item in metadata["packages"]}
    nodes = {item["id"]: item for item in metadata["resolve"]["nodes"]}
    root = metadata["resolve"]["root"]
    pending = [root]
    selected = set()
    while pending:
        item = pending.pop()
        if item in selected:
            continue
        selected.add(item)
        for dependency in nodes[item]["deps"]:
            if any(kind["kind"] in [None, "build"] for kind in dependency["dep_kinds"]):
                pending.append(dependency["pkg"])
    output = ROOT / ".ci-work/native/licenses"
    output.mkdir(parents=True, exist_ok=True)
    locked = {item["name"] + "@" + item["version"]: item
              for item in tomllib.loads((ROOT / "Cargo.lock").read_text(encoding="utf-8"))["package"]}
    supplements = json.loads((ROOT / "docs/licenses/supplements.json").read_text(encoding="utf-8"))
    inventory = []
    failures = []
    for identity in sorted(selected - {root}):
        try:
            inventory.append(notices(packages[identity], output, locked, supplements))
        except NoticeError as error:
            failures.append({"code": error.code, "package": error.package})
            runner.emit("ERROR", "Resolved dependency notice failed verification.",
                        code=error.code, package=error.package)
    if failures:
        (output / "failures.json").write_text(json.dumps(failures, indent=2) + "\n", encoding="utf-8")
        raise RuntimeError("Resolved dependency notices remain unverified")
    (output / "inventory.json").write_text(json.dumps(inventory, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"component": "native-licenses", "resolved_packages": len(inventory)}))


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(json.dumps({"level": "ERROR", "component": "native-licenses",
                          "message": "Resolved native dependency notices could not be verified.",
                          "error_type": type(error).__name__}), file=sys.stderr)
        raise SystemExit(1) from None
