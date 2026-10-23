"""Hosted-only Apple native signing with memory-only PKCS12 import; no keychain changes."""
from __future__ import annotations

import argparse
import ctypes as ct
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import sys


class NativeSigningError(RuntimeError):
    """Fixed diagnostic, never private Apple error text."""


def symbol(library, name):
    value = ct.c_void_p.in_dll(library, name).value
    if not value:
        raise NativeSigningError("macos_signing_capability_unavailable")
    return value


class AppleSigner:
    def __init__(self):
        if sys.platform != "darwin":
            raise NativeSigningError("native_macos_signer_required")
        try:
            self.cf = ct.CDLL("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation")
            self.security = ct.CDLL("/System/Library/Frameworks/Security.framework/Security")
            pointer, index, status, flags = ct.c_void_p, ct.c_long, ct.c_int32, ct.c_uint32
            signatures = {
                "CFDataCreate": (pointer, [pointer, pointer, index]),
                "CFStringCreateWithBytes": (pointer, [pointer, pointer, index, ct.c_uint32, ct.c_ubyte]),
                "CFDictionaryCreate": (pointer, [pointer, ct.POINTER(pointer), ct.POINTER(pointer), index, pointer, pointer]),
                "CFRelease": (None, [pointer]),
                "CFArrayGetCount": (index, [pointer]),
                "CFArrayGetValueAtIndex": (pointer, [pointer, index]),
                "CFDictionaryGetValue": (pointer, [pointer, pointer]),
                "CFDataGetLength": (index, [pointer]),
                "CFDataGetBytePtr": (pointer, [pointer]),
                "CFURLCreateFromFileSystemRepresentation": (pointer, [pointer, pointer, index, ct.c_ubyte]),
                "SecPKCS12Import": (status, [pointer, pointer, ct.POINTER(pointer)]),
                "SecIdentityCopyCertificate": (status, [pointer, ct.POINTER(pointer)]),
                "SecCertificateCopyData": (pointer, [pointer]),
                "SecStaticCodeCreateWithPath": (status, [pointer, flags, ct.POINTER(pointer)]),
                "SecCodeSignerCreate": (status, [pointer, flags, ct.POINTER(pointer)]),
                "SecCodeSignerAddSignatureWithErrors": (status, [pointer, pointer, flags, ct.POINTER(pointer)]),
            }
            for name, (result, arguments) in signatures.items():
                function = getattr(self.cf if name.startswith("CF") else self.security, name)
                function.restype, function.argtypes = result, arguments
            # Resolve every mandatory symbol before importing any private material. No omitted-flag fallback.
            self.keys = {name: symbol(self.security, name) for name in (
                "kSecImportToMemoryOnly", "kSecImportExportPassphrase", "kSecImportItemIdentity",
                "kSecCodeSignerIdentity", "kSecCodeSignerRequireTimestamp")}
            self.true, self.false = (symbol(self.cf, name) for name in ("kCFBooleanTrue", "kCFBooleanFalse"))
        except (AttributeError, OSError, ValueError):
            raise NativeSigningError("macos_signing_capability_unavailable") from None

    def sign(self, pfx: bytes, password: bytes, certificate_sha256: str, files: list[Path]):
        if (not pfx or len(pfx) > 1024 * 1024 or not re.fullmatch(rb"[A-Za-z0-9+/=_-]{1,4095}", password)
                or not re.fullmatch(r"[a-f0-9]{64}", certificate_sha256) or not 1 <= len(files) <= 2):
            raise NativeSigningError("invalid_memory_signing_input")
        owners = []

        def own(value):
            if not value:
                raise NativeSigningError("macos_signing_allocation_failed")
            owners.append(value)
            return value

        def result(function, *args):
            value = ct.c_void_p()
            code = function(*args, ct.byref(value))
            if value.value:
                owners.append(value.value)
            if code or not value.value:
                raise NativeSigningError("macos_signing_native_operation_failed")
            return value.value

        def dictionary(pairs):
            # No retain callbacks: all input references are held in owners through the entire operation.
            keys = (ct.c_void_p * len(pairs))(*[key for key, _ in pairs])
            values = (ct.c_void_p * len(pairs))(*[value for _, value in pairs])
            return own(self.cf.CFDictionaryCreate(None, keys, values, len(pairs), None, None))

        try:
            data = own(self.cf.CFDataCreate(None, pfx, len(pfx)))
            passphrase = own(self.cf.CFStringCreateWithBytes(None, password, len(password), 0x08000100, False))
            options = dictionary([(self.keys["kSecImportToMemoryOnly"], self.true),
                                  (self.keys["kSecImportExportPassphrase"], passphrase)])
            items = result(self.security.SecPKCS12Import, data, options)
            if self.cf.CFArrayGetCount(items) != 1:
                raise NativeSigningError("macos_signing_single_identity_required")
            item = self.cf.CFArrayGetValueAtIndex(items, 0)
            identity = self.cf.CFDictionaryGetValue(item, self.keys["kSecImportItemIdentity"]) if item else None
            if not identity:
                raise NativeSigningError("macos_signing_identity_not_found")
            certificate = result(self.security.SecIdentityCopyCertificate, identity)
            der = own(self.security.SecCertificateCopyData(certificate))
            size = self.cf.CFDataGetLength(der)
            if not 1 <= size <= 16384:
                raise NativeSigningError("macos_signing_certificate_bound")
            content = self.cf.CFDataGetBytePtr(der)
            if not content or hashlib.sha256(ct.string_at(content, size)).hexdigest() != certificate_sha256:
                raise NativeSigningError("macos_signing_certificate_mismatch")
            parameters = dictionary([(self.keys["kSecCodeSignerIdentity"], identity),
                                     (self.keys["kSecCodeSignerRequireTimestamp"], self.false)])
            # Exported Apple signing SPI, also used by codesign; missing capability fails closed.
            signer = result(self.security.SecCodeSignerCreate, parameters, 0)
            for file in files:
                path = os.fsencode(file)
                url = own(self.cf.CFURLCreateFromFileSystemRepresentation(None, path, len(path), False))
                code = result(self.security.SecStaticCodeCreateWithPath, url, 0)
                error = ct.c_void_p()
                status = self.security.SecCodeSignerAddSignatureWithErrors(signer, code, 0, ct.byref(error))
                if error.value:
                    owners.append(error.value)
                if status:
                    raise NativeSigningError("macos_signing_native_signature_failed")
        finally:
            for value in reversed(owners):
                self.cf.CFRelease(value)


