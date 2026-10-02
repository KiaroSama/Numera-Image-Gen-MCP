# Practical testing

## Prepared checks

The readiness command uses a real SDK stdio client and only read-only tools. It never calls
image generation/editing, uploads, job cancellation or host registration. It creates normal local
Numera state/log/storage directories. Logs use the shared UTF-8 UTC maintenance logger; raw provider
errors, catalog/model strings, credentials and config paths are excluded from its summary.
Use the existing bounded owner (90-second wall / 20-second idle ceiling):

```bash
node scripts/bounded.mjs 90000 20000 scripts/readiness.mjs --config "C:/projects/numera-image-gen-mcp/config.local.json" --connection default --probe
```

The shown configuration path is the owner's Windows checkout, not a portable default. On other
machines replace it with the actual absolute configuration. Omit --probe for provider-free local
checks. Add --model <exact catalog ID> for a scoped capability call; the command never selects a
model for generation. Model presence is checked only in the first bounded catalog page (up to100);
absence is a blocker for this conservative check, not proof the provider cannot generate with it.

Exit0: local storage/protocol and declared credentials ready; if probed, catalog nonempty/complete.
Exit2: readiness blocked. Exit1: invalid invocation/configuration or maintenance failure.
connection_ready=null means not probed; generation_verified is always false. Catalog visibility and
configured forwarding evidence do not prove account entitlement, cost, output quality or editing.

Compact setup uses connection name default and private api_key in config.local.json; no environment
or DPAPI steps are required. Advanced config retains independent existing secret sources. Reconnect
after saving. The first configured model defaults; display labels never replace exact model IDs.

## Earlier local observation, 2026-10-03

Windows11 Enterprise, Node24.21.0. Private local config was prepared from the existing example with
only OmniRoute enabled, installed gateway version3.8.51, absolute project storage/input roots,
no guessed default model and no secret values. The connection declares OMNIROUTE_API_KEY.
Actual stdio readiness:10 tools, local_ready=true, credentials_ready=false, exit2.
A separate bounded unauthenticated GET /v1/models returned401 from the already-running local gateway
on20100. No generation was submitted and no private gateway credential store was inspected.
No listener was found on20128 or8188 at inspection; this is not proof software is uninstalled.

Before a real catalog probe, set api_key privately in the compact config, or use the declared
secret source for advanced config. Never paste
keys into prompts, tool arguments, source, screenshots or public host config. No automatic dotenv
loading. Existing Desktop processes do not inherit a newly set terminal environment.

## Windows DPAPI evidence

The persistent Windows fixture executes protect-secret.ps1 with only Read-Host replaced by a
non-secret value. Encryption, file creation, icacls restriction and Numera's decryption reader run
for real. It checks current-user-only protected ACL, non-ASCII UTF-8 round trip, ciphertext not
containing plaintext, corrupt ciphertext rejection and existing-file refusal. The Unicode fixture
first reproduced codepage loss; credentials now sets Console.OutputEncoding to UTF-8 explicitly.
Linux visibly skips this Windows-only case. This does not test another user's decryption denial,
actual human input, domain/service accounts or MSIX host identity differences.

## Controlled gateway and live boundaries

Gateway-shaped HTTP fixtures are not real gateway-process integration. Shared installed gateways
must not be reconfigured or reused for fake upstreams. A complete controlled integration requires
an isolated gateway artifact/state/home/cache, no real accounts, loopback fake upstream, blocked
external egress, bounded owner and zero process survivors. DATA_DIR alone is insufficient if the
framework writes cache next to shared installed code. No unsupported network-isolation claim.

Paid/live acceptance needs an explicitly approved connection, exact model, maximum submission count
and cost/quota permission. Use count1 first, one fresh request_id, no automatic retry or fallback.
After uncertain outcome inspect get_job; do not submit a replacement just because the host timed out.
Validate produced file MIME/dimensions/hash and visual fidelity against the actual prompt.

Reference edit: use a real approved PNG/JPEG/WebP in the configured inputs directory or owned output;
choose a route with verified forwarding. Do not use Antigravity reference edits at the examined
versions that discard image inputs. Mask tests need actual alpha/dimension/polarity-compatible input.
No sample user image was available for live semantic editing in this task.

## Host and GPU handoff

Host registration remains opt-in: select the exact Claude Code/Desktop/Codex config, use setup
-WhatIf first, then the explicit Register action; preserve backup/unrelated settings. Existing
examples and docs/CLAUDE.md, docs/CODEX.md provide entry shapes. Reconnect the host and call
health_check/list_connections; actual GUI preview rendering is separate from SDK success.
No host settings were changed by preparation.

ComfyUI requires a reachable explicitly selected GPU backend, installed checkpoints/nodes, exported
API workflow and explicit bindings. No model/custom node/runtime installation or unapproved service
start is performed. Windows Server2025 needs an actual test host; hosted Windows CI is not that proof.
These acceptance paths remain not_run until their access/authorization/real inputs exist.
