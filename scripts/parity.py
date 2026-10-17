"""Fail-closed source-suite mapping checks; no claim that source tests prove native behavior."""
from __future__ import annotations

import argparse
import importlib.util
import hashlib
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parent.parent


def inventory(source: Path) -> list[dict]:
    result = []
    for file in sorted((source / "tests").rglob("*.test.ts")):
        data = file.read_bytes()
        result.append({"source": file.relative_to(source).as_posix(), "sha256": hashlib.sha256(data).hexdigest(),
                       "native": [], "status": "pending", "evidence": None})
    if len(result) != 50:
        raise RuntimeError("Pinned baseline suite count differs from expected 50")
    return result


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, default=ROOT / "docs/testing/native-parity.json")
    parser.add_argument("--inventory", action="store_true")
    parser.add_argument("--collection", type=Path, default=ROOT / ".ci-work/parity/source-cases.json")
    args = parser.parse_args()
    source = args.source.resolve()
    expected = inventory(source)
    if args.inventory:
        output = ROOT / ".ci-work/parity/source-suites.json"
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps({"baseline": "fc7da25c3b470199949b1b4c67127d77afa0c34a", "baseline_case_count": 470, "suites": expected}, indent=2) + "\n", encoding="utf-8")
        print(json.dumps({"component": "parity", "discovered_source_suites": len(expected), "acceptance_verified": False}))
        return
    spec = importlib.util.spec_from_file_location("native_case_ledger", ROOT / "scripts/native-case-ledger.py")
    ledger = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(ledger)
    ledger.validate(source, json.loads(args.collection.read_text(encoding="utf-8")),
                    ROOT / "docs/testing/native-cases", True)
    actual = json.loads(args.manifest.read_text(encoding="utf-8"))
    rows = actual["suites"]
    if len(rows) != len(expected) or {r["source"] for r in rows} != {r["source"] for r in expected}:
        raise RuntimeError("Every baseline suite must have an explicit mapping")
    hashes = {r["source"]: r["sha256"] for r in expected}
    for row in rows:
        if row["sha256"] != hashes[row["source"]]:
            raise RuntimeError("Baseline source mapping is stale")
        if row["status"] != "verified" or not row["native"] or not row["evidence"]:
            raise RuntimeError("Native migration has pending or unverified source contracts")
        for mapping in row["native"]:
            if isinstance(mapping, str):
                path, separator, test = mapping.partition("::")
                if not separator or not path or not test:
                    raise RuntimeError("Native mapping must contain a file and test identity")
            else:
                path, test = mapping["file"], mapping["test"]
            ledger.mapping_source(path, "::", test)
    if actual["baseline_case_count"] != 470 or actual.get("case_mapping_complete") is not True:
        raise RuntimeError("All 470 baseline cases must be mapped; suite grouping alone is insufficient")
    print(json.dumps({"component": "parity", "source_suites": len(rows), "source_cases": 470, "acceptance_verified": True}))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, RuntimeError):
        print(json.dumps({"level": "ERROR", "component": "parity", "message": "Native migration acceptance mapping failed."}), file=sys.stderr)
        raise SystemExit(1) from None