def regular(path: Path, limit: int) -> bytes:
    root = Path(__file__).resolve().parent.parent / ".ci-work"
    info = path.lstat()
    if (not path.is_absolute() or not path.resolve(strict=True).is_relative_to(root.resolve(strict=True))
            or not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or info.st_size > limit):
        raise NativeSigningError("owned_regular_signing_input_required")
    with os.fdopen(os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK), "rb") as stream:
        actual = os.fstat(stream.fileno())
        if (actual.st_dev, actual.st_ino) != (info.st_dev, info.st_ino):
            raise NativeSigningError("changed_signing_input")
        value = stream.read(limit + 1)
    if len(value) > limit:
        raise NativeSigningError("memory_signing_input_bound")
    return value


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--qualify", action="store_true")
    parser.add_argument("--pfx", type=Path)
    parser.add_argument("--certificate-sha256")
    parser.add_argument("--file", type=Path, action="append", default=[])
    args = parser.parse_args()
    signer = AppleSigner()
    if args.qualify:
        print(json.dumps({"component": "native-macos-signing", "memory_only_capability": True}), flush=True)
        return
    if (os.environ.get("GITHUB_ACTIONS") != "true" or not args.pfx
            or not args.certificate_sha256 or not 1 <= len(args.file) <= 2):
        raise NativeSigningError("hosted_memory_signing_required")
    pfx = regular(args.pfx, 1024 * 1024)
    for file in args.file:
        regular(file, 256 * 1024 * 1024)
    password = sys.stdin.buffer.read(4097)
    if not password.endswith(b"\n"):
        raise NativeSigningError("protected_password_format")
    signer.sign(pfx, password[:-1], args.certificate_sha256, args.file)
    print(json.dumps({"component": "native-macos-signing", "native_signing_completed": True,
                      "memory_only": True, "public_os_trust": False}), flush=True)


if __name__ == "__main__":
    try:
        main()
    except (NativeSigningError, OSError, ValueError, TypeError) as error:
        print(json.dumps({"component": "native-macos-signing", "code": str(error)
                          if isinstance(error, NativeSigningError) else "memory_only_native_signing_failed"}),
              file=sys.stderr, flush=True)
        raise SystemExit(1) from None
