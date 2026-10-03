# Configuration

## Simple setup (Windows, Linux, macOS)

Save `config.local.json` next to package.json: api_endpoint, api_key and models:[{id,name}].
Public root template: [config.local.json.example](../config.local.json.example); input schema
schemas/simple-config.schema.json. First model is default, exact IDs remain unchanged, name is a
display label. api_endpoint is any valid HTTP(S) API prefix, including custom deployment paths, not
a fixed localhost address. Do not append full operation endpoints. Default profile openai-images;
optional omniroute/9router select those explicit gateway contracts.

Each model can optionally set output_format (png/jpeg/webp), size and quality (nonblank strings up to
100 characters, native provider values). They become per-model defaults; explicit tool arguments win.
Omit a field to keep its provider default. Unsupported gateway options fail rather than disappear.
For Responses use [simple-responses.json](../examples/simple-responses.json), profile openai-responses
and orchestration_model (nonblank exact ID up to300 characters). This separate model drives the
Responses request; selected image ID/settings drive its single image_generation tool.
orchestration_model is rejected on other compact profiles to prevent silently ignored config.
Raw tools arrays/unknown fields are rejected; no hidden extra submissions.
No DPAPI/PowerShell/env setup required. Private inline key is plaintext: keep file ignored/out of
packages, restrict access (0600 on Linux/macOS, current-user ACL on Windows), never share it.
Default output/state/log/input directories sit beside that selected file, independent of CWD.
Save and reconnect the server to apply changes; no automatic paid operation or live hot reload.
Configured model listing is offline/unverified; refresh supplements provider facts, not entitlement.
Duplicate exactIDs, empty labels/key, unsafe endpoint and unknown fields reject before provider I/O.

Config selection: explicit --config > NUMERA_CONFIG > adjacent package config.local.json > per-user
config.json. Per-user: Windows %LOCALAPPDATA%/Numera/ImageGen; Linux/macOS
$XDG_CONFIG_HOME/numera-image-gen (fallback ~/.config/numera-image-gen). Explicit missing/invalid file
fails without silently choosing another. Do not create a user config inside node_modules for packed
installs: select your own private file with --config or NUMERA_CONFIG.

## Advanced configuration

Existing examples/config.json/schemaVersion1 remains supported, with schemas/config.schema.json.
Advanced output/state/log/input roots must be absolute; each named connection has independent
adapter/base/prefix/auth/model/defaults/policy. Use native adapters or explicit edit/workflow settings here.

Precedence: supported CLI flags, namespaced Numera environment, selected JSON, safe defaults.
CLI: --config, --default-connection, --output-dir, --state-dir, --log-dir, --log-level,
--request-timeout-ms, --max-concurrent-requests, --return-mode, --openai-compat.
Environment equivalent list is in `.env.example`; no automatic dotenv loading.
OPENAI_BASE_URL/KEY/MODEL apply only with --openai-compat, never unrelated connections.

Auth: none for explicitly trusted local backend; bearer or header with exactly one private apiKey, secretEnv,
secretFile (absolute/private/bounded) or secretDpapiFile (Windows, PowerShell 7, current-user DPAPI).
Set auth.origin to destination origin; changing base without changing the origin fails.
Environment variables are not encrypted. Keep real keys out of checked-in host/config files.
Run `scripts/protect-secret.ps1 -Path <absolute private path>` interactively only; hidden secret prompt,
no overwrites, user-scoped DPAPI and ACL restriction. The decryption subprocess explicitly emits
UTF-8 so non-ASCII values round-trip independently of the Windows console codepage.
Portable alternative: private0600 secret file. See PRACTICAL-TESTING.md for read-only readiness;
missing credentials are a blocker, not permission to read another application's private store.

Per-model maxCount/maxReferences/supportedParameters/capabilities overrides require actual contract
verification. Defaults use snake_case common names. providerOptionKeys permits bounded provider
options, never credentials/destination/model/prompt/image ownership overrides. Unsupported common
parameters fail; pixels (`size`), shape (`aspect_ratio`) and tiers (`image_size`) remain distinct.
Dedicated OpenRouter endpoint descriptors are checked before submission; a compatible endpoint is
pinned with provider.only and allow_fallbacks=false. Missing descriptors reject explicit requirements;
a failed/empty catalog reports unknown rather than blacklisting an explicitly selected model.

api-prefix preserves `/api/v1` or reverse-proxy prefixes. origin mode requires origin-only base and
adds native adapter prefix once. Full endpoints, query keys/credentials/fragments/traversal rejected.
Use separate query map for safe non-secret fields. Proxy is explicit connection-scoped, never global.

`files.assetAllowances` may narrowly allow origin/path-prefix local asset servers; configured local API
trust is not inherited by asset URLs. Defaults: input20MiB, output50MiB, aggregate100MiB,64M pixels,
14 references, preview512px/256KiB. Connection/model limits may be stricter.
