# Linux and macOS setup

Node24.21.0 or newer24.x/npm required. Install the native Node build for your OS/architecture using
the official https://nodejs.org/en/download distribution. Numera does not install a runtime itself.

In the checkout, install and build once:

```bash
npm ci
```

```bash
npm run build
```

Save config.local.json next to package.json, using the public root config.local.json.example as a template. Only
api_endpoint, api_key and models:[{id,name}] are required. Use your actual image API prefix and exact
model IDs; no Windows paths. Default profile openai-images; explicit omniroute/9router or openai-responses with separate
orchestration_model. Optional per-model output_format/size/quality need no code edit.
First model is default, display names do not change native IDs. Outputs/state/logs/inputs are local
folders beside the file. Keep plaintext API key private, never commit/sync/share it:

```bash
chmod 600 config.local.json
```

For Claude Code, register once from the checkout (CLI writes a host entry, not an API key):

```bash
claude mcp add --transport stdio --scope user numera-image-gen -- "$(command -v node)" "$PWD/dist/index.js"
```

For Codex, register once:

```bash
codex mcp add numera-image-gen -- "$(command -v node)" "$PWD/dist/index.js"
```

For Claude Desktop, merge only mcpServers.numera-image-gen into its actual config using the example
entry; replace Windows command/args with the absolute native Node and checkout dist/index.js paths.
Do not replace existing settings. Existing scripts/register.mjs supports entry-scoped JSON/TOML edits
with explicit targets/backups; Windows PowerShell setup is optional, not a Linux/macOS dependency.
Set Codex tool_timeout_sec=360 when using long image requests; preserve unrelated settings.

Reconnect the MCP server after saving config changes. Ask it to call health_check/list_connections/
list_models first; configured catalog is offline and is not entitlement proof. refresh:true contacts
the selected backend read-only. Do not run dist/index.js expecting a browser UI: it speaks stdio MCP.
The host must launch it with pipes. A config file alone cannot register a tool in an arbitrary host.

Hosted CI includes macos-latest (Apple Silicon), Ubuntu and Windows native dependencies, real stdio
fixtures, coverage and clean packed installation. Exact successful evidence lives in TEST-RESULTS.md;
Intel Mac/live account/actual GUI/GPU behavior must not be inferred from an Apple Silicon fixture.
DPAPI is Windows-only; compact plaintext key or advanced private/env sources work cross-platform.
