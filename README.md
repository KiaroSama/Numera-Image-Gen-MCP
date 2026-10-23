# Numera Image-Gen MCP

<div align="center">

<img src="docs/assets/logo.png" alt="Numera Image-Gen MCP logo" width="320" />

[![CI](https://github.com/KiaroSama/Numera-Image-Gen-MCP/actions/workflows/ci.yml/badge.svg)](https://github.com/KiaroSama/Numera-Image-Gen-MCP/actions/workflows/ci.yml)
[![License GPL-3.0-only](https://img.shields.io/badge/license-GPL--3.0--only-blue)](LICENSE)
[![Rust 1.99](https://img.shields.io/badge/Rust-1.99-CE422B)](rust-toolchain.toml)
[![Rust edition 2024](https://img.shields.io/badge/edition-2024-CE422B)](Cargo.toml)
[![Native executable](https://img.shields.io/badge/distribution-native_binary-blue)](Cargo.toml)
[![MCP stdio](https://img.shields.io/badge/MCP-stdio-7651a8)](docs/ARCHITECTURE.md)
[![Windows target](https://img.shields.io/badge/target-Windows_11-0078d4)](docs/WINDOWS-WSL.md)
[![Linux CI](https://img.shields.io/badge/CI_matrix-Linux-555)](.github/workflows/ci.yml)
[![JSON Schema](https://img.shields.io/badge/config-JSON_Schema-3068b7)](schemas/config.schema.json)
[![PNG JPEG WebP](https://img.shields.io/badge/images-PNG_JPEG_WebP-99cc00)](native/files/images.rs)
[![SQLite receipts](https://img.shields.io/badge/receipts-SQLite-003b57)](native/jobs/store.rs)
[![Documentation](https://img.shields.io/badge/docs-Markdown-blue)](docs/CONFIGURATION.md)
[![RMCP SDK](https://img.shields.io/badge/SDK-RMCP-7651a8)](Cargo.toml)
[![Support donations](https://img.shields.io/badge/Support-donations-d04a9a)](#donate)

</div>

A local provider-independent image generation/editing MCP server. Keep your host chat model unchanged;
image work is an explicitly selected tool operation. Named connections, separate OmniRoute/9router
profiles, native image protocols and configured GPU ComfyUI workflows. No paid POST retries, hidden
uploads, prompt enhancement or automatic provider/model fallback.

**Migration status:** all 470 frozen baseline cases and 50 suites have reviewed native assertion
mappings and applicable hosted execution. The retirement tree `b513884` passed
[run `39519759155`](https://github.com/KiaroSama/Numera-Image-Gen-MCP/actions/runs/39519759155):
all six native targets, strict quality checks and internal packaged SDK execution; coverage was
85.81% regions / 83.41% branches, above the unchanged 85% / 80% floors.
The tracked production implementation is Rust; Node is used only for the frozen CI oracle and SDK tests.
No native Release is published yet. Owner-authorized signing custody is restricted to the migration
branch; signing proceeds without a separate deployment review after validation. The six-target native signing and final signed-package checks must pass their own
exact-SHA CI before delivery; current internal artifacts are not final signed builds. The owner's working Node installation stays intact
until they change the host entry and verify the native connection.
See [migration acceptance](docs/testing/MIGRATION-MATRIX.md) and [historical test evidence](docs/TEST-RESULTS.md).

The native target also adds an optional direct HTTP MCP endpoint at `http://127.0.0.1:1969/mcp`,
a configurable port and a cross-platform native menu. It reuses Numera's own tools, not a remote MCP
proxy. Local authentication is separate from provider keys; `bridge` remains launcher-free.
[HTTP and menu setup](docs/HTTP.md) has provider-free hosted SDK/CLI verification; actual owner
registration and interactive GUI use remain unverified.

## Installation and setup

### 1. Download the ready native build

Once the native Release passes all gates, use this repository's
[Releases page](https://github.com/KiaroSama/Numera-Image-Gen-MCP/releases).
Normal usage needs **no Node.js, npm, Rust toolchain, Python, compiler or local build**.
Your MCP client runs the executable directly. ComfyUI remains an optional separately installed backend,
not a bundled runtime.

Choose the archive for your actual OS/CPU:

| OS      | CPU                       | Archive suffix                                                         |
| ------- | ------------------------- | ---------------------------------------------------------------------- |
| Windows | Intel/AMD 64-bit          | `x86_64-pc-windows-msvc.zip`                                           |
| Windows | ARM64                     | `aarch64-pc-windows-msvc.zip`                                          |
| Linux   | Intel/AMD 64-bit or ARM64 | `x86_64-unknown-linux-gnu.tar.gz` / `aarch64-unknown-linux-gnu.tar.gz` |
| macOS   | Intel or Apple Silicon    | `x86_64-apple-darwin.tar.gz` / `aarch64-apple-darwin.tar.gz`           |

Published compatibility and minimum OS versions must match that Release's actual native CI evidence.
A Linux binary is not proof that Claude Desktop itself supports Linux.

### 2. Verify and extract

Download the archive **and its `.sha256` sidecar** from the same Release. Match the SHA-256 before
extracting. The archive contains `bin/numera-image-gen-mcp` (Windows: `.exe`), license/notices,
configuration templates, schemas and examples. Extract into a writable installation folder you own;
do not overwrite an existing private configuration or outputs.

Windows PowerShell, replacing the downloaded filename:

```powershell
Get-FileHash -Algorithm SHA256 -LiteralPath './numera-image-gen-mcp-0.2.0-x86_64-pc-windows-msvc.zip'
```

Linux, from the download folder:

```bash
sha256sum -c numera-image-gen-mcp-0.2.0-x86_64-unknown-linux-gnu.tar.gz.sha256
```

macOS, replacing the archive filename for Intel if necessary:

```bash
shasum -a 256 -c numera-image-gen-mcp-0.2.0-aarch64-apple-darwin.tar.gz.sha256
```

Use your OS archive extractor. On Linux/macOS the tar archive preserves the executable permission.
Do not bypass an OS security warning blindly: verify origin/checksum and follow your platform's
normal permission process. Managed temporary files and caches stay inside the installation folder;
a non-writable managed path fails instead of falling back to a global temporary directory.

### 3. Create your private configuration

Copy [.env.example](.env.example) to .env in the extracted installation root (the parent of `bin/`). Do not overwrite an existing private config.

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

### 4. Register with your MCP client

Registration is one-time. Preserve unrelated settings and back up the host file before merging only
Numera's entry. After registration **no launcher needs to be opened**: the host runs the executable
with `args: ["bridge"]`, which starts MCP over stdin/stdout. No background daemon or listening port
must be started first.

#### Claude Desktop

Merge this into the actual `claude_desktop_config.json`; the paths are examples, not your machine's
installation. Keep every other `mcpServers` member unchanged:

```json
{
  "mcpServers": {
    "numera-image-gen": {
      "command": "C:/tools/numera-image-gen-mcp/bin/numera-image-gen-mcp.exe",
      "args": ["bridge"]
    }
  }
}
```

The executable finds `.env` first, then `config.local.json`, in its installation root (parent of `bin/`),
not Claude's working directory. Existing user-config fallback and an explicit absolute `NUMERA_CONFIG`
remain supported. If private configuration is elsewhere, add only this entry's `env`:

```json
{
  "NUMERA_CONFIG": "C:/private/numera/.env"
}
```

Never put the API key in host settings or arguments. On macOS use an actual absolute POSIX executable
path, such as `/Users/your-name/tools/numera/bin/numera-image-gen-mcp`; `args` stays `["bridge"]`.
Use Desktop's Edit Config to locate its actual settings, fully quit/reopen the app and call
`health_check` plus `list_connections` to confirm the selected configuration.
[Claude clients](docs/CLAUDE.md) has the registration and reconnect details.

#### Claude Code

Linux/macOS, from the extracted installation root:

```bash
claude mcp add --transport stdio --scope user numera-image-gen -- "$PWD/bin/numera-image-gen-mcp" bridge
```

Windows PowerShell, with your actual executable:

```powershell
claude mcp add --transport stdio --scope user numera-image-gen -- 'C:/tools/numera/bin/numera-image-gen-mcp.exe' bridge
```

Project JSON alternative: [Claude project entry](examples/claude-project.json). Do not replace the
whole `.mcp.json`. Save endpoint/key/models in the private application config and call `list_models`
again; accepted reloads retain operation snapshots and never resubmit existing image work.

#### Codex and other local MCP clients

Merge [Codex TOML](examples/codex.toml), changing its absolute executable/config paths and retaining
the long-operation timeout. Codex command registration from the extracted POSIX installation:

```bash
codex mcp add numera-image-gen -- "$PWD/bin/numera-image-gen-mcp" bridge
```

Cursor/Windsurf use their local `mcpServers` configuration. Kiro uses `mcpServers` with its `disabled`
flag. Hermes uses `mcp_servers`. VS Code-native `.vscode/mcp.json` uses `servers` and `type: "stdio"`.
For those clients set `command` to the native binary and `args` to `["bridge"]`; optionally add the
absolute private `NUMERA_CONFIG` in that member's `env`. Do not translate top-level keys between
clients blindly. OpenCode uses `mcp` with `type: "local"` and a command array containing the binary
and `"bridge"`. Existing host trust/approval and installation support remain the host's responsibility.

Optional native entry-scoped registration (never automatic at startup):

```bash
./bin/numera-image-gen-mcp register --client ClaudeDesktop --host-config /absolute/claude_desktop_config.json --config /absolute/numera/.env --dry-run
```

Review the dry run first. Removing `--dry-run` deliberately updates only Numera with an exact-byte
backup and concurrent-change protection. `--remove` removes only that member, never user images or
application data. Ambiguous TOML, links, active registration locks and invalid host JSON are rejected.
These instructions are not registration performed by this repository task.

### 5. Verify the connection before approving an image

In the host call `health_check`, `list_connections`, `list_models`, then `get_model_capabilities`
with the exact selected model ID. Readiness is not account entitlement or a successful generation.
An optional native read-only diagnosis is available:

```bash
./bin/numera-image-gen-mcp readiness --config /absolute/numera/.env --probe
```

The report distinguishes local configuration/storage/credentials from a read-only provider catalog;
it never submits an image. Logs go to stderr/files, never MCP stdout.

When you explicitly approve potentially billable work, ask: **Use Numera generate_image once to create
a blue mug beside an orange. count=1; request_id=my-first-image-001.** Reuse that ID for the same
logical request only. Never give an unknown outcome a new ID just to retry it.

Outputs keep the provider's original bytes and verified MIME/dimensions/hash. A bounded preview is
optional; a remote/cloud host cannot inherently read local paths or launch a local stdio executable.

### Update and troubleshoot

Download the next verified Release and checksum into this project's own download/work directory.
Preserve private config, originals, receipts/journals and migration snapshots. Quit/reconnect the MCP
host around replacing the executable; never replace data folders with a fresh empty installation.
Keep the old executable until the new host connection actually succeeds. Only then remove reviewed
obsolete project-local runtime/dependency/temp/cache paths. Never clear another project's or a
system/global cache.

| Problem              | Check                                                                                                    |
| -------------------- | -------------------------------------------------------------------------------------------------------- |
| Server not visible   | Actual absolute binary path, `args: ["bridge"]`, correct host file and trust/reconnect                   |
| Startup exits        | Private config validity, writable project-contained scratch and configured storage; inspect redacted log |
| No configured models | At least one enabled model with exact ID and readable name                                               |
| Catalog/probe fails  | Selected route, API prefix, credentials and account; this is not generation failure proof                |
| Unknown outcome      | Inspect the same receipt with `get_job`; do not submit a replacement automatically                       |

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

## Ratios, resolutions and custom dimensions

Ask the agent to call `get_model_capabilities` for the exact model. Its `dimensions` result includes
built-in ratio presets, sourced model-native ratios/tiers/pixel sizes, custom-size support status and
separate selected-route parameter acceptance. Presets alone are suggestions, not support claims.
Unknown model IDs have unknown native dimensions rather than invented universal4K support.

For documented Gemini3.1 Flash Image,14 ratios include1:1,16:9,9:16,3:2,2:3 and21:9; native tiers
are0.5K/1K/2K/4K. The complete per-ratio pixel table is returned by the tool.4K16:9 maps to5504x3072,
not3840x2160. The examined OmniRoute path accepts1K/2K/4K but not0.5K; generic Images does not accept
undeclared `aspect_ratio`/`image_size`. Arbitrary custom pixels are not a native Gemini contract:
select a documented ratio+tier, or use an independently verified custom-size model. Numera never
silently resizes output to match. [Dimension selection details](docs/CONFIGURATION.md#dimension-discovery).

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
