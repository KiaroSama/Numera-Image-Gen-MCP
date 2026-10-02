# Codex clients

Current docs https://developers.openai.com/codex/mcp.md describe shared config.toml for local CLI,
desktop and IDE clients; trusted project scope also possible. Use examples/codex.toml as one entry,
not a replacement for the existing file. Preserve model/permission/server settings and comments.

Set startup_timeout_sec10 and tool_timeout_sec360 so default60s does not prematurely cancel a
300000ms image wait. command/args absolute, env only NUMERA_CONFIG or declared environment names,
never plaintext keys. CLI example in PowerShell7:

```powershell
codex mcp add numera-image-gen --env 'NUMERA_CONFIG=C:/projects/numera-image-gen-mcp/config.local.json' -- 'C:/tools/node/node.exe' 'C:/projects/numera-image-gen-mcp/dist/index.js'
```

Restart/reconnect existing desktop sessions after environment changes; call health_check to verify.
No host registration or actual Codex GUI test was performed without explicit opt-in. The real SDK
stdio fixture verifies protocol interoperability, not product-tab display or cloud-local filesystem
availability. Request IDs must be reused for host retries; unknown outcomes are not new authorizations.
