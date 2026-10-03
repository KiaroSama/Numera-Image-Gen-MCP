# Configuration

## Simple env setup (Windows, Linux, macOS)

Copy [.env.example](../.env.example) to .env beside package.json. Never overwrite an existing private
file. Node24 parses this file directly; no dotenv dependency or process environment mutation is needed.

- API_ENDPOINT: any valid HTTP(S) API prefix, including hosted/custom deployment paths. Do not append
  /images/generations or /responses. No fixed localhost address or port is assumed.
- API_KEY: private nonempty key, without newline/NUL. Quotes are useful for values containing # or spaces.
- PROFILE: openai-images (default), omniroute, 9router, openai-responses, gemini,
  gemini-interactions or openrouter-images. Gemini profiles use x-goog-api-key; others use bearer auth.
- ORCHESTRATION_MODEL: required only for openai-responses, separate from the image model.
- MODEL_1_ID/NAME/ENABLED/OUTPUT_FORMAT/SIZE/QUALITY through MODEL_100_*: exact native ID, display name,
  true/false enabled flag and optional native output defaults. Three consistent slots are provided.

At least one model must be enabled. First enabled slot is default; disabled IDs are denied and omitted
from configured listing. Empty output settings mean provider defaults; explicit tool arguments win.
OUTPUT_FORMAT accepts png/jpeg/webp. SIZE/QUALITY are native provider strings up to100 characters,
not local resizing instructions. IDs are unique; labels are never routing aliases. Fully empty slots
are omitted. Nonempty slots need ID/name even when disabled.

Duplicate/unknown keys, malformed assignments, invalid flags, invalid UTF-8 and files above1MiB reject
with safe errors. Comments/quoted Unicode and multiline names are parsed using Node util.parseEnv;
API keys cannot contain newlines. No raw tools arrays or hidden extra submissions.

Plaintext keys are not encrypted: keep .env ignored, out of archives/packages/sync, private0600 on
Linux/macOS and current-user ACL on Windows. Outputs/state/logs/inputs sit beside the selected file,
independent of the caller CWD. [Responses example](../examples/simple-responses.env.example).

## Save without restarting

Before each MCP tool call Numera reads the selected file, validates a complete candidate and publishes
its connection/model snapshot atomically. Call list_models with refresh=false after saving to display
new configured IDs/names. No provider call or paid operation is triggered by saving.

Invalid/partial saves keep the last good snapshot and log one sanitized warning per changed file.
In-flight operations retain their original configuration. Poll/cancel of existing jobs requires the
persisted endpoint/account identity; unavailable or ambiguous transport settings refuse network I/O.
At most64 snapshots are retained per process, including the initial one; further changes require restart.
Storage roots, logging, file/security limits and global concurrency policy also require restart.

Selection: explicit --config > NUMERA_CONFIG > adjacent .env > adjacent legacy config.local.json >
per-user config.json. Windows per-user root: %LOCALAPPDATA%/Numera/ImageGen; Linux/macOS:
$XDG_CONFIG_HOME/numera-image-gen (fallback ~/.config/numera-image-gen). Explicit missing/invalid file
fails rather than selecting a fallback. Packed installs should use their own absolute --config path,
not a private file inside node_modules. Selected .env/.env.* files use env parsing; JSON remains supported.

## Editing contract

Generic Images does not infer input forwarding from an endpoint/model ID. Enable only a verified
contract using EDIT_MODE (multipart/json/generation), EDIT_ENCODING
(image/image[]/images/image_urls/input_references), EDIT_MAX_REFERENCES (1–32), EDIT_MASKS (true/false)
and EDIT_MASK_POLARITY (transparent-edit/white-edit/black-edit). Nonempty edit fields require EDIT_MODE.

For an actual OpenAI Images edit contract, a typical configuration is:

```dotenv
EDIT_MODE=multipart
EDIT_ENCODING=image
EDIT_MAX_REFERENCES=1
EDIT_MASKS=true
EDIT_MASK_POLARITY=transparent-edit
```

