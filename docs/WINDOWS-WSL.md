# Native Windows, Linux and macOS

Ready native archives are the installation target. All six native targets passed retirement-tree
parity and internal extracted-archive checks at `b513884` / run `39519759155`. Publisher signatures
and a verified Release remain pending; that earlier run does not certify later signing changes.

Windows uses a native MSVC `.exe`; Linux and macOS use their native executables. Choose the matching
x86_64 or ARM64 archive from the verified Release. Normal users do not install Node/npm/Rust/Python or
build locally. The binary does not bundle ComfyUI, GPU drivers, models/checkpoints or custom nodes.

A client starts the native executable with `args: ["bridge"]`. It does not need a launcher or background
service. Keep binary/config/storage in a writable location; the managed temporary/cache directories are
inside the installation root, never a global fallback. Preserve all private configuration, originals,
receipts/journals and migration snapshots during updates.

Use native path syntax per host. Existing Windows DPAPI secret files require the same Windows user;
DPAPI files cannot be decrypted on Linux/macOS. POSIX private secret files require owner-only permissions.
Windows supports declared absolute local/UNC inputs and rejects device paths/alternate data streams;
POSIX paths use allowed input roots and reject resolved escapes. Actual OS-specific behavior belongs to
native CI fixtures, not an SDK-only or cross-compilation claim.

## WSL

WSL is a separate Linux environment. Windows hosts do not inherently execute a Linux ELF binary, and
Linux processes do not inherently have Windows local file access. Choose the native Windows binary for
a Windows local stdio host, or explicitly configure a supported WSL-aware host with Linux paths.
Do not assume source or data paths are interchangeable. Keep project-owned Linux temp/cache/build state
inside its canonical Linux workspace; do not clear shared/global runtimes or another project.

Offline/native CI tests and actual Claude/Codex GUI behavior are different evidence. A successful
provider probe or listed model does not prove paid generation/editing entitlement. GPU/live image tests
require their own prerequisites and authorization; the Rust rewrite does not authorize them.
