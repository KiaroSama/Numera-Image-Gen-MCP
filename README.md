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
Windows323 tests pass/3 POSIX-only skipped; Linux/macOS325 pass/1 Windows-only DPAPI skipped.
Runtime coverage: at least90.06 percent statements and88.15 percent branches.
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

Copy [.env.example](.env.example) to .env next to package.json. Do not overwrite an existing private config.

Windows PowerShell:

```powershell
Copy-Item -LiteralPath .env.example -Destination .env
```

Linux/macOS:

```bash
cp -n .env.example .env
```

Edit .env: set API_ENDPOINT, API_KEY and MODEL_1_ID/MODEL_1_NAME. The template has three consistent
slots, each with ENABLED, OUTPUT_FORMAT, SIZE and QUALITY. Extra slots are disabled; fill their exact
IDs/names before setting ENABLED=true. At least one model must be enabled; the first enabled slot
is the default. Names are display labels, never routing aliases. Empty output fields preserve provider
defaults; explicit tool arguments override them. Use only native values your selected model supports.

```dotenv
API_ENDPOINT=https://gateway.example/v1
API_KEY=replace-with-your-private-key
PROFILE=openai-images
MODEL_1_ID=provider/exact-image-model
MODEL_1_NAME="My image model"
MODEL_1_ENABLED=true
MODEL_1_OUTPUT_FORMAT=
MODEL_1_SIZE=
MODEL_1_QUALITY=
```

The endpoint is any valid HTTP(S) API prefix: hosted, localhost or reverse-proxy, not a fixed port.
Do not include /responses or /images/generations. Generic openai-images is the default; explicitly
choose PROFILE=omniroute or PROFILE=9router for gateway-specific contracts. An endpoint/key pair
alone cannot turn a chat-only API into an image API.

For Responses use [simple-responses.env.example](examples/simple-responses.env.example):
PROFILE=openai-responses plus ORCHESTRATION_MODEL, separate from the selected image model.
Native Gemini/Gemini Interactions/OpenRouter Images profiles are also available; see
[configuration](docs/CONFIGURATION.md). Editing is enabled only by its verified native contract,
not guessed from an endpoint/model ID.

Outputs/state/logs/inputs sit beside the selected config. This plaintext-key file is ignored and
excluded from packages: never share, sync or commit it. On Linux/macOS restrict access:

```bash
chmod 600 .env
```

**Save and call list_models again: valid model/endpoint settings reload without restarting.**
Invalid or partially saved files keep the last valid configuration and emit a redacted warning.
Active jobs retain their original connection; reload never submits or retries an image request.
Storage/logging/input-root/global-policy changes and the bounded 64-snapshot limit require restart.
Legacy compact and advanced JSON remain supported for existing installations.

#### What each .env field means

Values may be quoted as in [.env.example](.env.example). An empty value (`""`) means unset, not
`auto` or zero. Boolean fields accept exactly `"true"` or `"false"`; `yes`, `1` and `0` are invalid.
Keep key names unchanged: unknown keys and duplicate assignments reject the saved configuration.

**Connection and API protocol**

| Field                 | Purpose                                                                                                                                                                                                                         | Required / default                                                                  |
| --------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| `API_ENDPOINT`        | Your image API's HTTP(S) base prefix, such as `https://gateway.example/v1`. Numera appends the operation path. Do not include `/images/generations`, `/images/edits` or `/responses`.                                           | Required. No fixed host or port.                                                    |
| `API_KEY`             | Private credential for that endpoint. Gemini profiles send `x-goog-api-key`; other compact profiles send bearer authentication. Never paste a real key into host config, prompts or public files.                               | Required, nonblank, without newline/NUL.                                            |
| `PROFILE`             | Selects the actual API contract. Accepted: `openai-images`, `omniroute`, `9router`, `openai-responses`, `gemini`, `gemini-interactions`, `openrouter-images`. Gateway profiles use Images with their specific forwarding rules. | Empty/omitted: `openai-images`. Choose the protocol your endpoint actually exposes. |
| `ORCHESTRATION_MODEL` | Exact conversation-model ID that drives a Responses request. It is separate from the image model selected through `MODEL_n_ID`.                                                                                                 | Required for `openai-responses`; leave empty for every other profile.               |

**Model slots: replace `n` with 1, 2, 3, ... up to 100**

The same six fields apply to every slot. Slots are ordered numerically; the first enabled model is the
default. A nonempty slot needs both ID and NAME, including disabled placeholders. At least one model
must be enabled, and IDs must be unique. A configured model is not proof of account entitlement.

| Field                   | Purpose                                                                                                                                             | Required / default                                                  |
| ----------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------- |
| `MODEL_n_ID`            | Exact provider model ID sent to the API, including any required provider prefix.                                                                    | Required for a nonempty slot; up to300 characters.                  |
| `MODEL_n_NAME`          | Friendly name the agent displays through `list_models`. It does not change routing.                                                                 | Required for a nonempty slot; up to100 characters.                  |
| `MODEL_n_ENABLED`       | Enables the slot. `false` hides it from configured listing and denies its ID for submission.                                                        | Empty/omitted: `true`. Template slots2 and3 explicitly use `false`. |
| `MODEL_n_OUTPUT_FORMAT` | Default output format: `png`, `jpeg` or `webp`, only if the selected route supports it.                                                             | Empty: provider default, nothing sent.                              |
| `MODEL_n_SIZE`          | Default native pixel dimensions, for example `1024x1024`, if accepted by that API/model. Numera does not resize the returned original to match.     | Empty: provider default; nonblank values up to100 characters.       |
| `MODEL_n_QUALITY`       | Default native quality value, for example `auto`, `low`, `medium` or `high` on models supporting those values. It is not a universal quality scale. | Empty: provider default; nonblank values up to100 characters.       |

