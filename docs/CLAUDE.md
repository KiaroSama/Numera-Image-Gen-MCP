# Claude clients

Local stdio does not change the host's chat model. `examples/claude-project.json` shows the .mcp.json
shape; `examples/claude-desktop.json` shows Desktop's distinct local MCP shape. Replace Node path
with the installed absolute executable and use an absolute built entrypoint/config. Never replace
an entire existing settings file; merge only mcpServers.numera-image-gen with a backup.

Claude Code project registration can use a .mcp.json entry. User CLI registration, in PowerShell7:

```powershell
claude mcp add --transport stdio --scope user numera-image-gen -- 'C:/tools/node/node.exe' 'C:/projects/numera-image-gen-mcp/dist/index.js' --config 'C:/projects/numera-image-gen-mcp/config.local.json'
```

The command is an example, not registration performed by this project task. Current docs:
https://code.claude.com/docs/en/mcp . Desktop local server configuration depends on installation;
MSIX config may be under the package's LocalCache/Roaming/Claude. Choose actual file explicitly.

Changing a terminal variable does not update an already-running Desktop process. Restart/reconnect
and actually call health_check/list_connections to confirm selected configuration. Generic SDK tests
are not Claude GUI tests. Hosted/cloud clients cannot inherently run your local stdio process or see
local file paths; file saving is reliable, preview display is host-dependent.
