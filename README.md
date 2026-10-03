# Numera Image-Gen MCP

<div align="center">

<img src="docs/assets/logo.png" alt="Numera Image-Gen MCP logo" width="320" />

[![CI](https://github.com/KiaroSama/Numera-Image-Gen-MCP/actions/workflows/ci.yml/badge.svg)](https://github.com/KiaroSama/Numera-Image-Gen-MCP/actions/workflows/ci.yml)
[![License GPL-3.0-only](https://img.shields.io/badge/license-GPL--3.0--only-blue)](LICENSE)
[![Node 24](https://img.shields.io/badge/Node.js-24-43853d)](package.json)
[![TypeScript strict](https://img.shields.io/badge/TypeScript-strict-3178c6)](tsconfig.json)
[![ESM](https://img.shields.io/badge/modules-ESM-blue)](package.json)
[![MCP stdio](https://img.shields.io/badge/MCP-stdio-7651a8)](docs/ARCHITECTURE.md)
[![Windows target](https://img.shields.io/badge/target-Windows_11-0078d4)](docs/WINDOWS-WSL.md)
[![Linux CI](https://img.shields.io/badge/CI_matrix-Linux-555)](.github/workflows/ci.yml)
[![Zod schema](https://img.shields.io/badge/schema-Zod-3068b7)](schemas/config.schema.json)
[![Sharp image validation](https://img.shields.io/badge/images-Sharp-99cc00)](src/files/images.ts)
[![SQLite receipts](https://img.shields.io/badge/receipts-SQLite-003b57)](src/jobs/store.ts)
[![Documentation](https://img.shields.io/badge/docs-Markdown-blue)](docs/CONFIGURATION.md)
[![npm lockfile](https://img.shields.io/badge/install-npm_ci-cb3837)](package-lock.json)
[![Support donations](https://img.shields.io/badge/Support-donations-d04a9a)](#donate)

</div>

A local provider-independent image generation/editing MCP server. Keep your host chat model unchanged;
image work is an explicitly selected tool operation. Named connections, separate OmniRoute/9router
profiles, native image protocols and configured GPU ComfyUI workflows. No paid POST retries, hidden
uploads, prompt enhancement or automatic provider/model fallback.

**Status:** published offline-tested implementation on Windows, Linux and macOS.
Windows297 tests pass/3 POSIX-only skipped; Linux/macOS299 pass/1 Windows-only DPAPI skipped.
Runtime coverage: at least89.74 percent statements and88.31 percent branches.
See [test evidence](docs/TEST-RESULTS.md);
no npm publication, all-model support, GUI integration or production-readiness claim.
One owner-authorized real generation also succeeded on2026-10-03 through the configured Images API:
JPEG1024×1024, saved bytes/hash verified. This is one model/generation, not editing/all-provider proof.

## Installation and setup

### 1. Install prerequisites and get the project

Install [Node.js](https://nodejs.org/en/download) >=24.21.0 and <25, npm (bundled with Node), and
[Git](https://git-scm.com/downloads). Use the native build for Windows, Linux or macOS.
PowerShell7 is needed only for optional Windows setup/DPAPI scripts. There is no published npm
release: use this repository rather than an unverified similarly named npm package.

```bash
git clone https://github.com/KiaroSama/Numera-Image-Gen-MCP.git
```

```bash
cd Numera-Image-Gen-MCP
```

Confirm Node reports a supported24.x version:

```bash
node --version
```

### 2. Install dependencies and build

Run from the checkout on Windows, Linux or macOS:

```bash
npm ci
```

```bash
npm run build
```

The compiled entry point is dist/index.js. It is a stdio MCP server, not a web page; your MCP client
launches it with pipes. Do not run it expecting an interactive menu or browser UI.

### 3. Create your private configuration

Copy [config.local.json.example](config.local.json.example) to config.local.json next to package.json.
Do not overwrite an existing private config. In Windows PowerShell:

```powershell
Copy-Item -LiteralPath config.local.json.example -Destination config.local.json
```

On Linux/macOS:

```bash
cp -n config.local.json.example config.local.json
```

Open config.local.json in a text editor and replace the endpoint, key and exact model IDs:

```json
{
  "api_endpoint": "https://YOUR_API_HOST/YOUR_API_PREFIX",
  "api_key": "REPLACE_WITH_YOUR_API_KEY",
  "models": [
    {
      "id": "YOUR_EXACT_IMAGE_MODEL_ID_1",
      "name": "Image model 1",
      "enabled": true,
      "output_format": null,
      "size": null,
      "quality": null
    },
    {
      "id": "YOUR_EXACT_IMAGE_MODEL_ID_2",
      "name": "Image model 2",
      "enabled": false,
      "output_format": null,
      "size": null,
      "quality": null
    },
    {
      "id": "YOUR_EXACT_IMAGE_MODEL_ID_3",
      "name": "Image model 3",
      "enabled": false,
      "output_format": null,
      "size": null,
      "quality": null
    }
  ]
}
```

The endpoint is your chosen HTTP(S) API prefix: hosted, localhost or reverse-proxy, not a fixed port.
For example, https://api.openai.com/v1 or your own /api/v1; do not include /responses or
/images/generations. The default is generic openai-images, not an OmniRoute-specific connection. Any gateway exposing
compatible Images operations can use its own api_endpoint/api_key; a chat-only API is insufficient.
For gateway-specific forwarding/discovery behavior, explicitly select
`"profile":"omniroute"` or `"profile":"9router"` when using those gateways. Both use your chosen
endpoint and key; neither requires a fixed port or an installed copy on the Numera host.

Each of the three template slots has enabled/output_format/size/quality. Set the exact ID/name and
enabled:true when activating an extra slot; enabled:false slots are excluded from the configured
model list and cannot be submitted. At least one model must be enabled. Null output fields mean
provider defaults and are not sent to the API; replace null with a supported value when needed.
Format/size/quality are optional per-model defaults; explicit tool arguments override them. Use only
settings supported by your selected API/model; omit them for provider defaults. First listed model
is the default among enabled slots; select others by exact ID. Names are display labels, not routing aliases.

For Responses use [simple-responses.json](examples/simple-responses.json): profile openai-responses
plus a separate orchestration_model. The selected image model/settings form one image_generation tool;
the orchestration model is the top-level request model. An endpoint alone cannot identify its wire
protocol. Other native protocols use [advanced config](docs/CONFIGURATION.md).

Default outputs/state/logs/inputs directories sit beside the selected config. This plaintext-key file
is ignored and excluded from packages: never share, sync or commit it. On Linux/macOS restrict access:

```bash
chmod 600 config.local.json
```

Save changes and reconnect to apply them; there is no hot reload or automatic paid request.
No environment variables, credential wizard or storage-path setup are required for compact config.

### 4. Register Numera in your MCP client

Choose one client below. Registration happens once; saving application config alone does not register
a server. Use absolute native paths. Never put the API key in the host entry.

#### Claude Desktop (Windows example)

Open **Settings → Developer → Edit Config**. Add this member inside the existing mcpServers object,
keeping every other server. These are neutral example paths; replace them with your actual absolute paths. Forward slashes work in Windows JSON and avoid backslash escaping.

```json
    "numera-image-gen": {
      "command": "C:/tools/node/node.exe",
      "args": [
        "C:/projects/numera-image-gen-mcp/dist/index.js",
        "--config",
        "C:/projects/numera-image-gen-mcp/config.local.json"
      ]
    },
```

This is an inner member, not a complete JSON file. Keep the comma only when another member follows;
the preceding server also needs a comma. If this is your first server, the complete shape is:

```json
{
  "mcpServers": {
    "numera-image-gen": {
      "command": "ABSOLUTE_PATH_TO_NODE",
      "args": [
        "ABSOLUTE_PATH_TO_NUMERA/dist/index.js",
        "--config",
        "ABSOLUTE_PATH_TO_NUMERA/config.local.json"
      ]
    }
  }
}
```

On macOS use absolute POSIX paths; do not reuse Windows paths. Use Edit Config to find the actual
Desktop settings file (packaged Windows installations can use a different location). Save, fully quit
Claude Desktop and reopen it. Numera should appear among its local MCP tools. Desktop availability is
separate from the native Linux server support. See [Claude clients](docs/CLAUDE.md).

#### Claude Code

From the built checkout on Linux/macOS:

```bash
claude mcp add --transport stdio --scope user numera-image-gen -- "$(command -v node)" "$PWD/dist/index.js" --config "$PWD/config.local.json"
```

Windows PowerShell, using neutral example paths (replace them for your installation):

```powershell
claude mcp add --transport stdio --scope user numera-image-gen -- 'C:/tools/node/node.exe' 'C:/projects/numera-image-gen-mcp/dist/index.js' --config 'C:/projects/numera-image-gen-mcp/config.local.json'
```

Project-scoped JSON alternative: [Claude project entry](examples/claude-project.json). Do not replace
the whole .mcp.json or unrelated host settings.

#### Codex

From the built checkout on Linux/macOS:

```bash
codex mcp add numera-image-gen -- "$(command -v node)" "$PWD/dist/index.js" --config "$PWD/config.local.json"
```

For Windows or manual TOML configuration, merge [the Codex entry](examples/codex.toml) into your
existing config, replace the paths and keep tool_timeout_sec=360 for long image operations.
See [Codex setup](docs/CODEX.md).

#### Codex: manual configuration

Add this table to ~/.codex/config.toml (or a trusted project's .codex/config.toml), preserving existing
tables. All paths below are neutral examples; replace them with your actual paths:

```toml
[mcp_servers.numera-image-gen]
command = "C:/tools/node/node.exe"
args = ["C:/projects/numera-image-gen-mcp/dist/index.js", "--config", "C:/projects/numera-image-gen-mcp/config.local.json"]
startup_timeout_sec = 10
tool_timeout_sec = 360
```

Codex CLI, desktop and IDE extension share host configuration. Check with `codex mcp list` and start
a new session. [Official Codex MCP documentation](https://developers.openai.com/codex/mcp).

#### Kiro

Merge the following into .kiro/settings/mcp.json for the workspace, or ~/.kiro/settings/mcp.json for
all workspaces. Enable MCP support in Kiro settings, save and check that the server reconnects:

```json
{
  "mcpServers": {
    "numera-image-gen": {
      "command": "C:/tools/node/node.exe",
      "args": [
        "C:/projects/numera-image-gen-mcp/dist/index.js",
        "--config",
        "C:/projects/numera-image-gen-mcp/config.local.json"
      ],
      "disabled": false,
      "autoApprove": []
    }
  }
}
```

Keep generation/edit approvals enabled; do not auto-approve all tools just to connect.
[Official Kiro configuration](https://kiro.dev/docs/mcp/configuration/).

#### Cursor

Merge this into .cursor/mcp.json for the project or ~/.cursor/mcp.json globally. Enable the server in
Cursor's Customize/MCP page and check its tool list:

```json
{
  "mcpServers": {
    "numera-image-gen": {
      "type": "stdio",
      "command": "C:/tools/node/node.exe",
      "args": [
        "C:/projects/numera-image-gen-mcp/dist/index.js",
        "--config",
        "C:/projects/numera-image-gen-mcp/config.local.json"
      ]
    }
  }
}
```

[Official Cursor MCP documentation](https://cursor.com/docs/context/mcp).

#### Hermes Agent

Add this server under the existing mcp_servers map in ~/.hermes/config.yaml. This is Hermes Agent
by Nous Research, not an unrelated product named Hermes:

```yaml
mcp_servers:
  numera-image-gen:
    command: "C:/tools/node/node.exe"
    args:
      - "C:/projects/numera-image-gen-mcp/dist/index.js"
      - "--config"
      - "C:/projects/numera-image-gen-mcp/config.local.json"
```

Restart the Hermes session/gateway that should load it, or use its supported /reload-mcp command.
Ask for list_models before authorizing generation. Do not paste the private API key into Hermes config.
[Official Hermes MCP documentation](https://hermes-agent.nousresearch.com/docs/user-guide/features/mcp/).

#### OpenCode

Merge this into project opencode.json/opencode.jsonc or global ~/.config/opencode/opencode.json.
OpenCode uses a command array under mcp, not a Claude-style mcpServers object:

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "numera-image-gen": {
      "type": "local",
      "command": [
        "C:/tools/node/node.exe",
        "C:/projects/numera-image-gen-mcp/dist/index.js",
        "--config",
        "C:/projects/numera-image-gen-mcp/config.local.json"
      ],
      "enabled": true,
      "timeout": 10000
    }
  }
}
```

The shown timeout bounds tool discovery, not image-generation duration. Check `opencode mcp list`.
[Official MCP guide](https://opencode.ai/docs/mcp-servers/) and [config locations](https://opencode.ai/docs/config/).

#### VS Code / GitHub Copilot

The current portable workspace format is .mcp.json, using mcpServers; reuse the complete Claude Code
project entry with neutral paths. User-wide portable configuration is ~/.copilot/mcp-config.json.
For existing VS Code-native .vscode/mcp.json, the top-level key is servers instead:

```json
{
  "servers": {
    "numera-image-gen": {
      "type": "stdio",
      "command": "C:/tools/node/node.exe",
      "args": [
        "C:/projects/numera-image-gen-mcp/dist/index.js",
        "--config",
        "C:/projects/numera-image-gen-mcp/config.local.json"
      ]
    }
  }
}
```

Use MCP: Add Server or MCP: Open User Configuration, trust the reviewed local executable, start the
server and enable its tools in Agent chat. Remote workspaces run the command on their configured host:
Windows paths cannot launch a process on a Linux container.
[Official VS Code guide](https://code.visualstudio.com/docs/copilot/customization/mcp-servers).

#### Gemini CLI

For Gemini CLI installations, add a server to ~/.gemini/settings.json (user) or .gemini/settings.json
(project). Availability of Gemini CLI versus newer Antigravity tooling depends on your account:

```json
{
  "mcpServers": {
    "numera-image-gen": {
      "command": "C:/tools/node/node.exe",
      "args": [
        "C:/projects/numera-image-gen-mcp/dist/index.js",
        "--config",
        "C:/projects/numera-image-gen-mcp/config.local.json"
      ],
      "timeout": 360000,
      "trust": false
    }
  }
}
```

Restart/reconnect and inspect /mcp; keep trust:false so billable operations require approval.
[Official Gemini CLI MCP documentation](https://geminicli.com/docs/tools/mcp-server/).

These are configuration instructions verified against official documentation, not claims that every
client GUI was practically tested. On Linux/macOS replace the example Windows Node/project paths with
absolute POSIX paths. Do not set an MCP URL to the provider's api_endpoint: Numera's transport is stdio.

### 5. Verify the connection and generate your first image

In your client ask: **Call Numera health_check, list_connections and list_models. Do not generate yet.**
Ask: **Show the image models configured in Numera as a table of display name and exact model ID.
Call list_models with refresh=false; do not generate.**

The agent receives the configured id/name pairs through MCP and can display them to you without
reading the private key or config file. Pass limit/offset to list_models when the list spans pages.
Configured models are listed offline; refresh:true additionally queries the selected catalog and can
include unconfigured provider models. A listed ID does not prove account entitlement.
For a direct read-only check from the checkout on Linux/macOS:

```bash
node scripts/bounded.mjs 90000 20000 scripts/readiness.mjs --config "$PWD/config.local.json" --probe
```

Windows PowerShell:

```powershell
node scripts/bounded.mjs 90000 20000 scripts/readiness.mjs --config (Join-Path $PWD 'config.local.json') --probe
```

Exit0 means local configuration/storage/credentials and the requested read-only catalog check are
ready; it does not prove image-generation entitlement. A gateway without a catalog can still have an
image route: inspect the explicit blocker rather than treating discovery as generation proof.

When you approve a potentially billable request, ask your client: **Use Numera generate_image once
to create a blue mug beside an orange. count=1; request_id=my-first-image-001.** Use the configured
default model or provide its exact native ID. Reuse that ID only for the same logical request; use
a new ID for a deliberately new image. Never retry an outcome_unknown request with a new ID.

Images are saved beneath outputs/ beside your private config, with actual dimensions/MIME/hash in
the receipt. Open the returned path locally; cloud clients cannot inherently read your local files.
return_mode=files_and_preview adds a bounded preview when the host supports image tool results.

### Update and troubleshoot

Before updating, close the MCP server and preserve your private config and outputs. If your checkout
has local source changes, resolve those first; never use destructive reset/clean commands.

```bash
git pull --ff-only
```

```bash
npm ci
```

```bash
npm run build
```

Reconnect after rebuilding. The ignored config is not replaced by an update. Common problems:

| Problem                            | Check                                                                                                                              |
| ---------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| Server absent in Claude            | Correct inner mcpServers entry, valid JSON/no trailing final comma, absolute existing Node/dist/config paths, full Desktop restart |
| Startup fails                      | Supported Node24, npm ci/build completed, nonempty private key/model IDs, writable config-directory storage                        |
| API401/403                         | Selected endpoint/key/account; never paste keys or read another application's credential store                                     |
| Route404 or unsupported parameters | Correct API prefix/profile, exact model and supported size/quality/format; no full operation URL                                   |
| Timeout or uncertain outcome       | Inspect get_job with the original request_id; no automatic resubmission or provider fallback                                       |

Stdout is exclusively MCP traffic; logs go stderr/files. npm run smoke checks local handshake/tools/
health without a provider. Optional Windows [setup.ps1](scripts/setup.ps1) supports -WhatIf and explicit
entry-scoped registration with backups. Read-only readiness and real-test prerequisites:
[Practical testing](docs/PRACTICAL-TESTING.md). Missing declared credentials fails with exit2.

## Protocols and tools

### OpenAI-compatible vs Anthropic-compatible API

Numera currently submits images through evidenced image APIs, including OpenAI-compatible Images
and Responses. Using Claude as the MCP host does not require an Anthropic image endpoint: Claude
calls Numera over MCP, and Numera calls your selected image backend independently.

The standard Anthropic POST /v1/messages contract accepts images for vision but does not define a
native generated-image output or OpenAI-style image_generation tool. A generic Anthropic-compatible
chat endpoint is therefore not sufficient for image generation. Numera does not claim such an adapter.

The examined OmniRoute3.8.51 Messages path delegates to chat; image-only models are rejected and its
Claude response translation does not preserve generated image content. Use its supported
/v1/images/generations route instead. A future Messages image-output adapter needs an explicit
gateway extension defining request options, authentication and final image bytes; no protocol
inference, silent parameter dropping or automatic fallback is permitted.

Sources: [Anthropic Messages](https://platform.claude.com/docs/en/api/messages/create),
[vision input](https://platform.claude.com/docs/en/build-with-claude/vision), and
[pinned OmniRoute response conversion](https://github.com/diegosouzapw/OmniRoute/blob/c1e30b7676975feb298b49eff6ff58923c04b89e/open-sse/handlers/responseTranslator.ts#L736).

Generic Images, OmniRoute, 9router, Gemini generateContent, Gemini Interactions, OpenAI Responses image
tools, dedicated OpenRouter Images, opt-in chat image output and explicitly bound ComfyUI API graphs.
[Compatibility/limitations](docs/COMPATIBILITY.md) distinguish protocol evidence from account/model
availability. Antigravity refs are blocked at the examined gateway versions that drop input images.

10 tools: health_check, list_connections, list_models, get_model_capabilities, generate_image,
edit_image, get_job, cancel_job, list_outputs, get_output_info. Use one request_id per logical operation
and reuse it on host retries. Unknown outcomes never automatically resubmit. Native batches require
verified limits. Edit sources are typed output_id/path/url/data_url objects; no ambiguous strings.

Saved originals provide owned IDs, paths, actual MIME/dimensions/bytes/alpha/SHA256. Optional bounded
previews are derivatives. Local paths are not automatically accessible to cloud hosts. Prompts/input
bytes preserved; provider policy/terms still apply. Read [privacy](docs/PRIVACY.md).

## Documentation

[Configuration](docs/CONFIGURATION.md) · [OmniRoute](docs/OMNIROUTE.md) · [9router](docs/9ROUTER.md) ·
[Claude](docs/CLAUDE.md) · [Codex](docs/CODEX.md) · [Windows/WSL](docs/WINDOWS-WSL.md) · [Linux/macOS](docs/LINUX-MACOS.md) ·
[Architecture](docs/ARCHITECTURE.md) · [Testing](docs/TESTING.md) · [Requirements](docs/REQUIREMENTS.md) ·
[References](docs/REFERENCES.md)

## Logs and troubleshooting

New UTF8 JSON log per execution: `numera-image-gen-mcp_YYYY-MM-DD_HH-mm-ss_UTC-<pid>-<uuid>.log`, UTC
ISO8601 timestamp, INFO/WARNING/ERROR/DEBUG and component. Default directory configured/per-user;
14-day own runtime-log retention, initialization falls back stderr. Prompts/keys/auth/cookies/Base64/
signed query data/original paths not logged. Every accepted tool call records start/completion/failure,
run_id/call_id correlation, elapsed milliseconds and safe error code/stage; never arguments or raw
provider bodies. Event timestamp/level/component/run_id cannot be overwritten by context. DEBUG never
relaxes redaction. Attach only sanitized logs.

Catalog failure is not generation failure. Check route/base prefix/key/model before new authorization.
A text-only gateway cannot edit just because its model has native vision. Distinguish provider rejection,
partial stream, host timeout, OAuth/project/account problems, uncertain billing and storage permissions.
Never retry unknown generation automatically. Cancel waiting does not prove upstream cancellation/refund.

## License

[GPL-3.0-only](LICENSE), complete GPLv3 text. [Third-party notices](THIRD_PARTY_NOTICES.md).
Generated image/asset/model licensing is separate from software licensing.

## Donate

If this project helps you, donations are appreciated.

| Currency              | Network | Address                                            |
| --------------------- | ------- | -------------------------------------------------- |
| Bitcoin (BTC)         | Bitcoin | `bc1qmth5m03pu5hujw5xw5jmywam3jj3sqwqupesdt`       |
| USDT, BNB, USDC, etc. | BEP20   | `0x0Bd0BA443a8B9cf15922bf7f0Bb0a4b495fD06Ef`       |
| USDT, TRX, USDC, etc. | TRC20   | `TWBA3xFTqgZAeAYMxqo85xWnzvty3DcAhw`               |
| Ethereum (ETH)        | ERC20   | `0x0Bd0BA443a8B9cf15922bf7f0Bb0a4b495fD06Ef`       |
| TON                   | TON     | `UQCN8Umo_OfOWqImZetQsrNStPcmLkMAKajFyiCOhso23NDb` |
| Litecoin (LTC)        | LTC     | `ltc1qntqnnrunadurnw4cshv3qgspywrueyyeyngwuy`      |
| Solana (SOL)          | Solana  | `7B2wkczUjmkDhETwQuknBL8sUsbuV7nErxc317TmQuwR`     |
| Polygon (POL)         | Polygon | `0x0Bd0BA443a8B9cf15922bf7f0Bb0a4b495fD06Ef`       |