Explicit image-tool arguments override these model defaults. Native Gemini uses `aspect_ratio` and
`image_size` tool arguments instead of Images-style SIZE/QUALITY; generateContent does not accept a
requested output format. Unsupported settings fail rather than silently disappearing.

**Optional reference-editing contract**

These fields describe a verified Images input contract, not a switch that grants the model editing
capability. Leave all five empty for generation-only use or when your route has not been verified.
Native protocols may already carry references independently; these fields cannot add native mask
support to an adapter that does not implement it. Any nonempty edit setting requires `EDIT_MODE`.

| Field                 | Purpose                                                                                                                                                                                                                            | Required / default                                      |
| --------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------- |
| `EDIT_MODE`           | `multipart`: upload reference bytes to `/images/edits`; `json`: inline references in JSON to `/images/edits`; `generation`: inline references through `/images/generations`. Explicit gateway contracts may override this mapping. | Empty: no explicit Images edit contract.                |
| `EDIT_ENCODING`       | Reference field shape expected by the route: `image`, `image[]`, `images`, `image_urls` or `input_references`. For multipart, use `image` or `image[]`; JSON/generation must match the provider's documented field.                | Required when EDIT_MODE is set.                         |
| `EDIT_MAX_REFERENCES` | Maximum reference count declared for the edit contract; other route/model limits can be stricter.                                                                                                                                  | Integer1–32; empty:1.                                   |
| `EDIT_MASKS`          | Declares verified mask forwarding. `true` alone cannot bypass an adapter/gateway restriction.                                                                                                                                      | Empty: `false`.                                         |
| `EDIT_MASK_POLARITY`  | Which mask pixels are editable: `transparent-edit` = transparent pixels; `white-edit` = white pixels; `black-edit` = black pixels. Must match the provider.                                                                        | Needed for mask/rectangle editing; no inferred default. |

For a verified standard Images edit/mask route, a typical setup is:

```dotenv
EDIT_MODE="multipart"
EDIT_ENCODING="image"
EDIT_MAX_REFERENCES="1"
EDIT_MASKS="true"
EDIT_MASK_POLARITY="transparent-edit"
```

Do not enable this example blindly on a gateway. `edit_region` and `target_language` belong to
`edit_image` tool arguments, not .env fields. [Editing examples](docs/IMAGE-EDITING.md) and
[advanced configuration](docs/CONFIGURATION.md) explain route limits and additional JSON settings.
Save .env and call `list_models` again to apply a valid change; no image is submitted just by saving.

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
        "C:/projects/numera-image-gen-mcp/.env"
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
        "ABSOLUTE_PATH_TO_NUMERA/.env"
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
claude mcp add --transport stdio --scope user numera-image-gen -- "$(command -v node)" "$PWD/dist/index.js" --config "$PWD/.env"
```

Windows PowerShell, using neutral example paths (replace them for your installation):

```powershell
claude mcp add --transport stdio --scope user numera-image-gen -- 'C:/tools/node/node.exe' 'C:/projects/numera-image-gen-mcp/dist/index.js' --config 'C:/projects/numera-image-gen-mcp/.env'
```

Project-scoped JSON alternative: [Claude project entry](examples/claude-project.json). Do not replace
the whole .mcp.json or unrelated host settings.

#### Codex

From the built checkout on Linux/macOS:

```bash
codex mcp add numera-image-gen -- "$(command -v node)" "$PWD/dist/index.js" --config "$PWD/.env"
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
args = ["C:/projects/numera-image-gen-mcp/dist/index.js", "--config", "C:/projects/numera-image-gen-mcp/.env"]
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
        "C:/projects/numera-image-gen-mcp/.env"
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
        "C:/projects/numera-image-gen-mcp/.env"
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
      - "C:/projects/numera-image-gen-mcp/.env"
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
        "C:/projects/numera-image-gen-mcp/.env"
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
        "C:/projects/numera-image-gen-mcp/.env"
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
        "C:/projects/numera-image-gen-mcp/.env"
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
node scripts/bounded.mjs 90000 20000 scripts/readiness.mjs --config "$PWD/.env" --probe
```

Windows PowerShell:

```powershell
node scripts/bounded.mjs 90000 20000 scripts/readiness.mjs --config (Join-Path $PWD '.env') --probe
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
bytes preserved unless explicit in-image translation or upscale instructions are requested; provider policy/terms still apply. Read [privacy](docs/PRIVACY.md).

## Image-to-image, comic and selected-region editing

Use edit_image with approved reference_images for image-to-image/comic edits and authorized watermark removal. Add target_language
for translation replacing text inside the image; add edit_region={x,y,width,height} for an absolute
pixel rectangle on the first reference. A verified native mask route is required for region editing.
Use upscale=true with one reference and a larger numeric size to ask the same image model for a
higher-resolution edit; verified reference/size support is required, and actual size deviations remain
visible. Unsupported masks/parameters fail before submission. No local output resizing is performed.
[Examples, coordinate rules and limitations](docs/IMAGE-EDITING.md). Actual translation quality needs
a real authorized sample; offline wire/mask tests are not semantic proof.

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
