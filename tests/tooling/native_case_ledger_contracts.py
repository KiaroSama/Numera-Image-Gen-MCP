"""Structural ledger checks use real frozen source bytes, never claim native case parity."""
from __future__ import annotations

import copy
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("native_case_ledger", ROOT / "scripts/native-case-ledger.py")
ledger = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ledger)


class CaseLedger(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.source = Path(os.environ["NUMERA_ORACLE_ROOT"])
        cls.directory = ROOT / "docs/testing/native-cases"
        cls.index = json.loads((cls.directory / "index.json").read_text(encoding="utf-8"))
        cls.rows = [json.loads(line) for name in cls.index["files"]
                    for line in (cls.directory / name).read_text(encoding="utf-8").splitlines()]
        # Validator-only input assembled from the ledger; real runtime collection is a separate CI gate.
        cls.collection = {"baseline": ledger.BASELINE, "source_cases": 470, "source_suites": 50,
                          "platform": "linux", "cases": [{**{key: row[key] for key in ["id", "source", "source_sha256", "name", "location"]},
                                                         "mode": row["platform_modes"]["linux"]} for row in cls.rows]}

    def test_current_ledger_structure_and_acceptance_match_explicit_dispositions(self):
        result = ledger.validate(self.source, self.collection, self.directory, False)
        self.assertEqual(result["cases"], 470)
        self.assertFalse(result["acceptance_verified"])
        complete = self.index["mapping_complete"] is True and all(row["status"] == "verified" for row in self.rows)
        if complete:
            accepted = ledger.validate(self.source, self.collection, self.directory, True)
            self.assertEqual(accepted["pending"], 0)
            self.assertTrue(accepted["acceptance_verified"])
        else:
            with self.assertRaisesRegex(RuntimeError, "pending"):
                ledger.validate(self.source, self.collection, self.directory, True)

    def test_mapping_cache_is_per_validation_and_never_hides_changed_file_bytes(self):
        # Two references share a mapping, but only inside one immutable validation pass.
        with patch.object(ledger, "mapping_source", wraps=ledger.mapping_source) as mapped:
            ledger.validate(self.source, self.collection, self.directory, False)
            calls = mapped.call_count
            self.assertEqual(calls, len({mapping for row in self.rows for mapping in row["native"]}))
        verified = next(row for row in self.rows if row["status"] == "verified")
        path = verified["native"][0].partition("::")[0]
        original = Path.read_bytes

        def changed(file):
            data = original(file)
            return data + b"\n" if file.resolve() == (ROOT / path).resolve() else data

        with patch.object(Path, "read_bytes", changed), self.assertRaisesRegex(RuntimeError, "stale"):
            ledger.validate(self.source, self.collection, self.directory, False)

    def test_duplicate_missing_and_changed_identities_are_rejected(self):
        for mutation in ["duplicate", "missing", "name", "mode"]:
            with self.subTest(mutation=mutation):
                changed = copy.deepcopy(self.collection)
                if mutation == "duplicate":
                    changed["cases"][-1] = changed["cases"][0]
                elif mutation == "missing":
                    changed["cases"].pop()
                elif mutation == "name":
                    changed["cases"][0]["name"] = "changed assertion identity"
                else:
                    changed["cases"][0]["mode"] = "todo"
                with self.assertRaises(RuntimeError):
                    ledger.validate(self.source, changed, self.directory, False)

    def test_only_registered_native_sdk_fixture_is_mappable(self):
        text = ledger.mapping_source("scripts/native-tools-acceptance.mjs", "::", "toolsAcceptance")
        self.assertIn("host.client.callTool", text)
        self.assertIn("host.client.readResource", text)
        for path, symbol in [
            ("scripts/native-two-host-acceptance.mjs", "twoHostAcceptance"),
            ("scripts/native-compact-acceptance.mjs", "compactAcceptance"),
            ("scripts/native-env-acceptance.mjs", "envAcceptance"),
            ("scripts/native-negotiation-acceptance.mjs", "negotiationAcceptance"),
            ("scripts/native-config-reload-acceptance.mjs", "configReloadAcceptance"),
            ("scripts/native-terminal-acceptance.mjs", "terminalAcceptance"),
            ("scripts/native-races-acceptance.mjs", "racesAcceptance"),
            ("scripts/native-lifecycle-acceptance.mjs", "lifecycleAcceptance"),
            ("scripts/native-transport-acceptance.mjs", "transportAcceptance"),
        ]:
            with self.subTest(path=path):
                self.assertIn(f"export async function {symbol}(", ledger.mapping_source(path, "::", symbol))
        for path, separator, symbol in [
            ("scripts/native-tools-acceptance.mjs", "::", "missing"),
            ("scripts/native-compact-acceptance.mjs", "::", "twoHostAcceptance"),
            ("scripts/native-interop.mjs", "::", "tool"),
            ("tests/integration/tools.test.ts", "::", "toolsAcceptance"),
            ("scripts/native-tools-acceptance.mjs", "", "toolsAcceptance"),
        ]:
            with self.subTest(path=path, symbol=symbol), self.assertRaises(RuntimeError):
                ledger.mapping_source(path, separator, symbol)

    def test_complete_boolean_cannot_bypass_pending_assertions(self):
        directory = ROOT / ".ci-work/native/tooling-tests"
        directory.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="ledger ", dir=directory) as temp:
            root = Path(temp)
            index = {**self.index, "mapping_complete": True}
            (root / "index.json").write_text(json.dumps(index), encoding="utf-8")
            pending_written = False
            for name in self.index["files"]:
                rows = [json.loads(line) for line in (self.directory / name).read_text(encoding="utf-8").splitlines()]
                if not pending_written:
                    rows[0]["status"] = "pending"
                    pending_written = True
                (root / name).write_text("".join(json.dumps(row) + "\n" for row in rows), encoding="utf-8")
            with self.assertRaisesRegex(RuntimeError, "pending"):
                ledger.validate(self.source, self.collection, root, True)


if __name__ == "__main__":
    unittest.main()