Do not paste this into a gateway whose edit/mask route is unverified. Native Gemini image references
use its documented protocol without these Images fields; native masks are not implemented there.
[Editing and translation](IMAGE-EDITING.md) explains region coordinates and route limitations.

## Advanced JSON compatibility

[examples/config.json](../examples/config.json), schemaVersion1 and
[config schema](../schemas/config.schema.json) remain supported. Compact legacy JSON input uses
[its schema](../schemas/simple-config.schema.json). Named connections have independent adapter/base/
prefix/auth/model/defaults/policy, absolute roots, explicit edit contracts and ComfyUI workflow bindings.

Precedence: supported CLI flags > namespaced process environment > selected file > defaults.
CLI: --config, --default-connection, --output-dir, --state-dir, --log-dir, --log-level,
--request-timeout-ms, --max-concurrent-requests, --return-mode, --openai-compat.
Equivalent process environment: NUMERA_CONFIG, NUMERA_DEFAULT_CONNECTION, NUMERA_OUTPUT_DIR,
NUMERA_STATE_DIR, NUMERA_LOG_DIR, NUMERA_LOG_LEVEL, NUMERA_REQUEST_TIMEOUT_MS,
NUMERA_MAX_CONCURRENT_REQUESTS, NUMERA_RETURN_MODE. These are not compact .env keys.
OPENAI_BASE_URL/OPENAI_API_KEY/OPENAI_MODEL apply only with --openai-compat.

Auth is none for explicitly trusted local backends, bearer or header with exactly one private apiKey,
secretEnv, absolute/private/bounded secretFile or Windows current-user secretDpapiFile (PowerShell7).
auth.origin binds credentials to the destination. Environment variables are not encrypted.
Run scripts/protect-secret.ps1 interactively only; no overwrites. Its decryption subprocess emits UTF-8.
Missing credentials never authorize reading another application's store.

Per-model maxCount/maxReferences/supportedParameters/capabilities overrides need verified contracts.
Defaults use snake_case common names. providerOptionKeys permits bounded provider options, never
credentials/destination/model/prompt/image overrides. Pixels (size), shape (aspect_ratio) and tiers
(image_size) are distinct; unsupported parameters fail, never disappear. Dedicated OpenRouter endpoint
descriptors pin compatible provider.only with allow_fallbacks=false; failed discovery means unknown.

## Dimension discovery

`get_model_capabilities` returns `dimensions.preset_aspect_ratios` as a built-in suggestion catalog;
`presets_are_model_support=false` prevents treating every listed ratio as universally accepted.
`model_support` contains sourced native ratios, tiers, exact pixel sizes and `custom_dimensions.status`.
For unsupported/unknown native custom sizes it explains why; unknown model IDs have null native lists.
`route_support` separately reports request fields accepted by Numera, documented gateway tier limits
and the local max_pixels ceiling. These facts do not establish account entitlement or actual output.

The Gemini3.1 Flash Image native table includes14 ratios and0.5K/1K/2K/4K. For16:9 these produce
688x384/1376x768/2752x1536/5504x3072. Aspect labels and native pixels are discrete; do not calculate
exact dimensions from a ratio label or interpret4K as a universal video size. OmniRoute Antigravity
accepts only1K/2K/4K tiers through `image_size`; `size` is a gateway aspect lookup. Generic Images
accepts `size` but not undeclared ratio/tier fields; selecting a model does not change its profile.
For custom widthxheight, use only a route/model with a verified pixel contract and stay within local
pixel limits. Always inspect actual saved dimensions/deviations; no hidden resize or provider fallback.
Official source: https://ai.google.dev/gemini-api/docs/image-generation#aspect-ratios-and-image-size .

api-prefix preserves reverse-proxy prefixes; origin mode requires an origin-only base and adds the
native prefix once. Full operation URLs, credentials/query/fragments/traversal reject. Proxy is explicit
connection-scoped. Asset URL trust is independent of API trust. Defaults: input20MiB, output50MiB,
aggregate100MiB,64M pixels,14 references, preview512px/256KiB; route limits may be stricter.
