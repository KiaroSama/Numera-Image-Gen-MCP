# Codex setup

The native target runs directly over stdio. No launcher, Node runtime or manually started service is
needed: Codex runs the absolute native executable with `args = ["bridge"]`. The Rust migration is
still awaiting hosted verification and a Release; do not switch a live host before installing the
verified binary.

Merge only Numera's table into the actual user configuration, preserving other entries:

```toml
[mcp_servers.numera-image-gen]
command = "C:/tools/numera-image-gen-mcp/bin/numera-image-gen-mcp.exe"
args = ["bridge"]
startup_timeout_sec = 10
tool_timeout_sec = 360
env = { NUMERA_CONFIG = "C:/tools/numera-image-gen-mcp/.env" }
```

The paths are examples. Linux/macOS use the actual absolute POSIX binary and configuration paths.
`NUMERA_CONFIG` can be omitted when the private `.env` is in the installation root (parent of `bin/`).
Never put an API key in the table. Restart/reconnect after command changes, then call `health_check`,
`list_connections` and `list_models`. Settings reload does not authorize or resubmit an image.

CLI registration from the extracted Linux/macOS installation root:

```bash
codex mcp add numera-image-gen -- "$PWD/bin/numera-image-gen-mcp" bridge
```

[Codex example](../examples/codex.toml) · [Official MCP documentation](https://developers.openai.com/codex/mcp).

Numera changes no host chat model. Never treat a configured/listed model as account entitlement;
inspect capabilities, explicitly authorize a potentially billable image and retain its request ID.
Unknown outcomes are inspected with `get_job`, never replaced automatically.
