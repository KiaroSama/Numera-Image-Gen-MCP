# Implementation requirements

Owner assignment dated 2026-10-02; this is a faithful public requirements summary, not a copy of private working material.

## Product

Personal provider-independent TypeScript image generation/editing MCP server, executable/package
`numera-image-gen-mcp`, registration `numera-image-gen`, GPL-3.0-only. Keep host chat model unchanged.
Target repository https://github.com/KiaroSama/Numera-Image-Gen-MCP. Example checkout location: `C:/projects/numera-image-gen-mcp`; use your own absolute paths.

## Required release contracts

Generic Images-compatible generation/multipart/configured JSON edits; independent OmniRoute and
9router gateway profiles; Gemini generateContent; separate Gemini Interactions; OpenAI Responses
with separate orchestration/image tool models; dedicated OpenRouter Image API; explicit opt-in
chat-style image output; directly configured ComfyUI API-format workflow execution.

Ten tools: health_check, list_connections, list_models, get_model_capabilities, generate_image,
edit_image, get_job, cancel_job, list_outputs, get_output_info. Stdio production transport; no
unfinished HTTP mode. stdout only protocol. Prompt/source fidelity, all valid finals saved, original
bytes/hash/metadata checked, bounded optional derivative previews and owned resources.

## Safety and operational scope

Versioned schema/config, namespaced precedence, isolated credential sources, origin binding, DPAPI
option and portable private files/env. Never request keys in tools. Resolve prefixes once; bounded
read-only discovery/pagination. Capability/model/forwarding/account/evidence separate. Native batches
only, no hidden paid loops, unsupported refs/masks/options rejected before submission.

Persist request intent before submission; fingerprint/identity conflict detection and shared-process
coordination; no paid POST retry or uncertain replay/provider fallback. Cancellation and upstream
billing distinct. Path/realpath/byte/pixel/aggregate/reference limits; public asset DNS/redirect checks,
no credential forwarding or hidden upload service. Atomic output writes, private state/logs.

## Deliverables and evidence

Strict Node24 ESM build, pinned lockfile, complete licensing/notices, schema/examples, PowerShell7
setup/diagnostics/opt-in safe registration/removal with backups and WhatIf, separate Windows/WSL and
Claude/Codex docs. Offline unit/contract/real-client/packed install checks on Windows/Linux CI,
85 percent statement and80 percent branch coverage without broad exclusions. Requirement matrix,
work log, truthful compatibility/test results. Paid generation, GUI registration, GPU/model installs,
npm publication and public releases require separate authorization. Unavailable live checks not_run.

## Request updates

2026-10-02: owner corrected local root (spaced name), required moving all work/preserving assets and
removing empty wrong root. Completed relocation.
2026-10-02: owner requested GitHub About and topics; descriptive metadata and12 matching topics verified.
2026-10-03: owner requested simple one-file endpoint/key/multiple model IDs with display names,
merging completed side branches/larger logo, Linux/macOS usability and new platform-specific CI tests.
Inline key is private-only; defaults replace mandatory wizard/path/env setup, not privacy/protocol checks.
