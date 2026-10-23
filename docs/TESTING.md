# Testing

## Native Rust migration — baseline acceptance

At retirement tree `b513884` / run `39519759155`, all ten jobs and 103 native harnesses in each of six
stable platforms plus nightly passed, including internal extracted SDK checks and unchanged 85/80 gates.
Coverage was 85.81% regions / 83.41% branches. All 470 cases and 50 suites have reviewed mappings
and applicable execution evidence; final signatures and later signing changes require separate proof.
Seven fifteen-input bundles bind exact artifact digests, bytes, source/helper hashes and named runs. Twelve SDK transport cases,
Comfy error conversion, tier-message and strict POSIX PID-absence assertions passed. See
[the migration matrix](testing/MIGRATION-MATRIX.md#release-blockers). Older observations below
are historical, not current failures: `05ed820` measured78.33/70.93 and `b4e466a`81.87/76.67 below
coverage floors. At `26d4224`, Windows x64 DPAPI failed before script entry with zero CPU/output; `49dde41` passed both Windows DPAPI and PSS snapshot tests without changing bounds. Its historical intermittent startup cause remains unresolved. Native verification is hosted-only.

The exact Node baseline generates independent config/canonical/image/policy/wire oracle values in CI;
[maintained validation](testing/SOURCE-VALIDATION.md) is separate. Native tests compare nonempty original fixtures. Real child
`bridge` tests launch without a launcher from another CWD, list all ten tools and require bounded EOF
shutdown. The hosted interoperability fixture also exercises two actual SDK hosts sharing source-seeded
SQLite receipts and restarts both twice, preserving original bytes and asserting that recovery sends
no generation POST. Transport/auth/poll errors remain distinct from terminal provider states. This
fixture passed on all six targets at `3f27fb9`, using 25 source-seeded jobs, not live providers.
The coverage workflow instruments the same external acceptance path before generating its report;
a passing external fixture alone does not establish the coverage floor. The next candidate adds
source-generated final SSE/result normalization, actual multipart capture, targeted cancellation,
public generation/editing, output recovery and conservative registration fixtures. These additions
passed their selected runtime paths in the next hosted candidate, except a new normalization oracle
exposed an unwanted `usageMetadata` fallback; that production drift is being repaired. Complete parity
and coverage acceptance remain pending. Direct HTTP/menu adds three further Rust targets and an actual
two-client HTTP SDK fixture, now exercised in hosted CI. The next expanded tree contains 38 targets,
including config, expiry, continuation/recovery, native entry-point and Comfy result cases. Those
additions passed selected runtime checks in `b4e466a`; archive verification is still gated by strict
quality checks. The next tree adds bounded admission/Comfy completion, existing-job recovery, image
route/deviation, API preflight and real filesystem/SQL publication-gap fixtures (43 Rust targets).
These newest additions remain pending exact-SHA acceptance. Raw-text JSON oracle checks now cover
JavaScript nonfinite numbers, lone UTF-16 values/keys, duplicate insertion order and binary64 rounding.
All nine tests passed on all six targets at `dc0fb4c`, together with the new UTF-16 access and direct
schema suites. The Windows HTTP cleanup regression also passed after an actual handler-release barrier
was added, without a cleanup retry or longer sleep. All 46 suites, strict Clippy and actual stdio/HTTP
SDK acceptance passed on that revision; formatting still failed. Actual coverage measured 84.43%
regions (below85%) and 80.91% branches (above80%). These are prior-revision observations, not current
integration acceptance. The authoritative value model is now being connected through configuration,
requests, adapter payloads, provider results and persistence. Typed request extensions preserve raw
tool arguments outside SDK serde metadata; boundary codecs restore UTF-16 request IDs and structured
results for both stdio and HTTP. These integration paths and new raw SQLite TEXT compatibility cases
remain pending hosted verification. Coverage reports are retained even after instrumented test
failures; those failures still fail the required gate. The first integrated revision `ed07460` failed
native compilation on two shared type-boundary errors; its tests and coverage are not passing evidence.
Those errors are being repaired together using actual hosted diagnostics and formatter/lock artifacts.
Further acceptance fixtures cover real killed-child checkpoints, queued credential snapshots, two-process
journal budgets, captured edit masks, catalog pagination, malformed Comfy uploads and hanging-response
socket cleanup. Their authored existence is not execution proof or a full 470-case mapping. Revision `4c9e8c7` still
failed native test compilation and strict linting; its partial instrumentation report is not a complete
coverage measurement. Those exact type/lint diagnostics are repaired in one replacement batch.
The Windows source fixture also incorrectly required numeric PIDs never to repeat after reaping;
process-instance checks now use the actual child's immutable startup-log `run_id`, while simultaneous
PID distinction, bounded shutdown and every original lifecycle assertion remain required. Native
stdio negotiation separately preserves the frozen source's legacy/auto behavior and modern-pin
rejection; the optional HTTP transport retains its SDK-supported modern protocol behavior. SQLite
fixture stores, loopback services and owned child processes must close and clean up.
Revision `da6056f` passed all three Node jobs but failed native quality/runtime gates. Its failed
instrumented run measured 83.51% regions / 80.40% branches, not a complete acceptance measurement.
Repairs preserve empty SSE priming events, select the actual response by ID, restore the source's
method-not-found bootstrap fallback, compare canonical Windows paths and construct crash/provider
fixtures through their real admission and allowlist contracts. Revision `ed07a39` passed strict Clippy,
all 54 native targets on POSIX and actual stdio/HTTP SDK acceptance; Windows exposed a published-target
fixture path mismatch. Formatting still failed, and a complete instrumented run measured 84.15%
regions / 80.91% branches. The next tree uses the real canonical publication path and adds actual
HTTP output/preview/resource/error/existing-handle contracts. At `3655fdc`, all 55 native targets, strict Clippy and actual stdio/HTTP SDK acceptance passed on
all six targets, including the Windows crash fixture and public output/resource test. Formatting
still failed in two test files. Complete coverage reached 84.63% regions / 81.10% branches, still
below the region floor. At `8271b81`, the retained original-route, ambiguous-history refusal and
credential-free recovery regression passed. Five stable platforms passed all 55 runtime targets;
Linux x86_64 hit the existing eight-variant asynchronous polling fixture's aggregate eight-second
deadline. All six targets passed strict Clippy and actual SDK stdio/HTTP acceptance. Formatting
failed in one test file. Complete nightly coverage measured 84.72% regions / 81.17% branches: the
region floor still failed. The repair isolates every adapter/state variant as a named harness case
without changing the real polling interval, eight-second per-case ceiling or two-worker cap. New
Comfy fixtures capture distinct reference/mask bytes and workflow targets, zero-upload preflight
failures, GPU refusal/opt-out and completed-original download recovery. At `bc34fbf`, all 56 native
targets, strict Clippy and SDK stdio/HTTP acceptance passed on all six native targets. The polling
fixture now ran 14 cases and passed; Linux x86_64 target duration was 3.02 seconds, versus the prior
8.32-second failed aggregate run. This is an observed run, not a repeated performance benchmark.
Two test files still failed formatting. Complete nightly coverage measured 84.86% regions / 81.42%
branches, still below 85% regions. The Node macOS job separately failed its retained maintenance
runner's cleanup-verification record; original cleanup assertions remain required. The next repair
adds named phase/errno diagnostics and actual close-event identity rather than probing a reusable
numeric PID. Real process-group disappearance remains mandatory within the existing two-second bound.

The accepted [source mapping](testing/native-parity.json) records all 50 frozen suite hashes and
verified native case unions. The following paragraphs retain earlier, superseded acceptance states.
Hosted case collection uses the frozen Vitest runtime to expand declaration loops without running test
bodies. Unlike CLI `list` JSON, it retains skipped platform cases with their names/locations/modes.
Collection must produce exactly 50 suites / 470 cases and close its pool under a 60-second wall /
30-second idle ceiling with two workers. At `3c5958f`, collection produced all 470 cases on Windows,
Linux and macOS with identical case identities and the expected three/one/one platform skips. All
56 native targets, strict Clippy and SDK stdio/HTTP acceptance passed on all six targets; the three
Node jobs also passed, including the cleanup regression. Complete coverage passed unchanged floors
at 85.25% regions / 82.18% branches. One native test-file formatting correction still failed and
prevented archive checks. Raw collection is evidence for mapping, not native assertion equivalence.
Full case-ledger and archive acceptance are still required. At `0870b9e`, formatting, strict Clippy,
all 56 native targets, release builds and SDK stdio/HTTP acceptance passed on five targets. Intel
macOS stopped before those checks when the official toolchain download hit its 30-second socket
read timeout. All three Node jobs and complete 85.25% / 82.18% native coverage passed. Five native
jobs then failed actual dependency-notice preservation; archives and extraction remained unexecuted.
A locked `alloc-stdlib 0.3.0` archive deterministically omits its upstream BSD notice. The repair
pins the exact original notice and provenance, retains all normal notices, and fails unknown missing
notices with sanitized package/code evidence. Official public downloads retain the 30-second socket
idle bound, discard partial timeout attempts, and run under 90-second wall / 35-second idle ownership;
checksums remain mandatory before execution. Offline tooling contracts are run in hosted stable jobs.
Six independent provider HTTP cases are now authored for exact POST/path/auth/Unicode body and original
normalization. At `0f80e3b`, all six provider cases, all 57 native targets, strict Clippy,
release builds, five offline tooling contracts and SDK stdio/HTTP acceptance passed on all six
platforms. Complete coverage remained 85.25% regions / 82.18% branches. One provider fixture
formatting correction still blocked packaging, so the real notice collector and extracted archives
remain unverified. The Linux Node job separately hit its retained PowerShell fixture's ten-second
idle deadline; the other two Node jobs passed. These runtime results are not complete source-case
equivalence. The retained PowerShell fixture now forwards real child output as it arrives and emits
bounded parse/action phase evidence instead of withholding all progress behind `spawnSync`. Its
12-second wall / 10-second idle ceilings and all syntax, WhatIf and no-mutation assertions remain.
The original slow phase is unproven; replacement three-OS execution is required.
The [case ledger](testing/native-cases/index.json) now retains every one of the 470 actual identities,
frozen source hashes and three-platform modes. All remain pending. Targeted cases have literal
source/native assertion ranges and explicit schedule gaps; neither a passing suite nor a completed
boolean can close them. Hosted structural validation compares the ledger with fresh frozen runtime
collection. Full acceptance requires reviewed assertions, no gaps, unchanged native source hashes
and actual applicable native execution evidence. At `3d79621`, formatting and every stable native
quality/runtime/SDK gate passed on all six targets; all three Node jobs and unchanged coverage floors
also passed. The 470-case structural ledger matched fresh collection on all three OSes, while all
470 assertion dispositions remained pending. All eight offline tooling contracts passed on six
targets. Actual notice collection reported only `rmcp 3.5.1` missing its packaged license; its
complete exact-revision upstream notice is now preserved, including the licensing-transition terms.
Archives and extracted execution still wait for successful notice preservation. Seventy configuration
cases now have individually recorded oracle-row/direct-test navigation and exact gaps, rather than
suite-wide coverage claims. Three newly authored directory race unit tests hold two real missing-path
observations before regular-file or link insertion, including an actual Windows junction. They retain
the production metadata, mkdir and containment checks through a private observation seam; no public
test switch, wider path permission or new dependency is added. At `ec60ae3`, all three race cases
passed on four POSIX targets. Both Windows jobs failed only the junction-creation command: the fixture
passed a canonical verbatim path to the shell, but the exact command failure cause remains unproven.
The correction uses its owned non-verbatim parent CWD
and relative junction basename without changing production containment or the two-second child bound.
All three Node jobs passed. Complete coverage passed at 85.37% regions / 82.20% branches. Actual
notice preservation, allowlisted archives, checksums and extracted stdio/HTTP SDK execution passed
on all four POSIX targets; Windows packaging remained gated by that fixture failure. These are hosted
artifacts, not a verified Release or complete 470-case assertion acceptance.

The next tree explicitly registers 59 integration targets. New configuration acceptance cases preserve
exact frozen invalid-input/profile/precedence/compact-default assertions. A pinned-source requirements
oracle covers six edge scenarios and all 56 Flash / 30 Pro documented pixel pairs. Native comparisons
retain exact deviation/warning strings; normal finalization and real SQLite commit-failure recovery
verify original bytes, stable output identity, persisted requirements and repeated public reads.
An opaque-tier recovery case requires exactly one source warning. Recovery deliberately retains
`storage_error` and `partial_or_failed` storage outcome, rather than implying clean storage. These
new cases and the Windows command correction are source-only until replacement hosted verification.
Metadata comparisons and JSON round trips are not image inference or process-restart evidence.
At `3dc882a`, all four requirements tests passed on all six native targets, including exact warnings,
normal publication and SQL-failure local recovery. Strict Clippy, release builds and SDK stdio/HTTP
acceptance passed on all six targets; all three Node jobs passed. The new compact Responses fixture
incorrectly compared the entire schema-defaulted model override instead of the source's requested
settings subset. Windows junction creation still returned exit1; its exact cause remains unproven.
Four files also required hosted formatting corrections. The failed instrumented run measured 85.55%
regions / 82.61% branches, but is not accepted coverage evidence because the config case failed.
Archives were gated. The combined repair applies the actual formatter, compares exact model defaults
and uses fully relative junction source/destination arguments under the owned fixture root. It still
requires replacement hosted proof; no source assertion or timeout is removed.
At `76d32e9`, all ten jobs passed: all 59 native integration targets, real Windows junction schedules,
strict quality checks, release builds, all-six dependency notices/archives/checksums/extracted SDK
execution and all three Node jobs. Complete native coverage passed at 85.51% regions / 82.61% branches.
The case ledger still has 470 pending dispositions; this is not complete assertion parity or a Release.
New source-only acceptance fixtures extend exact policy precedence/default immutability, six nested
reserved keys, scalar/allowlist/array/object/depth limits, URL references, mask polarity/model limits,
compatibility/file-loader failures, no-probe health, one actual503 catalog refresh and private bearer
file variants. A separate frozen-source helper now supplies the exact six Persian prompt-only wire
inputs, both Antigravity reference refusals, all 24 malformed sibling pairs, five fatal envelopes and
two private continuation inputs. Native assertions require three-item order, both exact errors and the
original source-generated 8x6 RGBA image. Actual generation additionally exercises the source's count2,
valid-plus-null response, partial receipt, original bytes, explicit error code and one POST after repeat.
These bring the authored tree to 61 integration targets and require separate hosted execution. Failed
catalog-refresh assertions now close and join their fixture before propagating a panic. Its client
retains the 15-second discovery default; the scenario and fixture socket are bounded at eight and three
seconds respectively. A local helper check against the retained older `dist` failed because that runtime
predates sibling isolation; it is not frozen-baseline or native evidence. The old runtime was not rebuilt
or changed. No extra paid request, live host edit or old-runtime cleanup is part of these fixtures.
At `7f7b800`, all 61 native targets, strict Clippy, release builds and actual SDK stdio/HTTP checks
passed on all six platforms. The new config, policy, normalization and count2 partial-publication
cases passed, as did all three Node jobs. Complete nightly coverage passed at 85.65% regions / 83.02%
branches. Formatting in four test files alone failed the six stable jobs, so notice/archive/extraction
steps were skipped. The replacement applies the exact hosted formatter and rebases assertion ranges;
no assertion or timeout changes. All 470 case dispositions remain pending, with 130 assignments.
At `7199441`, the replacement run passed all ten jobs, all 61 native integration targets, strict
Clippy, release builds, complete coverage at 85.65% regions / 83.02% branches, and actual dependency
notices, allowlisted archives, checksums and extracted SDK execution on all six native targets.
This verifies that revision's distribution checks, not complete baseline assertion acceptance.

The next source-only batch adds one registered environment acceptance target (62 targets total):
actual `.env` file loads for all 17 expanded source cases, exact URL/redaction inputs and exact
source-generated 8x6 PNG/JPEG/WebP originals. It directly asserts four-MiB Base64, byte/pixel limits,
real mask dimensions, bounded preview geometry, Unicode file URLs, escaping input links/output
junctions, tampered/missing owned outputs and a regular-file output root. The root-failure fixture
uses a valid prepared/reserved/retained item, exact storage error and preserved pending original;
an unrelated missing request cannot satisfy it. The 82 reviewed case records bring concrete
assignments to 212, still with all 470 dispositions pending. Native compilation, formatting and
execution of this new batch have not run locally and require separate hosted evidence.
Legacy same-request/same-index fresh-UUID publication and the public `addOutput` contract remain
explicit gaps: distinct stable items and directly seeded SQL metadata do not prove those contracts.
Pure sibling-containment and the complete bounded-stream/cancellation input case also remain open.
At `9e6e234`, all 62 native targets, strict Clippy, release builds and SDK checks passed on five
completed native platforms, as did nightly tests and complete 85.80% regions / 83.55% branches.
The macOS ARM native and Node jobs were cancelled, not passing. Six Rust fixture files and the
JavaScript oracle import needed formatting; the two completed Node jobs stopped at that format
check, and all distribution packaging was skipped. The repair uses the actual hosted formatter
without changing assertions. Exact-source review also found blocked readiness returning exit1
instead of exit2; a scoped correction and four actual-executable readiness fixtures are authored
with exact catalog/error/credential inputs, report fields, GET-only/zero-call and redaction assertions.
Those new fixtures are not yet executed. Native readiness runs directly, so they verify the actual
child's reaping rather than inventing the old inner-stdio wrapper's cleanup log marker. That transport
mapping gap remains explicit. Four previously substituted request fixtures now use the exact `draw`,
`../id`, empty reference-path and `extra` inputs. The integrated repair has 63 registered native
targets and 216 concrete case assignments, with all 470 dispositions still pending. Independent
source review found no additional confirmed defects in the readiness repair; it is not runtime proof.
Replacement hosted verification is still required.
At `69e46ee`, all 63 native targets passed on five platforms and nightly; Windows ARM passed three
readiness cases but failed the fourth fixture's directory cleanup with `Uncategorized`, without a
raw OS-code diagnostic. All platforms passed Clippy, release builds and SDK checks. Full coverage
passed at 85.85% regions / 83.82% branches. Linux and Windows Node jobs passed; macOS failed one
POSIX cleanup-record assertion after a post-close group kill returned EPERM and the final probe
established group absence. Native readiness formatting also failed, and packaging was skipped.
The repair keeps unsuccessful signal attempts as diagnostics but resolves only the exact EPERM
attempt after child closure and definitive group absence; permission/probe/deadline/survivor failures
remain failures. Seven deterministic classification fixtures preserve this distinction. Windows
fixture cleanup now reports the raw OS code and retries only documented sharing/delete-pending/
not-empty errors during a two-second retry window. That window limits additional attempts, not the
duration of a synchronous OS deletion call; the original failure's precise cause remains unproven.
The independent-publication source gap is now implemented: two saves with the same request/index
and no receipt create fresh UUID paths, and `Store.add_output` preserves the unchanged SQL schema.
Stable production callers explicitly retain the journal, writer fence and recovery path. Exact
original/tampered/missing/outside-owned assertions are authored and independently source-reviewed,
not yet executed for this new tree. All 470 dispositions remain pending; no source case is promoted
from these source-only repairs, coverage or an older successful run.
At `98cc779`, all 63 native targets, Clippy, release builds and SDK checks passed on all six native
platforms; all three Node jobs passed, including the macOS cleanup regression. Complete native
coverage passed at 85.85% regions / 83.82% branches. Only formatting in `native/main.rs` and the
shared fixture helper failed; archives remained skipped. The Windows cleanup failure did not recur,
but that does not identify its earlier unknown OS cause. Exact hosted formatting is now applied.
The next source-only batch registers 65 native targets and records 271 case assignments. It adds
literal source stream normalization/JSON/SSE/binary cases, exact seven-byte preview/completion
schedules, actual two-reference multipart capture with alpha mask, sibling containment and the exact
three-byte bounded-body fixture. The shared API/asset accumulator rejects missing bodies separately
from empty present bodies. The 9router builder now preserves explicit multi-reference JSON and
rejects unverified implicit non-Codex edits, matching the frozen source rather than inferring current
account entitlement. These new assertions and repairs require their own hosted execution; all 470
case dispositions remain pending. Windows/Linux/macOS x64 and ARM64 tests and builds remain exclusively
on GitHub-hosted runners; no local Rust build or toolchain is used.

At `d9fc7be`, all 65 native targets passed on all six platforms and nightly; Clippy, release builds,
stdio/HTTP SDK checks and all three Node jobs also passed. Complete coverage passed at 86.08% regions /
84.32% branches. Only formatting in five test files failed, so notices, archives and extracted-package
checks were skipped. The actual hosted formatter corrections preserve every assertion and string.
The next acceptance repair adds exact direct-finalization fixtures: source 8x6 originals, ordered
JPEG deviations, aggregate limits, cancellation and recovery on the same Generation/Store, data-URL
output and terminal cancellation. Direct finalization now claims, renews and releases its writer;
already-claimed callers preserve their outer ownership. A controlled failure checks release on error.
Terminal retention updates receipt JSON without relinquishing the writer's full reservation. An exact
1024-byte budget/32-byte result case refuses a peer's 992-byte reservation before release and permits
it afterward. These new changes are source-reviewed but not yet compiled or executed. The withdrawn
hypothesis that same-owner recovery necessarily blocks is not a repair rationale or test result.
The 46-case Store/lifecycle review retains partial and unmapped schedules explicitly. The current
batch has 66 registered native targets and 313 concrete assertion assignments; many remain partial.
All 470 acceptance dispositions remain pending; passing related fixtures does not establish literal parity.

At `39c9643`, all 66 native targets, Clippy, release builds and stdio/HTTP SDK checks passed on all six
platforms; all three Node jobs passed. Complete coverage passed at 86.13% regions / 84.49% branches.
The direct-finalization and intermediate reservation regressions passed. Two formatting corrections
in the new finalization fixture were the only failures, leaving notices and archives skipped.
The next source-only target adds the seven literal Discovery-model dimension variants and two minimal
endpoint-descriptor cases, without adding provider fields absent from the source fixtures. It uses
bounded loopback only for descriptor reads and keeps native capability, forwarding and account
availability separate. The current batch has 67 registered native targets and 322 concrete assertion
assignments, including partial mappings; all 470 dispositions remain pending. Its compilation and
execution remain pending.

At `21a8d9b`, all 67 native targets passed on all six platforms and nightly; Clippy, release builds,
stdio/HTTP SDK checks and all three Node jobs passed. Coverage passed at 86.16% regions / 84.49%
branches. Only discovery-fixture formatting failed; notices and archives remained skipped.
The next unexecuted 68-target batch replaces seven partial Store mappings with literal fingerprint,
identity, admission, cancellation and output-index cases. Nine additional reservation variants use
exact byte counts, capacities, URLs and reopen/control schedules. A separate actual two-process case
uses the source's 2048-byte budget and checks reservation totals only after both Stores/processes
close; the broader live-writer fixture remains. All 470 acceptance dispositions remain pending.

At `18851d2`, all 68 native targets passed on all six platforms and nightly; Clippy, release compilation,
stdio/HTTP SDK checks and all three Node jobs passed. Complete coverage passed at 86.18% regions /
84.58% branches. Only formatting in two test files failed, leaving notices, archives and extracted-package
checks skipped. The exact hosted formatter corrections retain the SQL assertion and its closure body.
The next source-only fixtures preserve the ordered pre-intent refusals, real data-URL edit/mask limits
and owned Responses refresh/active-cancellation schedule on the same Generation/Store. Compilation and
execution of these additions remain pending; none of the 470 case dispositions is promoted.
Four new Discovery lifecycle cases preserve first-key default selection, same-instance profile cache
isolation/stale 503 warnings, repeated cursor/twenty-page refresh bounds, malformed first-page evidence
and enabled/disabled health with absent-secret redaction. The integrated tree registers 69 targets
and 326 concrete case assignments, including partial mappings; 144 cases remain unassigned.

At `f8971fb`, five stable platforms and nightly passed all 69 native targets; all six platforms
passed strict Clippy, and all three Node jobs passed. Complete coverage passed at 86.23% regions /
84.58% branches. Intel macOS did not reach a test harness: test compilation hit its 180-second idle
ceiling after 580.37 seconds, and release compilation hit its 360-second idle ceiling after 860.88
seconds. Its missing-binary stdio error and HTTP process termination are downstream failures, not
passing protocol checks. No compiler diagnostic or assertion failure explains those compilation
stalls; runner slowdown is observed, not an established root cause. Two fixture files required actual
hosted formatting corrections, now applied without assertion changes. Notices and archives were skipped.

The next source-only target preserves five exact HTTP status/error/no-retry cases, the ordered
redirect/malformed-JSON/five-byte-limit/pre-cancelled request case, and the complete source 8x6 PNG
asset-prefix/redirect/loop/limit/unsafe-URL schedule. Its scoped futures own every listener/socket,
with eight-second scenarios and three-second socket bounds; no detached service or sleep is needed.
The tree registers 70 native targets and 333 concrete assertion assignments, including partial mappings;
137 cases remain unassigned and all 470 dispositions remain pending. The new fixtures are not compiled
or executed locally. The next coherent hosted batch also confirms Intel macOS under unchanged bounds;
no timeout increase or production transport change substitutes for that evidence.

At `544baa9`, all ten jobs passed: all 70 native targets and 72 successful harness summaries on
six stable platforms and nightly, strict formatting/Clippy, native release compilation, actual
stdio/HTTP SDK checks, dependency notices, allowlisted archives/checksums and extracted SDK execution.
All three Node jobs passed. Full native coverage passed at 86.25% regions / 84.61% branches. Intel
macOS test and release compilation completed in 197.66 / 323.53 seconds under the unchanged bounds;
the earlier single-run compilation stall was not reproduced, but its original cause is unproven.
These are internal unsigned distribution-test results, not an approved signed Release or host cutover.

The next source-only batch registers 74 native targets and 364 concrete assertion assignments,
including partial mappings; 106 cases remain unassigned and all 470 dispositions remain pending.
Nine Discovery cases retain complete specialty-route evidence, Omni image-only pagination, Gemini
refresh cursor isolation and encoded metadata URLs with a separate 404 result. Comfy fixtures use the
exact five-node graph, source 8x6 originals, three uploads, nine-request schedule and bound final/temp
outputs. Pure workflow cases preserve parsed defaults and original mutation/refusal order. Reference
preflight now uses a separate three-channel opaque-JPEG oracle, unmasked two-upload submission and
both exact gateway aliases, including the serialized Omni multipart model field. Prior seven image
oracle rows remain unchanged; the consumer now requires eight rows. Shared bounded wire capture
avoids duplicated listener/multipart machinery. These additions are source-reviewed, not compiled or
executed. Direct history-error flags versus public provider rejection, arbitrary wait cancellation
reasons and the complete direct-wait schedule remain explicit acceptance gaps.

At `552941c`, five stable platforms and nightly passed all 74 native targets. Windows ARM
passed Clippy, release compilation and SDK checks, but test compilation hit its unchanged 180-second
idle ceiling before any harness started. All six required formatting corrections in five fixture
files; notices and archives were skipped. Complete coverage passed at 86.29% regions / 84.74%
branches. Node Windows/macOS passed; Linux's PowerShell fixture stopped after launch, before its first
parse marker, at the unchanged ten-second idle ceiling. Neither startup/compile stall has an established
root cause; the next coherent run supplies one confirmation without increasing deadlines.

The next source-only tree has 75 native targets and 366 case assignments, including partial mappings;
104 cases remain unassigned and all 470 dispositions stay pending. A source-faithful upscale fixture
captures both edit POSTs, exact 8x6/16x12 originals, unchanged-size partial status and four invalid sizes.
The image oracle preserves its earlier eight rows and adds the exact source 16x12 PNG. A separate
SDK fixture runs the frozen ten-tool case through the real native bridge, including schema descriptions,
original resource bytes, previews, editing, pagination and error results. It is mapped as native SDK
acceptance, not misrepresented as a Rust unit test. The edit description now retains the complete frozen
`edit_region`/translation/upscale contract. Failed-handshake ownership is registered before connecting,
and the actual spawned PID/close event remain tracked even when the SDK clears its transport handle.
A pre-cancelled legacy handshake exercises that failure path. These changes require hosted execution;
source review and syntax checks do not establish runtime acceptance.

At `98a5db8`, all 75 native targets passed on all six stable platforms and nightly. Strict Clippy,
native release compilation, actual stdio/HTTP SDK checks and all three Node jobs passed. The exact
ten-tool SDK case, upscale case and pre-cancelled handshake cleanup passed. Complete coverage passed
at 86.35% regions / 84.96% branches. Windows ARM compilation and Linux PowerShell startup passed
under unchanged bounds; the previous stalls were not reproduced and their original causes remain
unproven. Only formatting in three files failed; notices, archives and extracted checks were skipped.
Those exact hosted formatting corrections are applied without changing assertions.

The next source-only tree retains 75 native targets and adds three exact SDK scenarios: two real
native hosts share one concurrent logical request and submit once; a copied executable discovers an
adjacent compact file from a foreign CWD; compact Responses preserves configured defaults and explicit
overrides. Distinct host identities, environment-selected configuration, original PNG bytes, redacted
paired logs and bounded full-session stderr are asserted. The new MCP PNG uses the frozen source's
separate RGB20/40/60 recipe and `.png()` call; the previous nine image-oracle rows are unchanged.
The ledger now records 369 assignments, including partial mappings, with 101 unassigned cases.
All 470 dispositions remain pending. Three explicitly registered native SDK fixtures require actual
interoperability and extracted-package wiring; source review and structural checks are not runtime
acceptance. These additions require their own hosted verification.

At `e0c670e`, five stable platforms and nightly passed all 75 native targets. All six platforms
passed formatting, strict Clippy, release compilation and the actual SDK checks, including all three
new scenarios. Five targets also passed notices, archives, checksums and extracted SDK execution;
Windows x64 did not reach packaging. Its test compilation stopped at the unchanged 180-second idle
ceiling after 298.95 seconds, before any harness, with no compiler diagnostic. Its release build and
SDK fixtures passed. All three Node jobs and complete 86.35% regions / 84.99% branches coverage passed.
The compiler stall's cause remains unproven. Stable tests now request Cargo's documented artifact
messages while retaining rendered compiler diagnostics and every existing worker, timeout and test gate.

The next source-only batch keeps 75 targets and adds ten case assignments: adjacent dotenv reload,
three empty-config negotiation variants, three live-SDK configuration reload scenarios, two exact
logging cases and installed workflow enum/cycle messages. The retained-handle SDK case explicitly
releases an external seed Store before starting its native child; this is not the source's same-owner
Store schedule and remains partial. The pending ledger has 379 assignments and 91 unassigned cases;
all 470 dispositions remain pending. These additions and compiler-progress diagnostics require their
own hosted execution. No production configuration, provider behavior or dependency is changed.

At `5cd907a`, all 75 native targets and the actual stdio/HTTP SDK checks passed on all six
platforms and nightly, including the new reload, negotiation, logging and workflow cases. Strict
Clippy and release compilation passed everywhere; all three Node jobs passed. Complete coverage
reached 86.39% regions / 85.05% branches. Windows x64 test compilation passed under unchanged bounds;
the previous stall's original cause remains unproven. Only one workflow fixture comma-space failed
formatting, so notices, archives and extracted checks were skipped. That exact correction is applied.

The next source-only tree adds four literal cases across 76 targets: mixed URL/inline cancellation
without asset downloads, real SQL output-index failure followed by same-Store public recovery,
future schema refusal without changing version3, and an event-held Responses waiter/refresher race.
The SQL recovery uses a fresh Generation sharing the original Store, configuration and logger. The
race retains one POST, two GETs, one output/index record, the original PNG and a three-second bound.
The ledger records 383 assignments, including partial mappings, with 87 unassigned cases; every
one of the 470 dispositions remains pending. These additions require their own hosted execution.

At `9de7761`, all 76 targets executed on all six stable platforms and nightly. The held Responses
race, mixed cancellation and future-schema cases passed. One exact SQL recovery case failed
everywhere: successful recovery returned `partial` instead of `completed`. Strict Clippy, release
compilation, actual SDK checks and all three Node jobs passed; formatting failed in two fixture files
and packaging was skipped. Coverage exceeded the floors, but the instrumented test run failed and
is not passing acceptance evidence.

The source-confirmed repair restores the frozen output-save boundary: a failed stable SQL commit
becomes `output_file_error`, which successful local recovery clears. Direct Store errors and the
regular-file output-root error remain unchanged. Existing output-requirement assertions still require
partial status for genuine dimension deviations while requiring recovered storage errors to clear.
The next source-only tree has 78 targets and 387 assignments, including partial mappings, with 83
unassigned cases and all 470 dispositions pending. Four new cases preserve exact region-mask pixels
for three polarities and detached Comfy recovery on the same Generation/Store. Synthetic GPU metadata
and loopback images are not evidence of live inference. This repair and the new cases need hosted verification.

At `0510b6e`, SQL recovery and all four new region/detached cases passed on all six stable
platforms and nightly. Five stable platforms and nightly passed every one of the 78 targets. Linux
x64 failed one readiness case before execution because the OS refused to start its owned child;
the original error lacked an OS code, so its cause is unproven. Release compilation, actual SDK
checks and all three Node jobs passed. Complete coverage passed at 86.41% regions / 85.14% branches.
Strict Clippy found an unused captured header in the region fixture, and formatting failed in two
fixtures; packaging was skipped. The correction asserts the real JSON Content-Type and preserves all
mask assertions. Readiness now reports only ErrorKind and raw OS code without paths or credentials;
no retry, timeout increase or speculative production repair is added.

The next source-only tree has 79 targets and 389 assignments, including partial mappings; 81 cases
remain unassigned and every disposition stays pending. Two literal credential cases test private
file rotation across queue waiting on the same Generation/Store and ephemeral independent headers.
Authorization values are compared exactly, not by substring. Same-Generation process-environment
rotation and nested cancellation credential rotation remain explicit gaps. These cases and the
readiness diagnostic require their own hosted execution.

At `e9ff5eb`, all 79 targets passed on all six stable platforms and nightly, including both
credential cases and all readiness cases. The previous Linux child-spawn failure did not recur at
unchanged bounds; its original cause remains unproven. Strict Clippy, release compilation, actual SDK
checks and all three Node jobs passed. Complete coverage passed at 86.45% regions / 85.17% branches.
Only two formatter hunks in the credential fixture failed; their actual hosted corrections are applied.
Notices, archives and extracted checks were skipped, not passing.

The next source-only tree has 80 targets and 393 assignments, including partial mappings; 77 cases
remain unassigned and every disposition is pending. Four literal cases preserve scalar-reference
refusals, all three array encodings, both multipart reference fields, documented model limits versus
route safety, and original reference/mask bytes at the actual HTTP boundary. An eleventh tiny source
image is the exact 6x8 alpha PNG; the prior ten recipes are unchanged. The cases require their own
hosted execution and final literal review; no live provider or GPU inference was performed.

At `ae9089a`, all 80 targets passed on all six stable platforms and nightly, including the four
new reference/model/multipart cases. Strict Clippy, release compilation, actual SDK checks and all
three Node jobs passed. Complete coverage passed at 86.42% regions / 85.21% branches. Only two
formatter hunks in the reference fixture failed; their actual hosted corrections are applied.
Notices, archives and extracted checks were skipped.

The next source-only tree has 81 targets and 398 assignments, including partial mappings; 72 cases
remain unassigned and all 470 dispositions remain pending. Five literal cleanup cases keep response
bodies open and independently require prompt client rejection and actual socket EOF/reset. They
preserve redirect, declared-size, streamed-size and SSE-error inputs, plus the combined oversized
asset/unauthorized redirect schedule. The two DNS-hostname/autoSelectFamily cases remain unassigned.
New cleanup execution and final literal review are pending; existing broader fixtures remain intact.

At `68ac3f3`, all 81 targets and all five cleanup cases passed on six stable platforms and nightly.
Strict Clippy, release compilation, actual SDK checks, all three Node jobs and full 86.42% regions /
85.24% branches coverage passed. Only one asset-fetch call layout failed formatting; the actual
hosted correction is applied. Notices, archives and extracted checks were skipped.

The next source-only tree has 83 targets and 410 assignments, including partial mappings; 60 remain
unassigned and all 470 dispositions are pending. Ten exact review-regression cases preserve batch,
gateway-field and missing-job-ID refusals, two actual concurrent Store finalizers, original output
requirements and excessive-item rejection. Two exact local-recovery cases preserve pre-aborted inline
completion, same-Store connectionless recovery of original bytes and the 100-byte reservation/release
schedule. Independent source review caught a discarded generation-stamped preparation receipt in the
new fixture; it now passes the actual returned receipt without weakening production fencing. These
12 cases still require their own hosted execution and final literal acceptance.

At `d98405b`, all 83 targets and the twelve new review/local-recovery cases passed on all six
stable platforms and nightly. Strict Clippy, release compilation, actual stdio/HTTP SDK checks and
all three Node jobs passed. Complete coverage passed at 86.47% regions / 85.30% branches. Only six
formatter hunks in the two new fixtures failed; their actual hosted corrections are applied without
changing assertions or strings. Notices, archives and extracted checks were skipped.

The next source-only tree has 85 targets and 425 assignments, including partial mappings; 45 remain
unassigned and all 470 dispositions stay pending. Twelve transport cases preserve terminal/uncertain
SSE, the original 19-byte write split and both adapters' other/missing/owned job-ID waiter/refresh
matrix. They call the actual Application dispatch seam through real HTTP, not the SDK envelope or
InMemoryTransport handshake; those protocol assertions remain explicit gaps. Three recovery cases
preserve a published original across a real SQL commit fault on the same Store, refuse a changed
stable target without dropping retained bytes, and read back a completed receipt from the migrated
live-WAL backup. The remaining write/sync/link/unlink/receipt fault matrix is not closed by these cases.
Owned current-thread runtimes drop outstanding lease futures before synchronous workspace cleanup,
including panic/timeout paths. Tiny original fixtures, three-second sockets/local recovery, five-second
calls, eight-second scoped actions and the existing two-worker ceiling remain. These additions require
their own hosted compilation/execution and final independent literal acceptance.

At `8a0c926`, all 85 targets and all fifteen transport/publication cases passed on six stable
platforms and nightly. Strict Clippy, release compilation, actual SDK checks and all three Node jobs
passed. Full coverage passed at 86.50% regions / 85.36% branches. Only eight formatter hunks in the
transport/publication fixtures failed; the exact hosted corrections are applied without assertion
or string changes. Notices, archives and extracted checks were skipped.

The next source-only tree has 87 targets and 437 assignments, including partial mappings; 33 remain
unassigned and all 470 dispositions stay pending. Four exact terminal cases preserve disconnect or
malformed polling uncertainty and false or ambiguous cancellation acknowledgements, with two actual
Store reopens, blocked admission and no generation POST. They exercise Application dispatch, not SDK
envelopes or handshake. Eight separately named killed-child cases retain exact source defaults and
receipt fields, real checkpoints/termination, phase-specific first-open state, public job reads,
original/index/pending/file counts and a second reopen's deduplication. Synthetic SQL lease expiry
substitutes for the source's clock spy and the published checkpoint uses a restored SQL trigger;
those mechanism differences remain explicit partial-acceptance gaps. Child termination/reaping and
reader completion are bounded on success and unwind; only finished readers are joined, and incomplete
cleanup fails visibly to the outer owned CI command. The published checkpoint additionally verifies
actual original target bytes and an empty index before termination. These cases require their own
hosted execution and final independent literal acceptance; broader earlier fixtures remain intact.

At `ff4a47c`, all 87 targets and all twelve terminal/crash cases passed on six stable platforms
and nightly. Strict Clippy and all three Node jobs passed; full coverage was 86.51% regions / 85.33%
branches. Five platforms also passed release compilation and actual SDK checks. Intel macOS release
compilation stopped after 360 seconds without output; subsequent missing-executable SDK failures
are downstream, not protocol evidence. No compiler diagnostic established the stall's cause.
Five formatter hunks failed; their exact hosted corrections are applied. Notices, archives and
extracted checks were skipped. Release compilation now emits real Cargo artifact progress at the
unchanged wall/idle and two-worker bounds; this is diagnostic visibility, not a proven stall repair.

The next source-only tree has 88 native targets and 441 assignments, including partial mappings;
29 cases remain unassigned and all 470 dispositions stay pending. Three actual-SDK terminal cases
retain the frozen terminal/control/cancellation matrices, two native hosts, two restarts, original
bytes/output identities, admission probes and replay conflicts. Six startup run IDs plus simultaneous
distinct PIDs replace the source's across-restart numeric PID uniqueness; that fidelity substitution
remains explicit. Scenario cancellation awaits actual work and cleanup; diagnostics report only
completed rounds, not timer heartbeats. One exact Comfy waiter/refresher case holds the first history
socket, completes public recovery, releases and settles the original, and asserts one output/index
and one submission. These additions require hosted and extracted execution and final literal review.

At `f8fc7e1`, all 88 targets, the Comfy waiter case and three actual-SDK terminal matrices passed
on all six stable targets and nightly. Clippy, release compilation, stdio/HTTP SDK checks and all
three Node jobs passed. Full coverage passed at 86.59% regions / 85.52% branches. Intel macOS release
compilation passed at unchanged bounds; the earlier stall's cause remains unproven. Only two Comfy
fixture formatting hunks failed; their exact hosted corrections are applied. Notices, archives and
extracted checks were skipped.

The next source-only tree retains 88 targets and has 448 assignments, including partial mappings;
22 cases remain unassigned and all 470 dispositions remain pending. Seven actual-SDK race cases
preserve held Responses polls, exclusive live-writer ownership, targeted Comfy cancellation and
completion-before-download precedence across two hosts and two restarts. Original bytes, output
identities and exact submission/poll/cancellation/download counts remain asserted. Shared host/provider
ownership now observes asynchronous handler errors after draining cleanup; a two-case regression checks
late failure propagation and preservation of an original scenario error. Cases retain 30-second bounds,
45-second host lifetimes and five-second event limits; the whole SDK gate remains 180s wall / 45s idle.
These changes still require hosted/extracted execution and final independent literal acceptance.

At `f09561c`, all 88 targets passed on six stable platforms and nightly. All six stable jobs also
passed formatting, Clippy, release compilation, SDK stdio/HTTP, notices, archives/checksums and
extracted SDK execution, including all seven new race cases. All three Node jobs passed. The nightly
job nevertheless failed its SDK terminal admission probe: zero POSTs were observed where one was
required. Coverage itself passed at 86.55% regions / 85.39% branches; this does not accept the failed
job. The failure occurred before the seven new SDK race cases ran on nightly. Its cause is unknown.
The unchanged admission assertions now include sanitized adapter/mode/round and result error codes,
not prompts, private paths or raw responses. No retry, timeout increase or production repair is claimed.

The next source-only tree has 89 targets and 450 assignments, including partial mappings; 20 cases
remain unassigned and all 470 dispositions stay pending. Two exact Linux/macOS cases exercise private
0600 credentials, 0644 refusal, Unicode compact configuration paths and safe/escaping symlinks with
original image bytes. Windows retains the source's platform exclusion. These additions require their
own hosted execution and final literal acceptance. A Windows DPAPI script/native roundtrip draft is
not integrated: failure-path descendant ownership remains unresolved.

At `fbe8a3c`, all 89 targets passed on all six stable platforms and nightly; the two POSIX cases
passed on Linux/macOS and remained inapplicable on Windows. Clippy, release compilation, all SDK
terminal/race/HTTP checks and three Node jobs passed. The prior nightly admission failure did not
reproduce at unchanged bounds; its original cause remains unknown. Full coverage passed at 86.61%
regions / 85.58% branches. Only three POSIX formatting hunks failed; their exact hosted corrections
are applied without changing assertions. Notices, archives and extracted checks were skipped on this
commit, so the earlier `f09561c` packaged execution is not fresh evidence for this tree.

At `01eb340`, all ten jobs passed on the exact commit: all 89 targets on six stable platforms and
nightly, fmt/Clippy/release compilation, actual stdio/HTTP SDK checks, notices/licenses, allowlisted
archives/checksums and extracted SDK execution on every stable target. Complete coverage passed at
86.61% regions / 85.55% branches. These are unsigned internal distribution-test artifacts, not a
signed Release or complete baseline-case acceptance.

At `d58b316`, all ten exact-commit jobs passed: all 90 targets on six stable platforms and nightly,
fmt/Clippy/release compilation, SDK stdio/HTTP, notices/licenses, archives/checksums and extracted SDK
execution on all six targets. Coverage passed at 86.64% regions / 85.67% branches. The Windows fixture
separator repair passed with exact command equality and root-redaction assertions unchanged. Three
native registration cases preserve JSON backup/rollback/removal, CRLF TOML/refusals, concurrent writes,
foreign locks and invalid JSON. Native executable/bridge argv and PID-bearing log filenames still need
final source-format disposition. The exact requirements case preserves the 8x6 alpha PNG, opaque
roundtrip, Flash 4:3/4K mismatch and all 56 Flash / 30 Pro metadata pairs; the broad 7x5 oracle is separate.

At `a7273cf`, all ten jobs passed: 91 targets on six stable platforms and nightly, all-six packaged
SDK execution and coverage 86.65% regions / 85.61% branches. Exact lease-clock cases passed without
changing SQL/fences or receipt timestamps. The Responses waiter passed its unchanged three-second
bound; diagnostics and teardown correction do not establish the earlier timeout's cause.

At `b83d872`, all 92 targets and four Environment cases passed six stable platforms and nightly.
All-six fmt/Clippy/release/SDK stdio/HTTP/notices/licenses/archives/checksums/extracted SDK and three Node
jobs passed. Intel macOS release took 674.532 seconds under unchanged bounds; its earlier timeout cause
remains unknown. Only nightly SDK failed: Interactions/failed/round0, zero observed admission POSTs,
`request_timeout/network`, 1909 ms elapsed and available started/intent/completed rows. The unchanged
700 ms cooperative deadline is not a hard wall for synchronous work. Failed-run coverage measured
86.71% regions / 85.64% branches; it is not passing acceptance. A preparation-only observer now measures
client-build/pre-send time before logging, without changing TLS, retries, request ordering or deadlines.

At `4f471d2`, all ten jobs passed: 96 targets on six stable platforms and nightly, all-six
fmt/Clippy/release/SDK/notices/archives/checksums/extracted SDK, and 86.93% regions / 85.67% branches.
Both Windows strict DPAPI/native Unicode/corrupt/no-overwrite and owned Job abort cases passed.
The fresh private destination fixes fixture-child access; original ACL assertions/bounds remain.
x64 inventory passed in 13.015 seconds; its earlier idle cause remains unknown, not a proven fix.
At `5b4ec24`, all 97 targets passed six stable platforms and nightly, including both Windows DPAPI/abort/privacy cases; coverage was 86.90% / 85.63%. Only two formatting layouts failed, archives skipped. Exact hosted formatting is applied; the prior x64 timeout cause, all 470 dispositions and [limits](testing/CREDENTIAL-SNAPSHOTS.md) remain open.

CI starts only through a manual `workflow_dispatch`, once for the final integrated commit. The six
native OS/CPU targets and separate coverage job remain unchanged; no push or pull-request event starts
an additional matrix. Final publication also requires approved embedded publisher identity and genuine
platform signing. No approved signer/trust path has been established; the repository has no signing
secrets. Prior unsigned compilation is historical test evidence, not a signed release or activation claim.

CI compiler/dependency caches, target/build output and TEMP/TMP/TMPDIR stay inside the workspace, with
two workers and documented wall/idle ceilings. No Rust toolchain install/build/full test runs locally
for this migration. [Acceptance matrix](testing/MIGRATION-MATRIX.md) records incomplete source-case,
crash/restart, multipart, packaging and live/GUI verification rather than calling them passing.

## Historical Node implementation (frozen checkout only)

The frozen TypeScript7.0.2 compiler runs build/typecheck single-threaded through an explicit
@typescript/native npm alias. Type-aware ESLint retains the supported TypeScript6.0.3 API via
Microsoft's @typescript/typescript6 compatibility alias. Normal npm ci resolves both without forced
peers or parser patches. The check entrypoint asserts both versions and the lint API before running
existing strict checks; the three-platform matrix also validates the native compiler's emitted runtime.
See [Microsoft's side-by-side guidance](https://devblogs.microsoft.com/typescript/announcing-typescript-7-0/#running-side-by-side-with-typescript-6.0).

npm run check: strict typecheck/lint/build/UTF8/source-size. npm run coverage: all discovered unit,
contract and integration files with85 percent statements/80 percent branches. No runtime exclusions.
Outer runner180s wall/45s idle, case10s/hooks15s, max2 workers. POSIX cleanup waits at most2s
for the killed process group to disappear; it never treats a live group as verified cleanup. Tiny cached Sharp fixtures and isolated
project-owned temp roots; loopback external boundaries only, finally cleanup stores/loggers/sockets.

npm run package:check validates allowlisted npm artifact and a production-only clean install in a
path with spaces using a real MCP client. npm run smoke performs provider-free handshake/tools/health.
New tests/integration/posix-platform.test.ts runs on Linux/macOS: owner-only0600 secrets vs0644,
Unicode native config paths, symlink escape rejection and actual bounded POSIX process-group cleanup.
Windows skips those three OS-specific assertions; DPAPI remains its separate Windows-only assertion.
Common compact/stdio/packed tests run on every OS. Hosted CI Windows/Linux/macOS on Node24.21.0; Windows hosted is not Windows11 Enterprise/Server2025 proof.
Runtime dependencies audited. All test files matched automatically by Vitest include; no hand-list gap.

Local light checks ran only to drive implementation. Full final matrix is reserved for CI. Generic
SDK fixture interoperability is not a Claude/Codex GUI test. Synthetic images verify bytes/container/
metadata/path semantics, not real model quality or editing fidelity. No paid default tests.
Setup real-entry fixtures verify locked concurrent JSON/TOML registration, backup restoration,
conservative TOML refusal, five PowerShell7 WhatIf actions and idle-timeout descendant cleanup.
Logging fixtures verify immutable per-run metadata, closed file handlers, redaction at all levels;
actual stdio tests assert paired tool start/completion/failure with call IDs/durations and no key/prompt.
Native wire-contract fixtures assert method/path/header/body for each protocol, multipart byte fidelity,
legacy-client negotiation, endpoint descriptors and private continuation state. Recovery regressions
exercise renewable writer fencing, expired prepared admission, retained inline bytes after cancellation,
local recovery without connection credentials, published-file/hash reconciliation, bounded journal
reservations, real output-index SQL-trigger failure, future-schema refusal, and legacy migration.
Actual child kill checkpoints cover pre-intent/prepared/submitted/running/finalizing/retained/published/
committed states; reopened stores assert deduplication and original hashes. Original Comfy waiting
races a refresher through the actual Generation entry point; partial downloads retain the first image.
These cases passed fresh hosted CI on320f903; historical CI is not used as their evidence.
Scheduled filesystem races insert a directory, regular file or
junction between the missing-path observation and mkdir; only the real directory is accepted.
Two distinct reference images prove scalar contracts reject and array encoding preserves both.
Dimension fixtures separate sourced model limits from the route safety ceiling and compare persisted
aspect/tier requirements against original output bytes.
No controlled real-gateway process was run against fake upstreams; gateway-shaped contract fixtures
must not be called deployed-gateway integration. Interactive DPAPI creation/ACL remains not_run.

Live tests require endpoint/model/number/cost authorization. No existing key authorizes generation.
GPU actual graph/model/backend and host GUI tests not_run when unavailable. Keep evidence separate
from configured/documented/advertised support. See TEST-RESULTS.md and REQUIREMENT-TEST-MATRIX.md.

Terminal lifecycle fixtures exercise public MCP failed/cancelled results, polling error/identity controls,
targeted cancellation dispatch versus confirmation and database reopen. Reservation fixtures preserve
unknown receipts, retained bytes/URLs and live/detached budgets; actual child processes race one bounded
journal budget. Local recovery preserves terminal generation state and original bytes. These are
synthetic provider contracts, not live Comfy/Gemini/OpenAI cancellation or refund proof.

Real stdio terminal acceptance: stdio-terminal-lifecycle.test.ts runs two actual built MCP processes
sharing isolated SQLite, restarts both twice, and verifies Responses/Interactions terminal variants,
polling controls and targeted Comfy cancellation through public tools. Public generation probes verify
released versus retained admission without replaying old jobs. stdio-terminal-races.test.ts gates actual
waiting/refresher/cancellation races and completion-before-download precedence; original bytes and output
identities survive both restarts. The host fixture drains bounded stderr, limits calls/startup to5s and
child lifetime to45s, closes and observes real process exit. Cases are bounded at30s/60s; max2 CI workers
and existing coverage gates remain. These tests use loopback synthetic providers, not live accounts. A peer refresher must respect a live
writer in the other process rather than steal its lease. Explicit job recovery ignores prior local wait
cancellation for original-byte publication. Concurrent first-open WAL contention is bounded separately
from provider calls; no network submission is retried.
