# Linux and macOS setup

The native target is a ready executable, not a source-build prerequisite. Retirement-tree parity
and internal archive execution passed on all six targets at `b513884` / run `39519759155`. Publisher
signatures and a verified Release remain pending; do not switch a live client until that archive exists.

1. Download the matching Linux/macOS x86_64 or ARM64 archive and `.sha256` from this repository's
   Release. Verify the checksum, then extract into a writable installation folder.
2. Copy `.env.example` to `.env` in the extracted root (parent of `bin/`) only if no private config exists.
   Set API_ENDPOINT, API_KEY and exact MODEL_1_ID/MODEL_1_NAME; preserve existing data/config.
3. Register the absolute native executable with `bridge`; the host starts it automatically, without
   a launcher or separately running service.

No Node/npm/Rust/Python installation or local build is needed for normal usage. The tar archive
preserves the binary's executable bit. Private plaintext configuration remains owner-only:

```bash
chmod 600 .env
```

Claude Code registration from the extracted root:

```bash
claude mcp add --transport stdio --scope user numera-image-gen -- "$PWD/bin/numera-image-gen-mcp" bridge
```

Codex registration:

```bash
codex mcp add numera-image-gen -- "$PWD/bin/numera-image-gen-mcp" bridge
```

Claude Desktop uses the absolute native POSIX command and `args: ["bridge"]` in only Numera's entry.
Linux server support does not establish Desktop application support on Linux. An explicit absolute
NUMERA_CONFIG can point to an existing private file; otherwise discovery is installation-relative,
never caller CWD. API keys never go in host settings. Keep long-operation timeouts appropriately sized.

After reconnect call `health_check`, `list_connections`, `list_models` and model capabilities before
approving generation. Passing a probe does not prove account/model entitlement. Saved settings reload
without a new submission; storage/security policy changes require restart.

Managed temp/cache stays in the installation root; output/state/log/input defaults stay beside the
selected private config. Preserve originals, receipts/journals and migration snapshots. Keep the old
runtime until the owner has switched configuration and verified the actual native connection; then
remove only inspected obsolete project-local dependencies/build/temp/cache, never global caches.

Optional native `menu` provides direct HTTP start, port changes and connection guidance on both
Linux and macOS. `http` defaults to `127.0.0.1:1969/mcp`, requires a private local Bearer token, and
runs in foreground; Ctrl+C stops it. It is separate from automatic stdio `bridge` startup.
[HTTP/menu details](HTTP.md) describe the verified internal target and safe client authentication;
final signed Release and actual owner reconnection remain separate gates.

[Installation](../README.md#installation-and-setup) · [Claude clients](CLAUDE.md) · [Codex](CODEX.md).
