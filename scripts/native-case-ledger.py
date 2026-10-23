"""Validate exact frozen case identities; acceptance additionally requires every assertion and run."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parent.parent
BASELINE = "fc7da25c3b470199949b1b4c67127d77afa0c34a"


def contained(root: Path, relative: str) -> Path:
    file = (root / relative).resolve()
    if not file.is_relative_to(root.resolve()) or not file.is_file():
        raise RuntimeError("Case ledger references a missing or escaping file")
    return file


def mapping_source(path: str, separator: str, symbol: str) -> str:
    sdk_entries = {
        "scripts/native-tools-acceptance.mjs": "toolsAcceptance",
        "scripts/native-two-host-acceptance.mjs": "twoHostAcceptance",
        "scripts/native-compact-acceptance.mjs": "compactAcceptance",
        "scripts/native-env-acceptance.mjs": "envAcceptance",
        "scripts/native-negotiation-acceptance.mjs": "negotiationAcceptance",
        "scripts/native-config-reload-acceptance.mjs": "configReloadAcceptance",
        "scripts/native-terminal-acceptance.mjs": "terminalAcceptance",
        "scripts/native-races-acceptance.mjs": "racesAcceptance",
        "scripts/native-lifecycle-acceptance.mjs": "lifecycleAcceptance",
        "scripts/native-transport-acceptance.mjs": "transportAcceptance",
    }
    sdk = path in sdk_entries and symbol == sdk_entries[path]
    if not separator or not (sdk or (path.startswith(("tests/rust/", "native/")) and path.endswith(".rs"))):
        raise RuntimeError("Native case mapping requires a Rust test or registered native SDK fixture")
    text = contained(ROOT, path).read_text(encoding="utf-8")
    if sdk:
        if f"export async function {symbol}(" not in text:
            raise RuntimeError("Mapped native SDK fixture is missing")
        caller = contained(ROOT, "scripts/native-interop.mjs").read_text(encoding="utf-8")
        imported = f'import {{ {symbol} }} from "./{Path(path).name}";'
        if imported not in caller or f"await {symbol}(" not in caller:
            raise RuntimeError("Native SDK fixture is not executed by interoperability acceptance")
        for entry in ["scripts/native-ci.py", "scripts/native-extract-check.py"]:
            if "scripts/native-interop.mjs" not in contained(ROOT, entry).read_text(encoding="utf-8"):
                raise RuntimeError("Native SDK fixture is missing hosted or extracted execution")
    elif not re.search(r"\bfn\s+" + re.escape(symbol) + r"\s*\(", text):
        raise RuntimeError("Mapped native test is missing")
    elif path.startswith("native/") and not re.search(r"#\[test\]\s*fn\s+" + re.escape(symbol) + r"\s*\(", text):
        raise RuntimeError("Library mapping must identify an actual unit test")
    return text


def validate(source: Path, collection: dict, ledger_root: Path, acceptance: bool) -> dict:
    index = json.loads((ledger_root / "index.json").read_text(encoding="utf-8"))
    if index["baseline"] != BASELINE or collection["baseline"] != BASELINE:
        raise RuntimeError("Case ledger baseline differs")
    if collection["source_cases"] != 470 or collection["source_suites"] != 50:
        raise RuntimeError("Expanded case collection is incomplete")
    expected = {row["id"]: row for row in collection["cases"]}
    if len(expected) != 470:
        raise RuntimeError("Expanded case identities are duplicated or missing")
    rows = []
    for relative in index["files"]:
        file = contained(ledger_root, relative)
        if file.suffix != ".jsonl":
            raise RuntimeError("Case ledger requires JSON Lines files")
        rows.extend(json.loads(line) for line in file.read_text(encoding="utf-8").splitlines())
    if len(rows) != 470 or {row["id"] for row in rows} != set(expected):
        raise RuntimeError("Every expanded source case must appear exactly once")
    hashes = {}
    source_lines = {}
    mapping_sources = {}
    native_hashes = {}
    pending = 0
    mapped = 0
    for row in rows:
        case = expected[row["id"]]
        for field in ["source", "source_sha256", "name", "location"]:
            if row[field] != case[field]:
                raise RuntimeError("Case identity differs from actual runtime collection")
        file = contained(source, row["source"])
        if row["source"] not in hashes:
            data = file.read_bytes()
            hashes[row["source"]] = hashlib.sha256(data).hexdigest()
            source_lines[row["source"]] = len(data.decode("utf-8").splitlines())
        if hashes[row["source"]] != row["source_sha256"]:
            raise RuntimeError("Frozen source assertion identity is stale")
        modes = row["platform_modes"]
        if set(modes) != {"win32", "linux", "darwin"} or modes[collection["platform"]] != case["mode"]:
            raise RuntimeError("Case platform applicability differs from collection")
        if any(mode not in {"run", "skip", "todo"} for mode in modes.values()):
            raise RuntimeError("Invalid source platform mode")
        native_files = {}
        for mapping in row["native"]:
            path, separator, symbol = mapping.partition("::")
            if mapping not in mapping_sources:
                mapping_sources[mapping] = mapping_source(path, separator, symbol)
            text = mapping_sources[mapping]
            native_files[path] = text
            if path not in native_hashes:
                native_hashes[path] = hashlib.sha256(contained(ROOT, path).read_bytes()).hexdigest()
        for assertion in row["assertions"]:
            text = native_files.get(assertion["native_file"])
            if text is None or not assertion.get("claim"):
                raise RuntimeError("Assertion must reference its mapped native test")
            for bounds, count in [(assertion["source_lines"], source_lines[row["source"]]),
                                  (assertion["native_lines"], len(text.splitlines()))]:
                if len(bounds) != 2 or any(type(n) is not int for n in bounds) or not 1 <= bounds[0] <= bounds[1] <= count:
                    raise RuntimeError("Assertion line evidence is invalid")
        if row["native"] and row["assertions"]:
            mapped += 1
        if row["status"] == "pending":
            pending += 1
        elif row["status"] != "verified":
            raise RuntimeError("Unsupported case disposition; silent retirement is forbidden")
        if row["status"] == "verified":
            if not row["native"] or not row["assertions"] or row.get("gaps") or not row.get("review"):
                raise RuntimeError("Verified case has incomplete assertion review")
            evidence = row.get("evidence")
            if not evidence or not evidence.get("native_sha256") or not evidence.get("passed_targets") or not evidence.get("run_id"):
                raise RuntimeError("Verified case has no exact native execution evidence")
            for path in native_files:
                if evidence["native_sha256"].get(path) != native_hashes[path]:
                    raise RuntimeError("Native assertion evidence is stale")
            if any(modes[platform] == "run" and platform not in evidence["passed_targets"] for platform in modes):
                raise RuntimeError("Applicable native case execution is missing")
    if len(hashes) != 50 or index["source_cases"] != 470 or index["source_suites"] != 50:
        raise RuntimeError("Case ledger suite/case totals differ")
    if acceptance and (pending or index["mapping_complete"] is not True):
        raise RuntimeError("Complete native assertion parity is still pending")
    return {"component": "native-case-ledger", "cases": len(rows), "suites": len(hashes),
            "assertion_assignments": mapped, "pending": pending, "acceptance_verified": acceptance and not pending}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--collection", type=Path, required=True)
    parser.add_argument("--acceptance", action="store_true")
    args = parser.parse_args()
    collection = json.loads(args.collection.read_text(encoding="utf-8"))
    print(json.dumps(validate(args.source.resolve(), collection, ROOT / "docs/testing/native-cases", args.acceptance)))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError, RuntimeError) as error:
        print(json.dumps({"level": "ERROR", "component": "native-case-ledger", "message": str(error)
                          if isinstance(error, RuntimeError) else "Case ledger validation failed."}), file=sys.stderr)
        raise SystemExit(1) from None
