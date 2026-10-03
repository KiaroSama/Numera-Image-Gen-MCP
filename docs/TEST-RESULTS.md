# Test results

Verified2026-10-02: hosted CI passed on exact code SHA
`f34afdc3d2832d26a8dd2de864b04f17aa7f2b49`.
[CI run36949992617](https://github.com/KiaroSama/Numera-Image-Gen-MCP/actions/runs/36949992617).

| Check                                     | Observed result                            | Environment                                          |
| ----------------------------------------- | ------------------------------------------ | ---------------------------------------------------- |
| Offline suites                            | 237 tests passed across12 files on each OS | hosted ubuntu-latest and windows-latest, Node24.21.0 |
| Runtime statement coverage                | 90.94 percent on both OSes                 | all first-party src/**/*.ts included                 |
| Runtime branch coverage                   | Linux90.77 percent;Windows90.67 percent    | gates85 statements/80 branches passed                |
| Typecheck/lint/build/UTF8/source size     | passed                                     | both CI jobs                                         |
| Formatting                                | passed                                     | both jobs after explicit LF checkout correction      |
| Clean packed production install/spaces    | passed,10-tool real-client handshake       | both CI jobs                                         |
| Runtime dependency audit                  | 0 vulnerabilities reported                 | both CI jobs                                         |
| Seven review regressions                  | 10 targeted tests passed;included final CI | no review rerun                                      |
| Real stdio two-process dedup              | passed,one POST/exact output bytes         | loopback fixture, not paid provider                  |
| Setup JSON/TOML entry merge               | passed                                     | isolated fixtures, not user's host settings          |
| PowerShell setup WhatIf                   | passed                                     | local Windows11 Enterprise, no changes               |
| Paid/live gateway/provider generation     | not_run                                    | no endpoint/model/count/cost authorization           |
| Actual Claude/Codex GUI                   | not_run                                    | no host registration/mutation authorized             |
| ComfyUI installed GPU/checkpoint workflow | not_run                                    | no approved real backend/workflow                    |
| Windows Server2025 installation           | not_run                                    | no such test host                                    |

Initial CI failed Windows formatting because Git converted LF to CRLF. Root fix is committed
.gitattributes; all replacement checks passed. No test weakening or exclusions.

## Resume 2026-10-03

The table above is baseline evidence, not verification of the resumed changes. Resumed implementation
adds endpoint descriptor validation, ComfyUI graph/enum/link preflight and node-error warnings,
private continuation metadata, bounded incremental SSE, detached-job finalization coordination,
maintenance logging and locked fixture-only registration lifecycle. Compatible SDK upgraded2.3.0,
ESLint10.12.0; TypeScript6.0.3 retained because actual peer range rejects7.0.2.

Local seam checks passed for descriptors/private continuation/stream/workflow/detached jobs and
protocol negotiation. Setup executor's narrow fixture checks passed. Latest integrated typecheck/lint/
build/UTF8/format checks passed. Updated full CI passed on code SHA
`f640606e664ef1c397e7e5758fd70bf5c045d58c`:
[run37068503319](https://github.com/KiaroSama/Numera-Image-Gen-MCP/actions/runs/37068503319).
Both OSes:260 tests across21 files;89.55 percent runtime statements,87.89 percent Windows branches /
87.97 percent Linux branches. Clean packed installation, formatting, build and audit passed;
0 runtime advisories. Every bounded supervisor reported cleanup_verified=true.
An earlier resumed run failed generated-schema formatting only; formatted schema replacement passed.

Additional not_run: controlled real gateway processes against fake upstreams, interactive DPAPI/ACL
creation and exact Windows Server installation. Conversational continuation replay is not exposed;
stateless reference editing remains default. Direct stdio serves legacy negotiation; modern-only pin
is rejected and not advertised. Rich ambiguous TOML is refused for manual entry-scoped editing.
No all-model/quality/production-ready claim.

## Practical preparation 2026-10-03

Final practical code SHA `62d955611845c0b1d1a67cff609430c5b5a5130d` passed
[CI37074134568](https://github.com/KiaroSama/Numera-Image-Gen-MCP/actions/runs/37074134568).
Windows:265 tests/23 files;90.06 percent statements,88.28 percent branches. Linux:264 passed,
1 Windows-only DPAPI test visibly skipped;22 files passed/1 skipped,89.55 statements,87.97 branches.
Build/typecheck/lint/UTF-8/format, clean packed production install and audit0 passed on both OSes.
New readiness CLI and GET-only/blocked-state fixtures passed in that matrix. No full suite repeated locally.
Local light seam: real SDK readiness success fixture passed; actual Windows DPAPI same-user protection,
restricted ACL/decryption, corrupt ciphertext and no-overwrite fixture passed. The Unicode fixture first
failed with Japanese becoming???; explicit PowerShell UTF-8 output fixed the shared credential reader.
Native typecheck/build/lint passed locally; final integrated checks passed in the exact-SHA CI above.
All bounded owners reported cleanup_verified=true; local PID/creation-time sweep found zero survivors.

Actual owner's local connection:10-tool protocol, writable storage, missing OMNIROUTE_API_KEY -> exit2.
Bounded unauthenticated GET to local20100/v1/models returned401. No private gateway state was read,
no paid POST/upload, host registration or model install was performed. No listeners20128/8188 observed.
Practical commands/requirements and distinction from paid/GUI/GPU proof: PRACTICAL-TESTING.md.
Controlled real gateway fake-upstream integration remains blocked without a fully isolated writable
artifact and network confinement; shared DATA_DIR does not isolate Next.js cache writes. No docker/
podman available in the inspected PATH; shared user gateway is left untouched.
Actual interactive/other-user DPAPI, paid providers, GUI host, real GPU workflow and Server2025 remain
not_run. Ready-to-run preparation is not a claim that these missing scenarios were executed.

## Simple setup and platform delta2026-10-03

Compact endpoint/key/models configuration added with local file defaults and display-only model names.
Local narrow load/catalog/actual stdio seams passed; original native IDs and bytes survive, key stays
out of receipts/logs and duplicate request does not POST again. Final code SHA
`5fba8adbbf3b3f0c9c9d6391ddf42b5ed3ec7489` passed
[CI37078653969](https://github.com/KiaroSama/Numera-Image-Gen-MCP/actions/runs/37078653969).
Windows286 passed/3 POSIX-only skipped; Linux/macOS288 passed/1 Windows DPAPI skipped.26 files
are discovered;25 passed/1 platform-only file skipped per OS. Linux/macOS3 new native platform
assertions passed (0600/0644 private credentials, symlink containment, owned process-group cleanup).
Windows coverage90.17 statements/88.56 branches; Linux/macOS89.67/88.26. Build/type/lint/UTF8/format,
nativeSharp/SQLite/stdio, packed clean production install and audit0 passed on all three platforms.
No IntelMac/GUI/live provider entitlement claim. Owned supervisors reported cleanup_verified=true.
Merged logo/task branches removed; only incompatible failing DependabotTS7 branch/PR remains unmerged.

## Strong logging delta2026-10-03

Final runtime `69816338da5bde6b6d0dda194b754322c344b32a` passed
[CI37079856441](https://github.com/KiaroSama/Numera-Image-Gen-MCP/actions/runs/37079856441) on all3OS.
Windows287 passed/3 POSIX skipped,90.21 statements/88.58 branches; Linux/macOS289 passed/1DPAPI skipped,
89.71 statements/88.28 branches.26filesdiscovered25pass1platformskip. Build/lint/types/UTF8/format,
packed native production install/audit0 passed. Logging cases assert immutable run/event metadata,
paired real tool start/success/error, duration/call correlation, released handlers and no key/prompt.
No raw provider body or arbitrary exception stack logged; safe error code/stage retained.
Earlier platform proof above remains valid history, not the latest runtime SHA.

## Selectable settings delta2026-10-03

Local narrow checks: generic default red/green, compact per-model settings red/green, actual SDK stdio
Images and Responses defaults/override contracts passed. Tiny fixture dimensions deliberately differ
requested size, correctly yielding partial while saved originals match. New public root template and
Responses example are checked by packed installation in final hosted CI; previous logging run above
is not evidence for this new code. Incompatible TypeScript7 bot update was closed without merging,
its remote branch deleted; compiler6.0.3 retained under the verified eslint peer constraint.

Final code `6c4cef2869e17398684787a828ec4720de6886b9` passed
[CI37082578573](https://github.com/KiaroSama/Numera-Image-Gen-MCP/actions/runs/37082578573).
Windows297 passed/3 POSIX skipped,90.24 statements/88.61 branches; Linux/macOS299 passed/1 DPAPI
skipped,89.74/88.31.26filesdiscovered25passed1platformskip. Types/lint/build/UTF8/format, clean
production packed installation with valid public templates/private exclusions, native modules/stdio
and audit0 passed all3OS. All bounded supervisors reported cleanup_verified=true. No full suite
duplicated locally; no paid/GUI/GPU claim.

## Owner-authorized real generation2026-10-03

One explicitly approved generate_image call, count1, configured default antigravity/gemini-3.1-flash-image
through the selected Images-compatible endpoint completed. Saved JPEG1024x1024,484496 bytes; actual
decode/dimensions/SHA256 matched receipt and visual inspection showed the requested blue mug/orange.
Provider/client operation10.99s, outer owner12.53s, cleanup_verified=true. No retry/fallback or host
configuration mutation. Output retained locally and delivered; no real key, private endpoint or image
is published. This proves only one selected model/generation; editing/other routes remain untested.

## Portable client and model-slot verification2026-10-03

Final code SHA `1dbd9797696c4b30757d014dc9111113e6da7180` passed
[CI37088483026](https://github.com/KiaroSama/Numera-Image-Gen-MCP/actions/runs/37088483026).
Windows299 passed/3 POSIX skipped,90.28 statements/88.63 branches; Linux/macOS301 passed/1 DPAPI
skipped,89.79/88.33. Types/lint/build/UTF8/format, native packed installation and audit0 passed.
Active/disabled model slots and null-as-provider-default assertions passed. Actual read-only SDK
list_models returned the active configured name/ID without generation. Client configuration snippets
were parsed and checked against official docs; new client GUIs were not executed. Public examples
use neutral paths; source runtime preserved during the scoped historical path replacement, before
the separate model-slot change. Earlier run links remain historical evidence on their original SHAs.
