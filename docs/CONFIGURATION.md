# Configuration

Copy `examples/config.json` to an ignored local configuration; set an absolute `NUMERA_CONFIG`.
Generated schema: `schemas/config.schema.json`. Paths are examples, not portable defaults.
Default config location: Windows `%LOCALAPPDATA%/Numera/ImageGen/config.json`; Linux
`$XDG_CONFIG_HOME/numera-image-gen/config.json` (fallback `~/.config`). Output/state/log/input roots
must be absolute. Each connection has independent adapter/base/prefix/auth/model/defaults/policy.

Precedence: supported CLI flags, namespaced Numera environment, selected JSON, safe defaults.
CLI: --config, --default-connection, --output-dir, --state-dir, --log-dir, --log-level,
--request-timeout-ms, --max-concurrent-requests, --return-mode, --openai-compat.
Environment equivalent list is in `.env.example`; no automatic dotenv loading.
OPENAI_BASE_URL/KEY/MODEL apply only with --openai-compat, never unrelated connections.

Auth: none for explicitly trusted local backend; bearer or header with exactly one secretEnv,
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
