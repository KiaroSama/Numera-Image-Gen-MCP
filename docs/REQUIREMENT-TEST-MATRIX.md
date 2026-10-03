# Requirement-to-test matrix

| Area                                                       | Assertions / evidence                                           |
| ---------------------------------------------------------- | --------------------------------------------------------------- |
| Config/precedence/isolation/origin/no-auth                 | tests/unit/config.test.ts                                       |
| URL prefix/origin/traversal/query                          | tests/unit/foundation.test.ts, tests/contract/discovery.test.ts |
| Catalog pagination/cache/partial/gateway differences       | tests/contract/discovery.test.ts                                |
| Capabilities/defaults/conflicts/unknown/unsupported        | tests/unit/config.test.ts                                       |
| Adapter native wire mapping/prompt/model fidelity          | tests/contract/adapters.test.ts                                 |
| Editing multipart/JSON/reference/native shapes             | tests/contract/adapters.test.ts, tests/contract/comfyui.test.ts |
| Gateway Antigravity text-only regression                   | tests/contract/adapters.test.ts, pinned source references       |
| Responses/native/binary/SSE final/error/preview            | tests/contract/streams.test.ts                                  |
| PNG/JPEG/WebP/bytes/pixels/alpha/masks/paths               | tests/unit/images.test.ts                                       |
| Owned atomic outputs/partial/deviation/hash                | tests/unit/store.test.ts, tests/unit/images.test.ts             |
| Request-ID/shared-state/restart/conflicts/admission/cancel | tests/unit/store.test.ts, tests/integration/mcp.test.ts         |
| No paid automatic retry                                    | tests/integration/mcp.test.ts and Generation HTTP contracts     |
| MCP real handshake/tools/call/stdout framing               | tests/integration/mcp.test.ts, scripts/smoke.mjs                |
| Packaging clean prod-only install/spaces                   | scripts/package-check.mjs                                       |
| Comfy workflow/bindings/upload/history/view/GPU guard      | tests/contract/comfyui.test.ts                                  |
| Live/GUI/GPU installed-host checks                         | not_run; separate authorization/fixtures needed                 |

POSIX-specific Linux/macOS: tests/integration/posix-platform.test.ts asserts0600/0644 credential
permissions, native Unicode paths, confined symlinks and bounded real process-group cleanup.

Simple setup: tests/unit/simple-config.test.ts validates private key/model IDs/names/portable roots
and configured vs refreshed catalogs; tests/integration/simple-config.test.ts launches actual stdio
from another CWD with adjacent saved compact file, exact first/explicit model forwarding, output bytes,
no replay and no key in protocol/logs. Hosted matrix also exercises macOS native dependencies/package.

Read TEST-RESULTS.md for executed evidence, not just test existence. Additional resumed contracts:
wire-contracts.test.ts (exact native HTTP), capability-descriptors.test.ts (endpoint requirements),
workflow-validation/workflow-branches (graph/node errors), incremental-stream (bounded frames),
continuation (private scope), detached-jobs (recovery), negotiation (actual protocol era), logging and
setup real-entry fixtures. Matrix presence alone is not a passing coverage claim.
Practical readiness public CLI: tests/integration/readiness.test.ts checks actual SDK tools, GET-only
probe, blocked credentials/catalog/model and safe summaries. Actual Windows same-user DPAPI/ACL:
tests/integration/dpapi.test.ts uses a non-secret prompt substitute, real protection/reader and Unicode.
Real gateway processes against controlled upstreams, actual interactive credential input, other-user
DPAPI denial, real GUI/GPU/live providers and Windows Server remain separate unverified scenarios.

Selectable endpoint/settings: compact tests cover custom deployment prefixes, generic default and
explicit gateway selection, Responses orchestration vs image model, exact output_format/size/quality
defaults and explicit overrides. Package check asserts root config.local.json.example inclusion,
valid public templates and private config exclusion. Synthetic8x6 image intentionally reports partial
when requested dimensions differ; originals remain unchanged.
