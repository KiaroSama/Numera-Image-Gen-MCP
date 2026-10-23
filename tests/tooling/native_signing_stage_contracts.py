"""Small synthetic archive checks; real native signature acceptance belongs to hosted CI."""
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[2]


class SigningStage(unittest.TestCase):
    def test_stage_authenticates_complete_archive_before_writing_members(self):
        source = ROOT / "scripts/native-signing-stage.py"
        self.assertTrue(source.is_file(), "Protected signing artifact staging is missing")
        spec = importlib.util.spec_from_file_location("signing_stage", source)
        stage = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(stage)
        parent = ROOT / ".ci-work/native/signing-stage-tests"
        parent.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=parent) as directory:
            root = Path(directory)
            target = "x86_64-pc-windows-msvc"
            sha = "a" * 40
            entries = {"bin/numera-image-gen-mcp.exe": b"synthetic; never executed",
                       "bin/numera-dev.exe": b"synthetic; never executed",
                       "docs/assets/logo.png": (ROOT / "docs/assets/logo.png").read_bytes()}
            manifest = {"version": "0.2.0", "target": target, "source_sha": sha, "publisher": "KiaroSama",
                        "binary_sha256": hashlib.sha256(entries["bin/numera-image-gen-mcp.exe"]).hexdigest(),
                        "files": [{"path": k, "bytes": len(v), "sha256": hashlib.sha256(v).hexdigest()}
                                  for k, v in entries.items()]}
            entries["manifest.json"] = json.dumps(manifest).encode("utf-8")
            download = root / "download"
            download.mkdir()
            name = "unsigned-intermediate-numera-image-gen-mcp-0.2.0-" + target + ".zip"

            def archive(extra=None):
                data = io.BytesIO()
                with zipfile.ZipFile(data, "w") as bundle:
                    for k, v in (entries | (extra or {})).items():
                        bundle.writestr("unsigned-intermediate-numera-image-gen-mcp-0.2.0-" + target + "/" + k, v)
                payload = data.getvalue()
                (download / name).write_bytes(payload)
                (download / (name + ".sha256")).write_text(hashlib.sha256(payload).hexdigest() + "  " + name + "\n", encoding="utf-8")

            archive()
            installed = stage.stage(download, root / "prepared", target, sha)
            self.assertEqual((installed / "bin/numera-dev.exe").read_bytes(), entries["bin/numera-dev.exe"])
            for label, extra, expected_sha in [("private", {"secrets.md": b"synthetic forbidden member"}, sha),
                                               ("wrong-source", {}, "b" * 40),
                                               ("unlisted", {"docs/extra.txt": b"unlisted"}, sha)]:
                archive(extra)
                destination = root / label
                with self.subTest(label=label), self.assertRaises((RuntimeError, stage.signing.SigningError)):
                    stage.stage(download, destination, target, expected_sha)
                self.assertFalse(destination.exists())
            archive()
            (download / (name + ".sha256")).write_text("0" * 64 + "  " + name + "\n", encoding="utf-8")
            with self.assertRaises(RuntimeError):
                stage.stage(download, root / "bad-checksum", target, sha)
            self.assertFalse((root / "bad-checksum").exists())


if __name__ == "__main__":
    unittest.main()
