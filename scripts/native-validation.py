"""Prepare maintained source validation without changing the immutable native parity oracle."""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path, PurePosixPath
import subprocess
import tarfile
import uuid

ROOT = Path(__file__).resolve().parent.parent
BASELINE = "fc7da25c3b470199949b1b4c67127d77afa0c34a"
ZOD = {"version": "4.6.7", "resolved": "https://registry.npmjs.org/zod/-/zod-4.6.7.tgz",
       "integrity": "sha512-g8W2D2kU6a8untc053fX1uVBKzM7yqAxW/3zwk83+6RyF2+SKOuV+c0wBDhUwzV1PaGapmZH+rmcUzPf+3CGwg=="}
LIMIT = 128 * 1024 * 1024


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def prepare(root: Path, archive: bytes) -> dict:
    destination = root / ".ci-work/validation"
    if destination.exists() or destination.is_symlink() or len(archive) > LIMIT:
        raise RuntimeError("Validation requires a fresh bounded project-contained destination")
    if not destination.parent.resolve().is_relative_to(root.resolve()):
        raise RuntimeError("Validation destination escapes project")
    files = {}
    total = 0
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:") as bundle:
        for item in bundle:
            if item.isdir():
                continue
            name = item.name
            path = PurePosixPath(name)
            if (not item.isfile() or not name or "\\" in name or ":" in name or path.is_absolute()
                    or ".." in path.parts or path.as_posix() != name or name in files
                    or len(files) >= 4096 or not 0 <= item.size <= LIMIT):
                raise RuntimeError("Invalid source archive member")
            total += item.size
            if total > LIMIT:
                raise RuntimeError("Source archive exceeds aggregate bound")
            with bundle.extractfile(item) as stream:
                data = stream.read(item.size + 1)
            if len(data) != item.size:
                raise RuntimeError("Incomplete source archive member")
            files[name] = data
    try:
        package = json.loads(files["package.json"].decode("utf-8"))
        lock = json.loads(files["package-lock.json"].decode("utf-8"))
        if (package["dependencies"]["zod"] != "4.6.5" or lock["lockfileVersion"] != 3
                or lock["packages"][""]["dependencies"]["zod"] != "4.6.5"
                or lock["packages"]["node_modules/zod"]["version"] != "4.6.5"):
            raise RuntimeError("Unexpected frozen dependency contract")
        original = {name: digest(data) for name, data in files.items()}
        package["dependencies"]["zod"] = ZOD["version"]
        lock["packages"][""]["dependencies"]["zod"] = ZOD["version"]
        lock["packages"]["node_modules/zod"].update(ZOD)
        for name, value in (("package.json", package), ("package-lock.json", lock)):
            files[name] = (json.dumps(value, ensure_ascii=False, indent=2) + "\n").encode("utf-8")
        supervisor = root / "scripts/bounded.mjs"
        if supervisor.is_symlink() or not supervisor.resolve().is_relative_to(root.resolve()):
            raise RuntimeError("Maintained supervisor escapes project")
        files["scripts/bounded.mjs"] = supervisor.read_bytes()
    except (KeyError, TypeError, ValueError) as error:
        raise RuntimeError("Invalid frozen validation source") from error
    expected = {name: digest(data) for name, data in files.items()}
    changes = {name: {"baseline": original[name], "validation": expected[name]}
               for name in files if original[name] != expected[name]}
    tests = {name: value for name, value in original.items() if name.startswith("tests/")}
    if set(changes) != {"package.json", "package-lock.json", "scripts/bounded.mjs"} or not tests:
        raise RuntimeError("Validation delta differs from reviewed allowlist")
    destination.mkdir(parents=True, mode=0o700)
    for name, data in files.items():
        file = destination / name
        file.parent.mkdir(parents=True, exist_ok=True)
        with file.open("xb") as stream:
            stream.write(data)
        if digest(file.read_bytes()) != expected[name]:
            raise RuntimeError("Validation source write verification failed")
    return {"schema_version": 1, "baseline": BASELINE, "source_archive_sha256": digest(archive),
            "changes": changes, "unchanged_tests": tests, "validation_files": expected,
            "immutable_oracle_modified": False}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("action", choices=("prepare", "check"))
    args = parser.parse_args()
    if os.environ.get("GITHUB_ACTIONS") != "true":
        raise RuntimeError("Source validation execution is hosted CI only")
    spec = importlib.util.spec_from_file_location("validation_runner", ROOT / "scripts/native-ci.py")
    runner = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(runner)
    work = ROOT / ".ci-work/native"
    work.mkdir(parents=True, exist_ok=True)
    stamp = dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%d_%H-%M-%S")
    with (work / f"native-validation_{stamp}_UTC-{uuid.uuid4()}.log").open("x", encoding="utf-8") as log:
        runner.LOG = log
        env = os.environ.copy()
        runner.emit("INFO", "Maintained source validation started.", action=args.action)
        if args.action == "prepare":
            archive = runner.run(["git", "-C", str(ROOT / ".ci-work/baseline"), "archive", "--format=tar", BASELINE],
                                 wall=15, idle=8, env=env, capture=True, capture_limit=LIMIT)
            receipt = prepare(ROOT, archive)
            receipt["source_sha"] = os.environ["GITHUB_SHA"]
            with (work / "validation-source.json").open("x", encoding="utf-8") as stream:
                json.dump(receipt, stream, ensure_ascii=False, indent=2)
                stream.write("\n")
            runner.emit("INFO", "Separate validation source prepared; immutable oracle unchanged.",
                        changed_files=len(receipt["changes"]), unchanged_tests=len(receipt["unchanged_tests"]),
                        zod=ZOD["version"])
        else:
            receipt = json.loads((work / "validation-source.json").read_text(encoding="utf-8"))
            if receipt["baseline"] != BASELINE or receipt["source_sha"] != os.environ["GITHUB_SHA"]:
                raise RuntimeError("Validation evidence identity mismatch")
            for name, expected in receipt["validation_files"].items():
                if digest((ROOT / ".ci-work/validation" / name).read_bytes()) != expected:
                    raise RuntimeError("Validation source changed after preparation")
            control = ("const a=require('node:assert/strict'),z=require('zod');"
                       "a.equal(require('zod/package.json').version,'4.6.7');let parsed=0;"
                       "const r=z.array(z.string().refine(()=>{parsed++;return true})).max(2).safeParse(['a','b','c']);"
                       "a.equal(r.success,false);a.equal(parsed,0);a.equal(r.error.issues.length,1);"
                       "a.equal(r.error.issues[0].code,'too_big');console.log('patched_collection_bound_verified=true');")
            runner.run(["node", "-e", control], wall=5, idle=3, env=env, cwd=ROOT / ".ci-work/validation")
            runner.emit("INFO", "Validation source identity and patched collection bound verified.")
        runner.emit("INFO", "Maintained source validation completed.", action=args.action)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, RuntimeError, tarfile.TarError, subprocess.SubprocessError) as error:
        print(json.dumps({"level": "ERROR", "component": "native-validation",
                          "message": "Maintained source validation failed.", "error_type": type(error).__name__}), flush=True)
        raise SystemExit(1) from None
