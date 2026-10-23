"""Real hosted signature acceptance on disposable copies, after publisher inputs are removed."""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import secrets
import shutil
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("acceptance_signing", ROOT / "scripts/native-signing.py")
signing = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(signing)


def reject(action, label):
    try:
        action()
    except signing.SigningError:
        print(json.dumps({"component": "native-signing-acceptance", "rejected": label}), flush=True)
        return
    raise RuntimeError("Invalid signature accepted: " + label)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", required=True, choices=sorted(signing.TARGETS))
    args = parser.parse_args()
    if os.environ.get("GITHUB_ACTIONS") != "true" or (ROOT / ".ci-work/signing").exists():
        raise RuntimeError("Signature tests require hosted execution after publisher-input cleanup")
    if any(k.startswith("NUMERA_SIGNING_") for k in os.environ):
        raise RuntimeError("Publisher input environment leaked into acceptance")
    prepared = ROOT / ".ci-work/prepared" / args.target
    report = signing.verify(prepared, args.target)
    if report["source_sha"] != os.environ["GITHUB_SHA"]:
        raise RuntimeError("Signature source differs from current candidate")
    manifest, _ = signing.manifest(prepared, args.target)
    work = ROOT / ".ci-work/signature-acceptance"
    work.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=work, prefix="native-") as directory:
        scratch = Path(directory)
        scratch.chmod(0o700)
        public = scratch / "approved.pem"
        public.write_bytes(signing.pem("RSA PUBLIC KEY", signing.read(ROOT / "docs/signing/publisher-key.der", 8192)))
        # A fresh disposable test identity is never the publisher and is never uploaded.
        password = (secrets.token_urlsafe(32) + "\n").encode("ascii")
        key, cert, pfx = (scratch / n for n in ("wrong-key.pem", "wrong-cert.pem", "wrong.pfx"))
        old_mask = os.umask(0o077)
        try:
            signing.command([signing.tool("openssl"), "req", "-x509", "-newkey", "rsa:2048", "-noenc", "-sha256",
                             "-days", "1", "-subj", "/CN=Unrelated fixture signer", "-addext", "extendedKeyUsage=codeSigning",
                             "-addext", "basicConstraints=critical,CA:FALSE", "-addext", "keyUsage=critical,digitalSignature",
                             "-keyout", str(key), "-out", str(cert)])
            signing.command([signing.tool("openssl"), "pkcs12", "-export", "-inkey", str(key), "-in", str(cert),
                             "-out", str(pfx), "-passout", "stdin"], secret=password)
        finally:
            os.umask(old_mask)
        wrong_signature = scratch / "wrong-manifest.sig"
        signing.command([signing.tool("openssl"), "dgst", "-sha256", "-sign", str(key), "-out",
                         str(wrong_signature), str(prepared / "manifest.json")])
        reject(lambda: signing.command([signing.tool("openssl"), "dgst", "-sha256", "-verify", str(public),
            "-signature", str(wrong_signature), str(prepared / "manifest.json")]), "wrong-manifest-signer")
        changed = scratch / "changed-manifest.json"
        changed.write_bytes((prepared / "manifest.json").read_bytes() + b" ")
        reject(lambda: signing.command([signing.tool("openssl"), "dgst", "-sha256", "-verify", str(public),
            "-signature", str(prepared / "manifest.sig"), str(changed)]), "changed-manifest")
        identity = signing.public_identity()
        binaries = [row["path"] for row in manifest["files"] if row["path"].startswith("bin/")]
        if len(binaries) != 2:
            raise RuntimeError("Both executable signatures must be tested")
        for name in binaries:
            original = prepared / name
            copy = scratch / ("tampered-" + original.name)
            data = bytearray(signing.read(original, signing.MAX_BYTES))
            data[len(data) // 2] ^= 1
            copy.write_bytes(data)
            if "windows" in args.target or "apple" in args.target:
                reject(lambda: signing.native_verify(copy, args.target, scratch, identity), "tampered-native-" + original.name)
            else:
                changed_tree = scratch / ("changed-tree-" + original.name)
                shutil.copytree(prepared, changed_tree)
                (changed_tree / name).write_bytes(data)
                reject(lambda: signing.verify(changed_tree, args.target), "tampered-ELF-tree-" + original.name)
                shutil.rmtree(changed_tree)
            unsigned = scratch / ("wrong-" + original.name)
            shutil.copyfile(original, unsigned)
            if "windows" in args.target:
                stripped = scratch / ("unsigned-" + original.name)
                signing.command([signing.tool("osslsigncode"), "remove-signature", "-in", str(unsigned), "-out", str(stripped)])
                reject(lambda: signing.native_verify(stripped, args.target, scratch, identity), "unsigned-native-" + original.name)
                unsigned.unlink()
                signing.command([signing.tool("osslsigncode"), "sign", "-pkcs12", str(pfx), "-readpass", "-", "-h", "sha256",
                                 "-in", str(stripped), "-out", str(unsigned)], secret=password)
                reject(lambda: signing.native_verify(unsigned, args.target, scratch, identity), "wrong-native-signer-" + original.name)
            elif "apple" in args.target:
                signing.command(["/usr/bin/codesign", "--remove-signature", str(unsigned)])
                reject(lambda: signing.native_verify(unsigned, args.target, scratch, identity), "unsigned-native-" + original.name)
                certificate = signing.command([signing.tool("openssl"), "x509", "-in", str(cert), "-outform", "DER"], capture=True)
                before = signing.keychain_state()
                try:
                    signing.macos_sign(pfx, password, hashlib.sha256(certificate).hexdigest(), [unsigned])
                finally:
                    if signing.keychain_state() != before:
                        raise RuntimeError("Wrong-signer fixture changed keychain preferences")
                signing.command(["/usr/bin/codesign", "--verify", "--strict", str(unsigned)])
                reject(lambda: signing.native_verify(unsigned, args.target, scratch, identity), "wrong-native-signer-" + original.name)
        signing.verify(prepared, args.target)
    print(json.dumps({"component": "native-signing-acceptance", "target": args.target,
        "source_sha": report["source_sha"], "native_positive_negative": True, "publisher_inputs_absent": True,
        "wrong_signer_rejected": True, "tamper_rejected": True, "fixture_cleanup": True,
        "public_os_trust": False}), flush=True)


if __name__ == "__main__":
    main()
