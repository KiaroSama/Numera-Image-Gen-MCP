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

Read TEST-RESULTS.md for actually executed evidence, not just test existence. Remaining source-review
branches, packing/setup/asset tests are added before the final integrated CI pass; matrix presence alone
is not a passing coverage claim.
