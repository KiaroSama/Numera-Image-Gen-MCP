"""Synthetic Apple ABI seam only; genuine Mach-O acceptance runs on both hosted Macs."""
from __future__ import annotations

import ctypes as ct
import hashlib
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]


def load():
    source = ROOT / "scripts/native-macos-signing.py"
    if not source.is_file():
        raise AssertionError("Memory-only native signing boundary is missing")
    spec = importlib.util.spec_from_file_location("macos_signing", source)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class NativeMacSigning(unittest.TestCase):
    def test_memory_only_identity_is_pinned_and_native_references_are_released(self):
        native = load()
        certificate = b"synthetic public certificate; not a native signature"

        class Function:
            def __init__(self, call):
                self.call = call
            def __call__(self, *args):
                return self.call(*args)

        class Framework:
            def __init__(self):
                self.next = 100
                self.objects = {}
                self.owned = []
                self.released = []
                self.calls = []
                self.status = 0
                self.import_status = 0
                self.count = 1
                self.identity_certificate = certificate
                names = ("CFDataCreate", "CFStringCreateWithBytes", "CFDictionaryCreate", "CFRelease",
                         "CFArrayGetCount", "CFArrayGetValueAtIndex", "CFDictionaryGetValue",
                         "CFDataGetLength", "CFDataGetBytePtr", "CFURLCreateFromFileSystemRepresentation",
                         "SecPKCS12Import", "SecIdentityCopyCertificate", "SecCertificateCopyData",
                         "SecStaticCodeCreateWithPath", "SecCodeSignerCreate", "SecCodeSignerAddSignatureWithErrors")
                for name in names:
                    setattr(self, name, Function(lambda *args, name=name: self.invoke(name, args)))
                self.symbols = {name: i + 1000 for i, name in enumerate((
                    "kSecImportToMemoryOnly", "kSecImportExportPassphrase", "kSecImportItemIdentity",
                    "kSecCodeSignerIdentity", "kSecCodeSignerRequireTimestamp", "kCFBooleanTrue", "kCFBooleanFalse"))}

            def symbol(self, name):
                return self.symbols[name]

            def own(self, value):
                self.next += 1
                self.objects[self.next] = value
                self.owned.append(self.next)
                return self.next

            def result(self, output, value):
                ct.cast(output, ct.POINTER(ct.c_void_p))[0] = self.own(value)
                return 0

            def invoke(self, name, args):
                self.calls.append((name, args))
                if name == "CFRelease":
                    self.released.append(args[0].value if isinstance(args[0], ct.c_void_p) else args[0])
                    return None
                if name in {"CFDataCreate", "CFStringCreateWithBytes"}:
                    return self.own(ct.string_at(args[1], args[2]))
                if name == "CFDictionaryCreate":
                    return self.own({args[1][i]: args[2][i] for i in range(args[3])})
                if name == "SecPKCS12Import":
                    options = self.objects[args[1]]
                    assert options[self.symbol("kSecImportToMemoryOnly")] == self.symbol("kCFBooleanTrue")
                    assert len(options) == 2
                    self.result(args[2], "items")
                    return self.import_status
                if name == "CFArrayGetCount":
                    return self.count
                if name == "CFArrayGetValueAtIndex":
                    return 4000
                if name == "CFDictionaryGetValue":
                    return 4001
                if name == "SecIdentityCopyCertificate":
                    return self.result(args[1], "certificate")
                if name == "SecCertificateCopyData":
                    return self.own(self.identity_certificate)
                if name == "CFDataGetLength":
                    return len(self.identity_certificate)
                if name == "CFDataGetBytePtr":
                    return self.identity_certificate
                if name == "CFURLCreateFromFileSystemRepresentation":
                    return self.own("url")
                if name == "SecStaticCodeCreateWithPath":
                    return self.result(args[2], "code")
                if name == "SecCodeSignerCreate":
                    self.signing_parameters = self.objects[args[0]]
                    return self.result(args[2], "signer")
                if name == "SecCodeSignerAddSignatureWithErrors":
                    if self.status:
                        self.result(args[3], "private error details; never rendered")
                    return self.status
                raise AssertionError(name)

        api = Framework()
        with patch.object(native.sys, "platform", "darwin"), patch.object(native.ct, "CDLL", return_value=api), \
                patch.object(native, "symbol", side_effect=lambda lib, name: lib.symbol(name)):
            for label in ("valid", "wrong-certificate", "multiple-identities", "import-failure", "sign-failure"):
                api.owned.clear()
                api.released.clear()
                api.calls.clear()
                api.identity_certificate = b"unrelated signer" if label == "wrong-certificate" else certificate
                api.count = 2 if label == "multiple-identities" else 1
                api.status = -50 if label == "sign-failure" else 0
                api.import_status = -25293 if label == "import-failure" else 0
                signer = native.AppleSigner()
                if label == "valid":
                    signer.sign(b"synthetic encrypted PFX", b"fixturepassword", hashlib.sha256(certificate).hexdigest(), [ROOT / "fixture"])
                    self.assertEqual(api.signing_parameters, {
                        api.symbol("kSecCodeSignerIdentity"): 4001,
                        api.symbol("kSecCodeSignerRequireTimestamp"): api.symbol("kCFBooleanFalse")})
                else:
                    with self.assertRaises(native.NativeSigningError):
                        signer.sign(b"synthetic encrypted PFX", b"fixturepassword", hashlib.sha256(certificate).hexdigest(), [ROOT / "fixture"])
                    if label != "sign-failure":
                        self.assertFalse(any(name == "SecCodeSignerAddSignatureWithErrors" for name, _ in api.calls))
                self.assertCountEqual(api.owned, api.released)
                self.assertEqual(len(api.released), len(set(api.released)))
                self.assertFalse(any("Keychain" in name or "Trust" in name or "Export" in name for name, _ in api.calls))

    def test_missing_memory_only_symbol_fails_before_import_without_fallback(self):
        native = load()
        with patch.object(native.sys, "platform", "darwin"), patch.object(native.ct, "CDLL"), \
                patch.object(native, "symbol", side_effect=ValueError("synthetic missing symbol")):
            with self.assertRaisesRegex(native.NativeSigningError, "capability_unavailable"):
                native.AppleSigner()


if __name__ == "__main__":
    unittest.main()
