# Public native-signing fixtures

These files contain public manifests and signatures only. They are not ready application builds.
No private key, PFX, password or trust-store installation is needed to run the tests.

- `approved/`: version 0.1.0, synthetic executable bytes and the exact approved project logo.
- `update/`: version 0.2.0, different synthetic executable bytes and the same approved logo.
- Root target pairs: historical signed synthetic-logo manifests, retained as negative branding cases.
- `*.wrong.sig`: genuine signatures made with a disposable unrelated RSA key; the key was not saved.

The synthetic executable bytes are never executed. These fixtures prove manifest authentication,
member integrity and installation behavior, not PE/Mach-O signing or application branding in a build.
The production signing/publication gates are separate.

## Binary formats and exact text

`*.sig` and `*.wrong.sig` are binary RSA-3072 PKCS#1 v1.5 SHA-256 signatures (384 bytes), not text.
Do not decode, transcode or normalize them. `docs/signing/publisher-key.der` is a binary PKCS#1
RSAPublicKey; `docs/signing/publisher-cert.der` is a binary X.509 certificate. These required binary
formats are outside UTF-8 text validation. Rust's ring verifier and the independently checked Node
crypto verifier consume their original bytes.

`*.json` is UTF-8 without BOM. Each signature binds its exact JSON bytes, including whitespace;
formatting changes invalidate it. The formatter excludes this fixture directory deliberately.
The current certificate is not timestamped: current validity is required, through
2027-10-17T20:17:22Z. An expired identity requires an explicitly authorized custody/rotation decision,
not a bypass, automatic fixture rewrite or implicit new key.

Regeneration requires the existing protected project identity and explicit fixture-writing mode of
`scripts/native-signing-check.ps1`. The script uses exclusive creation and refuses to overwrite
existing fixture files. Never put signing inputs in this directory or upload the local custody store.
