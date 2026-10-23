"""Small deterministic validation-tree seam; never modify the immutable parity oracle."""
from __future__ import annotations

import copy
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


def load():
    file = ROOT / "scripts/native-validation.py"
    assert file.is_file(), "Separate maintained validation preparation is missing"
    spec = importlib.util.spec_from_file_location("validation_contract", file)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def archive(files):
    data = io.BytesIO()
    with tarfile.open(fileobj=data, mode="w") as bundle:
        for name, payload in files.items():
            item = tarfile.TarInfo(name)
            item.size = len(payload)
            bundle.addfile(item, io.BytesIO(payload))
    return data.getvalue()


class ValidationTree(unittest.TestCase):
    def test_only_reviewed_dependencies_and_supervisor_change_in_separate_tree(self):
        validation = load()
        package = {"private": True, "dependencies": {"zod": "4.6.5", "unchanged": "1.0.0"}}
        lock = {"lockfileVersion": 3, "packages": {
            "": copy.deepcopy(package),
            "node_modules/zod": {"version": "4.6.5", "resolved": "https://registry.npmjs.org/zod/-/zod-4.6.5.tgz",
                                 "integrity": "original", "license": "MIT"},
            "node_modules/unchanged": {"version": "1.0.0", "integrity": "same"},
        }}
        files = {"package.json": json.dumps(package).encode("utf-8"),
                 "package-lock.json": json.dumps(lock).encode("utf-8"),
                 "scripts/bounded.mjs": b"original supervisor\n",
                 "tests/exact.test.ts": 'const label = "تصویر";\n'.encode("utf-8"),
                 "src/exact.ts": b"export const fixture = 1;\n"}
        directory = ROOT / ".ci-work/native/tooling-tests"
        directory.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="validation ", dir=directory) as temp:
            root = Path(temp)
            (root / "scripts").mkdir()
            (root / "scripts/bounded.mjs").write_bytes(b"maintained supervisor\n")
            original = root / ".ci-work/baseline"
            original.mkdir(parents=True)
            for name, data in files.items():
                file = original / name
                file.parent.mkdir(parents=True, exist_ok=True)
                file.write_bytes(data)
            receipt = validation.prepare(root, archive(files))
            output = root / ".ci-work/validation"
            self.assertEqual({name: (original / name).read_bytes() for name in files}, files)
            self.assertEqual((output / "tests/exact.test.ts").read_bytes(), files["tests/exact.test.ts"])
            self.assertEqual((output / "src/exact.ts").read_bytes(), files["src/exact.ts"])
            expected = copy.deepcopy(lock)
            expected["packages"][""]["dependencies"]["zod"] = "4.6.7"
            expected["packages"]["node_modules/zod"].update(validation.ZOD)
            self.assertEqual(json.loads((output / "package-lock.json").read_text(encoding="utf-8")), expected)
            self.assertEqual(json.loads((output / "package.json").read_text(encoding="utf-8")),
                             {**package, "dependencies": {**package["dependencies"], "zod": "4.6.7"}})
            self.assertEqual((output / "scripts/bounded.mjs").read_bytes(), b"maintained supervisor\n")
            self.assertEqual(set(receipt["changes"]), {"package.json", "package-lock.json", "scripts/bounded.mjs"})
            self.assertEqual(receipt["unchanged_tests"]["tests/exact.test.ts"], hashlib.sha256(files["tests/exact.test.ts"]).hexdigest())
            with self.assertRaises(RuntimeError):
                validation.prepare(root, archive(files))
            for label, changed in [("traversal", {"../outside": b"bad"}),
                                   ("duplicate", {"src/exact.ts": b"bad"}),
                                   ("unexpected-version", {"package.json": b'{"dependencies":{"zod":"4.6.6"}}'})]:
                with self.subTest(label=label):
                    isolated = root / label
                    (isolated / "scripts").mkdir(parents=True)
                    (isolated / "scripts/bounded.mjs").write_bytes(b"maintained supervisor\n")
                    data = archive({**files, **changed})
                    if label == "duplicate":
                        data = archive({**files, "./src/exact.ts": b"bad"})
                    with self.assertRaises(RuntimeError):
                        validation.prepare(isolated, data)
                    self.assertFalse((isolated / ".ci-work/validation").exists())
                    self.assertFalse((isolated / ".ci-work/outside").exists())


if __name__ == "__main__":
    unittest.main()
