"""Offline contracts for hosted notice preservation and bounded official downloads."""
from __future__ import annotations

import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import urllib.error

ROOT = Path(__file__).resolve().parents[2]


def load(name: str, file: str):
    spec = importlib.util.spec_from_file_location(name, ROOT / "scripts" / file)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


notices = load("notice_contract", "native-licenses.py")
setup = load("setup_contract", "native-setup.py")


class Response:
    def __init__(self, chunks, length=None):
        self.chunks = iter(chunks)
        self.headers = {} if length is None else {"Content-Length": str(length)}
        self.closed = False

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.closed = True

    def read1(self, size):
        chunk = next(self.chunks, b"")
        if isinstance(chunk, Exception):
            raise chunk
        if len(chunk) > size:
            raise AssertionError("Fixture exceeds requested read bound")
        return chunk


class NativeTooling(unittest.TestCase):
    def setUp(self):
        directory = ROOT / ".ci-work/native/tooling-tests"
        directory.mkdir(parents=True, exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(prefix="notice ", dir=directory)
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.package_root = self.root / ".ci-cache/cargo/registry/src/fixture-1.0.0"
        self.package_root.mkdir(parents=True)
        self.package = {"name": "fixture", "version": "1.0.0", "manifest_path": str(self.package_root / "Cargo.toml"),
                        "license": "MIT", "license_file": None, "repository": "https://example.invalid/fixture"}
        self.output = self.root / "notices"
        root = patch.object(notices, "ROOT", self.root)
        root.start()
        self.addCleanup(root.stop)

    def test_actual_and_declared_nonstandard_notices_preserve_exact_bytes_and_hashes(self):
        expected = b"Original copyright and conditions.\n"
        (self.package_root / "LICENSE-MIT").write_bytes(expected)
        (self.package_root / "terms.txt").write_bytes(expected + b"extra\n")
        self.package["license_file"] = "terms.txt"
        result = notices.notices(self.package, self.output, {}, {})
        self.assertEqual(result["notices"], ["LICENSE-MIT", "terms.txt"])
        for name in result["notices"]:
            original = (self.package_root / name).read_bytes()
            self.assertEqual((self.output / "fixture-1.0.0" / name).read_bytes(), original)
            self.assertEqual(result["notice_sha256"][name], hashlib.sha256(original).hexdigest())

    def test_unknown_missing_notice_and_escaping_declared_file_fail_closed(self):
        with self.assertRaises(notices.NoticeError) as error:
            notices.notices(self.package, self.output, {}, {})
        self.assertEqual(error.exception.code, "missing_notice")
        self.assertEqual(error.exception.package, "fixture@1.0.0")
        (self.root / "outside.txt").write_text("not a package notice", encoding="utf-8")
        self.package["license_file"] = "../../../../outside.txt"
        with self.assertRaises(notices.NoticeError) as error:
            notices.notices(self.package, self.output, {}, {})
        self.assertEqual(error.exception.code, "declared_notice_path")

    def test_reviewed_supplement_requires_exact_crate_revision_license_and_notice_hash(self):
        supplements = json.loads((ROOT / "docs/licenses/supplements.json").read_text(encoding="utf-8"))
        self.assertEqual(set(supplements), {"alloc-stdlib@0.3.0", "rmcp@3.5.1"})
        for identity, supplement in supplements.items():
            with self.subTest(package=identity):
                original = (ROOT / supplement["file"]).read_bytes()
                file = self.root / supplement["file"]
                file.parent.mkdir(parents=True)
                file.write_bytes(original)
                name, version = identity.split("@")
                self.package.update(name=name, version=version, license=supplement["license"])
                vcs = self.package_root / ".cargo_vcs_info.json"
                vcs.write_text(json.dumps({"git": {"sha1": supplement["source_revision"]}}), encoding="utf-8")
                lock = {identity: {"checksum": supplement["crate_sha256"]}}
                result = notices.notices(self.package, self.output, lock, {identity: supplement})
                self.assertEqual((self.output / (name + "-" + version) / "LICENSE").read_bytes(), original)
                self.assertEqual(result["supplement"]["source_revision"], supplement["source_revision"])
                self.assertEqual(result["notice_sha256"]["LICENSE"], hashlib.sha256(original).hexdigest())
                for field in ["crate_sha256", "source_revision", "license", "sha256"]:
                    with self.subTest(field=field):
                        changed = {**supplement, field: "changed"}
                        with self.assertRaises(notices.NoticeError) as error:
                            notices.notices(self.package, self.output, lock, {identity: changed})
                        self.assertEqual(error.exception.code, "supplement_hash" if field == "sha256" else "supplement_identity")

    def test_timeout_retry_discards_partial_bytes_and_keeps_idle_bound(self):
        first = Response([b"partial", TimeoutError()], 20)
        second = Response([b"verified", b""], 8)
        with patch.object(setup.urllib.request, "urlopen", side_effect=[first, second]) as request, patch.object(setup, "emit"):
            self.assertEqual(setup.fetch("https://example.invalid/tool", 32), b"verified")
        self.assertEqual(request.call_count, 2)
        self.assertTrue(first.closed and second.closed)
        self.assertTrue(all(0 < call.kwargs["timeout"] <= 30 for call in request.call_args_list))

    def test_download_refuses_oversize_incomplete_permanent_error_and_exhausted_attempts(self):
        cases = [Response([], 33), Response([b"x" * 33, b""]), Response([b"short", b""], 8),
                 urllib.error.HTTPError("https://example.invalid/tool", 404, "missing", {}, None)]
        for response in cases:
            with self.subTest(response=type(response).__name__), patch.object(setup, "emit"), patch.object(setup.urllib.request, "urlopen") as request:
                if isinstance(response, Exception):
                    request.side_effect = response
                else:
                    request.return_value = response
                with self.assertRaises(RuntimeError):
                    setup.fetch("https://example.invalid/tool", 32)
                self.assertEqual(request.call_count, 1)
        with patch.object(setup, "emit"), patch.object(setup.urllib.request, "urlopen", side_effect=TimeoutError()) as request:
            with self.assertRaises(RuntimeError):
                setup.fetch("https://example.invalid/tool", 32)
            self.assertEqual(request.call_count, 3)
        with patch.object(setup, "emit"), patch.object(setup.time, "monotonic", side_effect=[0, 91]), patch.object(setup.urllib.request, "urlopen") as request:
            with self.assertRaises(RuntimeError):
                setup.fetch("https://example.invalid/tool", 32)
            request.assert_not_called()


class ClientExamples(unittest.TestCase):
    def test_examples_launch_only_native_bridge_and_keep_credentials_out_of_host_config(self):
        import tomllib
        desktop = json.loads((ROOT / "examples/claude-desktop.json").read_text(encoding="utf-8"))
        project = json.loads((ROOT / "examples/claude-project.json").read_text(encoding="utf-8"))
        codex = tomllib.loads((ROOT / "examples/codex.toml").read_text(encoding="utf-8"))
        self.assertEqual(set(desktop), {"mcpServers"})
        self.assertEqual(set(project), {"mcpServers"})
        self.assertEqual(set(codex), {"mcp_servers"})
        for value in (desktop["mcpServers"], project["mcpServers"], codex["mcp_servers"]):
            self.assertEqual(set(value), {"numera-image-gen"})
            entry = value["numera-image-gen"]
            self.assertEqual(entry["args"], ["bridge"])
            self.assertEqual(entry["command"], "C:/tools/numera-image-gen-mcp/bin/numera-image-gen-mcp.exe")
            self.assertEqual(entry["env"], {"NUMERA_CONFIG": "C:/tools/numera-image-gen-mcp/.env"})
        self.assertNotIn("type", desktop["mcpServers"]["numera-image-gen"])
        self.assertEqual(project["mcpServers"]["numera-image-gen"]["type"], "stdio")
        self.assertEqual(codex["mcp_servers"]["numera-image-gen"]["startup_timeout_sec"], 10)
        self.assertEqual(codex["mcp_servers"]["numera-image-gen"]["tool_timeout_sec"], 360)


class OracleEvidence(unittest.TestCase):
    def test_exact_allowlisted_inputs_bind_source_and_reject_tampering_without_private_files(self):
        self.assertTrue((ROOT / "scripts/native-oracle-evidence.py").is_file(), "Hosted oracle evidence capture is missing")
        evidence = load("oracle_evidence_contract", "native-oracle-evidence.py")
        self.assertEqual(set(evidence.INPUTS), {
            "config.json", "policy.json", "normalization-acceptance.json", "canonical.json",
            "adapters.json", "base64.json", "image-acceptance.json", "images.json", "animated-webp.json",
            "output-requirements.json", "stream-acceptance.json", "streams.json", "raw-json.json",
            "raw-js-schema.json", "sqlite-utf16-text.json",
        }, "Every reviewed synthetic native oracle consumer must have retained input bytes")
        import zipfile
        directory = ROOT / ".ci-work/native/tooling-tests"
        directory.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="oracle evidence ", dir=directory) as temp:
            root = Path(temp)
            baseline = root / "baseline"
            files = {
                "Cargo.toml": "fixture", "Cargo.lock": "locked", "rust-toolchain.toml": "stable",
                "native/main.rs": "fn main() {}", "tests/rust/common/mod.rs": "fixture",
                "schemas/config.schema.json": '{"type":"object"}',
                "scripts/rust-oracle.mjs": "fixture", "scripts/rust-normalization-oracle.mjs": "fixture",
                "scripts/native-ci.py": "fixture runner",
                "baseline/src/config/schema.ts": "fixture source", "baseline/dist/config/schema.js": "fixture compiled",
                "baseline/package.json": '{"version":"1"}', "baseline/package-lock.json": '{"lockfileVersion":3}',
                "baseline/node_modules/sharp/package.json": '{"version":"0.35.5"}',
                "baseline/node_modules/zod/package.json": '{"version":"4.6.5"}',
            }
            for name, text in files.items():
                file = root / name
                file.parent.mkdir(parents=True, exist_ok=True)
                file.write_text(text, encoding="utf-8")
            inputs = root / ".ci-work/parity"
            inputs.mkdir(parents=True)
            for name in evidence.INPUTS:
                (inputs / name).write_text('{"fixture":"تصویر"}\n', encoding="utf-8")
            (inputs / "current-user-dpapi.txt").write_text("excluded private fixture", encoding="utf-8")
            (inputs / "config-dpapi.json").write_text('{"excluded":"private fixture"}', encoding="utf-8")
            (root / "secrets.md").write_text("excluded registry fixture", encoding="utf-8")
            sha = "a" * 40
            archive = evidence.capture(root, baseline, sha, evidence.BASELINE)
            with zipfile.ZipFile(archive) as bundle:
                self.assertEqual(set(bundle.namelist()), {"provenance.json", *evidence.INPUTS})
                manifest = json.loads(bundle.read("provenance.json"))
                self.assertEqual(manifest["source_sha"], sha)
                self.assertEqual(manifest["baseline"], evidence.BASELINE)
                for name in evidence.INPUTS:
                    data = (inputs / name).read_bytes()
                    self.assertEqual(bundle.read(name), data)
                    self.assertEqual(manifest["inputs"][name]["sha256"], hashlib.sha256(data).hexdigest())
                self.assertIn("dist/config/schema.js", manifest["baseline_files"])
                self.assertIn("native/main.rs", manifest["native_files"])
                self.assertIn("schemas/config.schema.json", manifest["native_files"])
                self.assertIn("scripts/native-ci.py", manifest["harness_files"])
                self.assertNotIn("secrets.md", manifest["native_files"])
            evidence.verify(root, baseline, sha)
            (inputs / "config.json").write_text('{"changed":true}', encoding="utf-8")
            with self.assertRaisesRegex(RuntimeError, "input changed"):
                evidence.verify(root, baseline, sha)
            (inputs / "config.json").write_text('{"fixture":"تصویر"}\n', encoding="utf-8")
            (baseline / "dist/config/schema.js").write_text("changed", encoding="utf-8")
            with self.assertRaisesRegex(RuntimeError, "dependency changed"):
                evidence.verify(root, baseline, sha)
            (baseline / "dist/config/schema.js").write_text("fixture compiled", encoding="utf-8")
            with self.assertRaisesRegex(RuntimeError, "identity changed"):
                evidence.verify(root, baseline, "b" * 40)
            with zipfile.ZipFile(archive, "a") as bundle:
                bundle.writestr("unexpected.txt", b"not an input")
            with self.assertRaisesRegex(RuntimeError, "unexpected members"):
                evidence.verify(root, baseline, sha)
            with self.assertRaisesRegex(RuntimeError, "already exists"):
                evidence.capture(root, baseline, sha, evidence.BASELINE)


if __name__ == "__main__":
    unittest.main()
