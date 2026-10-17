# Native migration acceptance matrix

Baseline: `fc7da25c3b470199949b1b4c67127d77afa0c34a`, 50 discovered suites / 470 cases.
The Rust implementation is in progress. The integrated `3655fdc` run passed all 55 native targets,
strict Clippy and actual SDK stdio/HTTP acceptance on all six OS/CPU targets, including authoritative
UTF-16/number application values, the repaired Windows crash fixture and public HTTP outputs/resources.
At `8271b81`, the retained-route/ambiguity/local-recovery regression also passed. Five stable platforms
passed all 55 runtime targets; Linux x86_64 exceeded the aggregate asynchronous polling fixture's
eight-second deadline. All six passed strict Clippy and actual SDK stdio/HTTP acceptance. Formatting
failed in one test file. Complete instrumented coverage measured 84.72% regions / 81.17% branches:
the region floor still failed against unchanged 85% / 80% gates. At `bc34fbf`, all 56 native runtime
targets, strict Clippy and SDK stdio/HTTP acceptance passed on all six targets. Complete coverage was
84.86% regions / 81.42% branches; formatting in two test files and the region floor still failed.
The Node macOS maintenance cleanup-verification record also failed. At `3c5958f`, all 56 native targets,
strict Clippy and SDK stdio/HTTP acceptance passed on all six targets, all three Node jobs passed,
and coverage passed at 85.25% regions / 82.18% branches. Dynamic collection produced the same 470
case identities on all three OSes, including platform skips. One native formatting correction still
failed; archive checks did not run. At `0870b9e`, five native targets passed formatting, strict Clippy,
all 56 runtime targets, release builds and SDK stdio/HTTP acceptance. Intel macOS stopped on a
30-second toolchain-download timeout before native checks. All three Node jobs and complete native
coverage passed at 85.25% regions / 82.18% branches. Five actual-notice preservation steps failed;
archive/extraction checks remained skipped. Exact upstream-notice and bounded public-download repairs,
plus six actual provider-HTTP fixtures, were published at `0f80e3b`. All six provider cases, all
57 native targets, strict Clippy, release builds, five offline tooling contracts and SDK acceptance
passed on all six targets; complete coverage remained 85.25% regions / 82.18% branches. One test
formatting correction blocked packaging. Linux's Node PowerShell fixture hit its ten-second idle
bound; the other Node jobs passed. The actual notice collector and archives remain unverified.
At `3d79621`, formatting, all 57 native targets, strict Clippy, release builds and SDK acceptance
passed on all six targets. All three Node jobs and unchanged coverage floors passed. Fresh 470-case
structural validation passed on three OSes without accepting assertion parity. Actual notice
collection identified only `rmcp 3.5.1` missing its packaged license; its complete pinned upstream
notice was subsequently preserved. At `ec60ae3`, actual notices, allowlisted archives, checksums and
extracted stdio/HTTP SDK execution passed on all four POSIX targets. All three Node jobs and full
85.37% regions / 82.20% branches coverage passed. Both Windows jobs failed only the new junction
fixture's shell command; their archive/extraction steps did not run. The fixture correction uses an
owned normal CWD and relative basename, retaining real junction/containment assertions and time bounds.
New config acceptance and source-exact requirement warning/deviation/finalization/recovery cases bring
the authored tree to 59 integration targets. At `3dc882a`, all four requirements cases passed on all
six targets; all three Node jobs and native Clippy/release/SDK checks passed. Packaging remained gated
by formatting, a too-strict compact model-override fixture and Windows junction command exit1. Its
85.55% regions / 82.61% branches report came from a failed instrumented run, not accepted coverage.
The combined repair retains the source settings subset and real junction assertions; replacement
hosted execution remains required. That replacement, `76d32e9`, passed all ten jobs, including all
59 native targets and all-six notices/archives/checksums/extracted SDK execution. Complete coverage
passed at 85.51% regions / 82.61% branches. Windows junction and compact settings cases passed; full
470-case assertion equivalence and Release/cutover remain open. The next source-only policy/config
and exact wire/malformed-result/continuation fixtures bring the authored tree to 61 integration targets;
they are not covered by that green SHA. A new actual count2 partial-publication fixture retains original
bytes and proves one POST after same-instance repeat. All new literal mappings remain pending hosted
execution and final review; an older local runtime cannot supply the frozen normalization expectations.
At `7f7b800`, all 61 native targets, strict Clippy, release builds and SDK stdio/HTTP checks passed on
all six platforms; all three Node jobs and full 85.65% regions / 83.02% branches coverage passed.
Only formatting in four test files failed. Actual notices/archives/extracted execution were skipped
on that revision. Exact hosted formatting and assertion-range corrections still need replacement CI;
130 assignments and all 470 pending dispositions do not establish full source-case acceptance.
At `7199441`, all ten jobs passed, including all 61 native targets, strict quality, complete 85.65%
regions / 83.02% branches and all-six actual notices, archives, checksums and extracted SDK execution.
The new source-only environment/foundation/image batch has 62 registered targets and 212 concrete
case assignments, all still pending native execution and final literal acceptance. Exact file loads,
original codecs, Base64, masks/previews, Unicode paths, actual input/output links and owned-output
failures are now authored. Legacy fresh-UUID same-index publication/public addOutput, pure containment
and the exact bounded-stream/cancellation case remain gaps, not silently retired contracts.
At `9e6e234`, five completed platforms passed all 62 native targets, strict Clippy, release and SDK
checks; complete nightly coverage passed at 85.80% regions / 83.55% branches. Both macOS ARM jobs
were cancelled. Only six Rust-file layouts and the JavaScript import formatting failed completed
jobs; packaging was skipped. Actual hosted formatting is applied without weaker assertions.
A source-confirmed readiness exit1/exit2 drift is corrected with four source-faithful executable
fixtures and exact invalid-request literals. This repair is source-only pending its own hosted run;
direct native maintenance does not establish the old inner-stdio/cleanup-marker assertion.
The integrated repair registers 63 native targets and records 216 case assignments, all 470
pending. Independent source review found no additional confirmed readiness defects, not proof
of compilation or execution.
At `69e46ee`, five platforms and nightly passed all 63 native targets; Windows ARM failed only one
readiness fixture's final directory cleanup. All six passed Clippy, release and SDK checks, but
readiness formatting failed and archives were skipped. Full coverage passed at 85.85% regions /
83.82% branches; macOS Node failed the final POSIX cleanup-state report after a denied kill attempt.
The next unified repair applies hosted formatting, keeps denied attempts separate from definitive
final absence proof, and makes Windows transient fixture cleanup bounded and diagnostic. It also
implements source-compatible fresh-UUID same-index independent saves and `Store.add_output` while
stable callers retain their journal/fences. These changes remain unexecuted natively; pure containment,
exact bounded-stream/cancellation and the old inner-stdio/cleanup-marker acceptance remain open.
At `98cc779`, all six native platforms passed all 63 targets, Clippy, release builds and SDK checks;
all three Node jobs and full 85.85% regions / 83.82% branches coverage passed. Only main/shared-helper
formatting failed; archives were skipped. Exact hosted formatting is applied without changing behavior.
The next 65-target source batch brings concrete assignments to 271, all 470 still pending. Literal
stream/result/builder fixtures, exact incremental schedules, two-reference multipart with mask,
sibling containment and bounded-body/cancellation cases are authored. Source-confirmed 9router
multi-reference and unverified implicit-route divergences are corrected; missing API/asset bodies
are distinct from empty present bodies. Current compilation/execution and final literal acceptance
remain pending. Builds and tests continue on all six GitHub-hosted Windows/Linux/macOS x64/ARM64 targets.

At `d9fc7be`, all 65 native targets passed on all six stable platforms and nightly; all-six Clippy,
release builds and stdio/HTTP SDK checks, all three Node jobs and full 86.08% regions / 84.32% branches
coverage passed. Only five test-file formatting checks failed; notices and all archives remained
skipped. Actual hosted formatting is now applied without assertion/string changes.
The next source-only repair registers a direct-finalization acceptance target with four literal
8x6/default-receipt cases and a direct-error versus claimed-writer control. Direct calls claim/renew/
release; outer callers explicitly retain claimed ownership. Terminal retention preserves the full
reservation until release, with the 1024/32/992 intermediate peer-budget assertion missing from older
fixtures. Independent source review found no confirmed defects, not compilation/runtime proof.
The current batch has 66 registered native targets and 313 concrete assertion assignments, including
partial mappings. All 46 reviewed Store/lifecycle cases retain pending status; remaining public
SDK/reopen/process schedules are not closed by direct Store assertions. The earlier same-owner recovery-blocked
hypothesis was retracted and is not treated as a failure.

At `39c9643`, all 66 native targets and all-six Clippy/release/stdio/HTTP SDK checks passed; Node jobs
passed on all three OSes. Complete coverage passed at 86.13% regions / 84.49% branches. Both lifecycle
repairs passed their new regressions. Only two formatting corrections in one test file failed;
notices and archives were skipped. The next unexecuted target adds seven literal Discovery-model
dimension variants and two source-minimal endpoint-descriptor cases. Independent source review found
no confirmed defects; that is not native compilation or complete case acceptance. The current
67-target batch records 322 concrete assertion assignments, including partial mappings; all 470
acceptance dispositions remain pending.

At `21a8d9b`, all 67 native targets, all-six Clippy/release/stdio/HTTP SDK checks, all three Node jobs
and full 86.16% regions / 84.49% branches coverage passed. Only discovery-fixture formatting failed;
notices and archives were skipped. The next source-only 68-target batch adds seven literal Store
cases, nine exact reservation variants and a two-process 2048-byte budget assertion after both child
Stores/processes close. Previous broader assertions remain; all 470 acceptance dispositions stay pending.

At `18851d2`, all 68 native targets, all-six Clippy/release/stdio/HTTP SDK checks, all three Node jobs
and complete 86.18% regions / 84.58% branches coverage passed. Only formatting in two test files
failed; notices, archives and extracted-package checks were skipped. Those actual formatter changes
are applied. The next source-only batch adds three literal Generation scenarios and four Discovery
lifecycle cases: cache isolation/stale refresh, cursor/page limits, malformed first page and disabled
connection health/redaction. It has 69 registered targets and 326 concrete case assignments, including
partial mappings; 144 cases remain unassigned. All case dispositions remain pending native
execution and final literal review. Manual dispatch retains the complete matrix without automatic
push/PR reruns. Final signed publication additionally waits for an approved owner signer/trust path;
no signing credentials or trust identity have been invented.

At `f8971fb`, five stable platforms and nightly passed all 69 native targets, all six passed Clippy,
all three Node jobs passed, and complete coverage passed at 86.23% regions / 84.58% branches.
Intel macOS reached no test harness: test and release compilation stopped at unchanged 180-second /
360-second idle ceilings after 580.37 / 860.88 seconds. Missing-binary stdio and HTTP termination
are downstream failures; no compiler diagnostic or test assertion establishes a production regression.
Actual formatting corrections in two fixtures are applied; notices and archives were skipped.
The next source-only target adds the five literal HTTP status cases, ordered redirect/malformed/body/
cancellation case and complete original-byte asset schedule. There are 70 registered targets and 333
concrete assignments, including partial mappings; 137 remain unassigned, and all 470 dispositions
remain pending. Replacement hosted execution must confirm these additions and Intel macOS without
relaxing bounds. Literal-IP asset tests do not establish hostname DNS-pinning coverage.

At `544baa9`, all ten jobs passed: all 70 native targets, strict quality, native release compilation,
stdio/HTTP SDK checks, all-six notices/archives/checksums/extracted SDK execution and all three Node
jobs. Complete coverage passed at 86.25% regions / 84.61% branches. Intel compilation passed under
unchanged bounds; the earlier one-run stall was not reproduced and its original cause is unproven.
The next source-only batch has 74 registered targets and 364 assertion assignments, including partial
mappings; 106 remain unassigned, all 470 dispositions stay pending. It adds nine literal Discovery
cases and exact Comfy graph/upload/history/reference/gateway assertions with a separate opaque-JPEG
source oracle. The history-error terminal flag remains a partial seam mapping; direct wait/reason and
public rejection integration still need evidence. None of the new tests is verified by `544baa9`.

At `552941c`, all 74 native targets passed on five stable platforms and nightly; Windows ARM test
compilation hit the unchanged 180-second idle ceiling before a harness. All six Clippy/release/SDK
checks passed, but formatting in five fixtures failed and all archives were skipped. Full coverage
passed at 86.29% regions / 84.74% branches. Node Linux hit its PowerShell-startup idle deadline; Node
Windows/macOS passed. Neither stall is explained by a compiler diagnostic or reproduced root cause.
The next coherent batch confirms both under unchanged bounds and applies the actual hosted formatting.

The current source-only tree has 75 registered targets and 366 concrete assignments, including partial
mappings; 104 remain unassigned and all 470 dispositions remain pending. Exact upscale wire/original-byte
acceptance and the frozen ten-tool SDK case are authored. The SDK mapping validates its actual hosted
and extracted entrypoints; it is not a fictitious Rust test. Complete edit metadata and failed-handshake
child ownership are restored, with a pre-cancelled legacy-handshake regression. None of these additions
is verified by the earlier green SHA.

At `98a5db8`, all 75 native targets passed on all six stable platforms and nightly; all-six Clippy,
release compilation and SDK checks, all three Node jobs and full 86.35% regions / 84.96% branches
coverage passed. Exact ten-tool/upscale cases and failed-handshake cleanup passed. Both prior stalls
passed unchanged bounds, without establishing their original causes. Only three formatting paths
failed; notices, archives and extracted checks were skipped. The actual formatter corrections are applied.

The next source-only tree keeps 75 targets and records 369 concrete assignments, including partial
mappings; 101 remain unassigned and all 470 dispositions stay pending. Three new SDK scenarios cover
exact two-host concurrent deduplication, executable-adjacent compact config from a foreign CWD and
compact Responses defaults/overrides. The separate RGB20/40/60 source PNG preserves the literal `.png()`
recipe; previous nine oracle rows are unchanged. Actual hosted and extracted execution is still required.

At `e0c670e`, all 75 targets passed on five stable platforms and nightly; all six passed formatting,
Clippy, release compilation and actual SDK checks, including the three new scenarios. Five targets
passed notices, archives, checksums and extracted execution. Windows x64 test compilation hit the
unchanged 180-second idle ceiling after 298.95 seconds before any harness; no compiler diagnostic
explains it. Its release build and SDK fixtures passed, but packaging was skipped. All three Node
jobs and full 86.35% regions / 84.99% branches coverage passed. The next run emits actual Cargo artifact
completion messages, keeping rendered diagnostics, bounds, workers and all assertions unchanged.

The current source-only tree keeps 75 targets and records 379 assignments, including partial mappings;
91 remain unassigned and all 470 dispositions stay pending. Ten new assignments cover adjacent dotenv,
three negotiation variants, three SDK reload scenarios, two logging cases and workflow enum/cycle
messages. The retained-handle fixture's external Store ownership-release handoff is explicitly partial,
not source-equivalent same-owner seeding. Current native compilation and hosted/extracted execution
remain required; source review or a registered mapping does not establish them.

At `5cd907a`, all 75 targets passed on all six stable platforms and nightly. All-six strict Clippy,
release compilation and actual stdio/HTTP SDK execution passed, including the new seven SDK scenarios
and three literal logging/workflow cases. The three Node jobs and complete 86.39% regions / 85.05%
branches coverage passed. Windows x64 compilation passed unchanged bounds, without explaining the
original stall. Only one workflow fixture comma-space failed formatting; notices, archives and
extracted checks were skipped. The exact formatting correction is applied.

The next source-only tree has 76 targets and 383 assignments, including partial mappings; 87 cases
remain unassigned and all 470 dispositions remain pending. Four literal cases cover mixed-result
cancellation, public SQL-failure recovery using a fresh Generation sharing the original Store,
unchanged future schema version3 and an actual held Responses waiter/refresher race. Each retains
the exact source inputs and original PNG assertions; the race keeps its three-second bound. These
new cases have not yet run in hosted CI.

At `9de7761`, all 76 targets executed on all six stable platforms and nightly; one SQL recovery case
failed with `partial` instead of `completed`. The held Responses race, mixed cancellation and future
schema cases passed. Strict Clippy, release compilation, actual SDK checks and Node jobs passed;
formatting failed in two fixtures and archives were skipped. Coverage exceeded its floors, but the
instrumented test run failed and is not passing acceptance evidence.

The next source-only tree repairs the output-save error boundary rather than clearing arbitrary
storage errors: stable commit failures become the source's recoverable `output_file_error`. Existing
dimension deviations, direct Store errors, regular-file output-root behavior, journal identity and
original bytes remain asserted. Two new targets add three exact region-edit polarity cases and one
same-Generation/Store detached Comfy case. There are 78 targets and 387 assignments, including partial
mappings; 83 remain unassigned and all 470 dispositions stay pending. Current hosted execution is required.

At `0510b6e`, the SQL recovery repair and region/detached cases passed on all six stable platforms
and nightly. Five stable platforms and nightly passed all 78 targets; Linux x64 failed one readiness
case because its owned child could not start, with no OS code recorded. The cause remains unproven.
Release compilation, actual SDK checks, Node jobs and complete 86.41% regions / 85.14% branches
coverage passed. Strict Clippy reported an unread captured header in the region fixture, formatting
failed in two fixtures and packaging was skipped. The correction asserts real JSON Content-Type;
readiness adds safe ErrorKind/raw-OS-code diagnostics without retries or changed bounds.

The next source-only tree has 79 targets and 389 assignments, including partial mappings; 81 remain
unassigned and all 470 dispositions remain pending. Two exact credential cases cover same-Generation
private-file queue rotation and ephemeral independent headers with unchanged serialized values.
Same-Generation process-environment rotation and nested cancellation rotation remain unassigned.
Current native execution and final literal review are still required.

At `e9ff5eb`, all 79 targets and both credential cases passed on six stable platforms and nightly,
along with all readiness cases, strict Clippy, release compilation, actual SDK checks and Node jobs.
Full coverage passed at 86.45% regions / 85.17% branches. Only two credential-fixture formatter hunks
failed; actual hosted corrections are applied. Archives and extracted checks were skipped. The prior
Linux spawn failure did not recur under unchanged bounds; its original cause is still unknown.

The next source-only tree has 80 targets and 393 assignments, including partial mappings; 77 remain
unassigned and all 470 dispositions remain pending. Four exact cases cover scalar-reference loss,
ordered array/multipart originals, documented model limits and original alpha-mask HTTP forwarding.
The additional 6x8 source PNG preserves its literal recipe; no existing image recipe changed.
Current hosted execution and final literal review are still required.

At `ae9089a`, all 80 targets and all four new reference/model/multipart cases passed on six stable
platforms and nightly. Strict Clippy, release compilation, actual SDK checks, Node jobs and full
86.42% regions / 85.21% branches coverage passed. Only two reference-fixture formatter hunks failed;
the actual corrections are applied. Notices, archives and extracted checks were skipped.

The next source-only tree has 81 targets and 398 assignments, including partial mappings; 72 remain
unassigned and all dispositions are pending. Five exact HTTP cleanup cases assert prompt errors and
real EOF/reset with deliberately open response bodies, including the combined asset schedule. Both
DNS-hostname/autoSelectFamily cases remain unassigned. Current hosted execution and final literal
acceptance are required. Suite navigation now points to the concrete cases; broader fixtures remain.

At `68ac3f3`, all 81 targets and all five cleanup cases passed on six stable platforms and nightly.
Clippy, release compilation, actual SDK checks, Node jobs and full 86.42% regions / 85.24% branches
coverage passed. Only one asset-fetch formatter hunk failed; its actual correction is applied.
Notices, archives and extracted checks were skipped.

The next source-only tree has 83 targets and 410 assignments, including partial mappings; 60 remain
unassigned and all dispositions are pending. Ten exact review regressions cover eight refusal cases,
two-Store concurrent finalization with original requirements/bytes and excessive-item rejection. Two
local-recovery cases preserve pre-aborted inline completion, same-Store connectionless recovery and
the exact 100-byte reservation/release schedule. Their hosted execution and final literal acceptance
are pending. A fixture's discarded stamped receipt was corrected without changing production fences.

At `d98405b`, all 83 targets and all twelve new review/local-recovery cases passed on six stable
platforms and nightly. Clippy, release compilation, SDK checks, Node jobs and full 86.47% regions /
85.30% branches coverage passed. Only six formatter hunks in the two new fixtures failed; their actual
corrections are applied. Notices, archives and extracted checks were skipped.

The next source-only tree has 85 targets and 425 assignments, including partial mappings; 45 remain
unassigned and all dispositions stay pending. Twelve exact transport cases cover terminal/uncertain
SSE and both adapters' other/missing/owned job-ID waiter/refresh matrix at actual Application dispatch.
SDK structuredContent, protocol envelopes and handshake assertions remain explicit gaps. Three exact
recovery cases cover same-Store published-file reconciliation, changed stable-target refusal and
read-only receipt verification from a live-WAL migration backup. Other filesystem fault variants remain
open. Runtime teardown precedes workspace cleanup on normal and failure paths. Current hosted
compilation/execution and final independent literal acceptance are still required.

At `8a0c926`, all 85 targets and all fifteen transport/publication cases passed on six stable
platforms and nightly. Clippy, release compilation, SDK checks, Node jobs and full 86.50% regions /
85.36% branches coverage passed. Only eight formatter hunks failed; actual corrections are applied.
Notices, archives and extracted checks were skipped.

The next source-only tree has 87 targets and 437 assignments, including partial mappings; 33 remain
unassigned and all dispositions stay pending. Four terminal-uncertainty/cancellation cases preserve
actual public dispatch, capacity, two reopens and zero generation POSTs; SDK envelope/handshake gaps
remain. Eight named killed-child cases preserve exact defaults, receipt/phase state, public reads,
original/index/pending/file counts and second-reopen deduplication. Targeted SQL expiry substitutes
for the clock spy, and a restored SQL trigger represents the published commit fault; those differences
remain partial. Reaping and reader cleanup are bounded on failure too, with incomplete cleanup reported
to the outer CI owner. Current hosted execution and final literal acceptance are still required.

At `ff4a47c`, all 87 targets and twelve new terminal/crash cases passed on six stable platforms and
nightly; Clippy, Node jobs and full 86.51% regions / 85.33% branches coverage passed. Five release/SDK
paths passed. Intel macOS release compilation reached the 360-second idle bound without a compiler
diagnostic; SDK missing-executable failures followed. Five formatter hunks failed and their exact
hosted corrections are applied; notices/archives/extracted checks were skipped. Real Cargo artifact
progress is enabled for release compilation without increasing bounds or claiming a stall repair.

The next source-only tree has 88 targets and 441 assignments, including partial mappings; 29 remain
unassigned and all dispositions stay pending. Three actual-SDK terminal matrices preserve original
results, admission/replay/cancellation and two-host/two-restart behavior. Startup run IDs substitute
for across-restart PID nonreuse, an explicit fidelity gap. One exact Comfy original-waiter/refresher
race preserves held-history synchronization, completed state, output/index counts and one POST.
Applicable hosted/extracted execution and final independent literal acceptance remain required.

At `f8fc7e1`, all 88 targets, the exact Comfy waiter and three SDK terminal matrices passed on
six stable platforms and nightly. Clippy, release compilation, stdio/HTTP SDK and Node jobs passed;
full coverage was 86.59% regions / 85.52% branches. Intel release passed unchanged bounds; the prior
stall's cause remains unproven. Only two Comfy formatter hunks failed and their exact corrections
are applied. Notices/archives/extracted checks were skipped.

The next source-only tree retains 88 targets with 448 assignments, including partial mappings;
22 remain unassigned and all dispositions remain pending. Seven actual two-host SDK races preserve
held-response synchronization, exclusive writer ownership, cancellation/completion precedence,
two restarts, originals/output identities and exact request counts. Shared ownership observes late
async provider errors after cleanup; a persistent two-case regression guards that failure path.
Hosted/extracted execution and final independent literal acceptance remain required.

At `f09561c`, all 88 targets passed on six stable platforms and nightly. All six stable jobs also
passed fmt/Clippy/release compilation, SDK stdio/HTTP, notices, archives/checksums and extracted SDK,
including all seven new race cases. Node jobs passed. Nightly nevertheless failed the earlier terminal
admission probe: zero POSTs instead of one. Its coverage floors passed at 86.55% regions / 85.39%
branches, but the failed SDK gate remains failed and the seven new races were not reached there.
The cause is unknown; unchanged assertions now carry sanitized adapter/mode/round/error diagnostics.
No retry, timeout increase or production repair is claimed.

The next source-only tree has 89 targets and 450 assignments, including partial mappings; 20 remain
unassigned and all 470 dispositions stay pending. Two exact Linux/macOS permission/Unicode-config and
escape/safe-symlink cases require hosted execution and final literal review. A separate Windows DPAPI
script/native roundtrip draft is not integrated because abort-path descendant cleanup is unverified.

At `fbe8a3c`, all 89 targets passed on six stable platforms and nightly; both POSIX cases passed on
Linux/macOS with the original Windows exclusion. Clippy, release compilation, SDK terminal/race/HTTP
checks and Node jobs passed. The earlier nightly admission failure did not reproduce at unchanged
bounds; its cause remains unknown. Full coverage passed at 86.61% regions / 85.58% branches. Only
three POSIX formatter hunks failed; their exact hosted corrections are applied without changing
assertions. Notices, archives and extracted checks were skipped; no current packaged acceptance is claimed.

At `01eb340`, all ten exact-commit jobs passed: all 89 native targets on six stable platforms and
nightly, strict quality checks, actual SDK stdio/HTTP, notices/licenses, archives/checksums and extracted
SDK execution on every stable target. Full coverage passed at 86.61% regions / 85.55% branches.
These unsigned internal artifacts are not a signed Release or complete baseline-case acceptance.

The next source-only tree has 90 targets and 453 assignments, including partial mappings; 17 remain
unassigned and all 470 dispositions stay pending. Three isolated actual-native registration cases
preserve backup/rollback/removal, exact CRLF TOML/refusals, concurrent writes/foreign locks and invalid
JSON. Native executable/bridge argv and PID-bearing log filenames remain explicit source-format gaps.
The exact requirements case now uses the frozen 8x6 alpha PNG, opaque roundtrip, Flash 4:3/4K mismatch
and all 56 Flash / 30 Pro pure metadata pairs with unchanged originals; the broad 7x5 oracle remains
separate. These new assertions still need hosted execution and final independent literal acceptance.

At `c948fb3`, all 90 targets passed on four stable POSIX platforms and nightly. The exact requirements
case passed all six; Windows registration alone exposed mixed fixture separators versus native
`current_exe` text. Reconstructing fixture-root components preserves exact command and redaction
assertions. All six Clippy/release/SDK paths and Node jobs passed; coverage passed at 86.63% regions /
85.64% branches. Seven actual formatter hunks are applied; notices/archives/extracted checks were
skipped. The repaired tree still needs exact-SHA verification.

At `d58b316`, all ten exact-commit jobs passed: all 90 targets on six stable platforms and nightly,
strict fmt/Clippy/release compilation, SDK stdio/HTTP, notices/licenses, archives/checksums and extracted
SDK execution on all six targets. Coverage passed at 86.64% regions / 85.67% branches. Windows exact
registration command equality and redaction passed after fixture-root normalization; no assertion was
weakened. These unsigned internal artifacts are not a signed Release or full 470-case acceptance.

The next source-only tree has 91 targets and 456 assignments, including partial mappings; 14 remain
unassigned and all 470 dispositions stay pending. Three exact clock cases use the original schedules
for renewal/peer takeover, expired prepared admission and stale same-instance publication. Per-Store
lease time defaults to UTC without changing receipt timestamps, migration deadlines, SQL/fences or
renewal scheduling; tests use real isolated SQLite and original 8x6 alpha PNG bytes, not sleeps or SQL
lease edits. Rust 1.99.1 has all six official target/component sets available; new cases and the compiler
patch await their own hosted verification. Environment/DNS/fault/DPAPI gaps remain open.

At `5164efc`, all three clock cases passed all six targets and nightly. Rust 1.99.1 setup, Clippy,
release compilation, SDK stdio/HTTP and Node jobs passed; coverage passed at 86.68% regions / 85.67%
branches. Only two clock formatting hunks and one Linux x86_64 Responses waiter three-second timeout
failed. The timeout cause is unknown; fixed-label stage/counter diagnostics retain the deadline and
assertions, with owned Runtime destruction before workspace cleanup. Actual hosted formatting is
applied. The corrected tree needs its own verification; notices/archives/extracted checks were skipped.

At `a7273cf`, all ten jobs passed: all 91 targets on six stable platforms and nightly, Rust 1.99.1
quality/release/SDK checks, all-six notices/licenses/archives/checksums/extracted SDK and Node jobs.
Complete coverage passed at 86.65% regions / 85.61% branches. The Responses waiter passed unchanged
three-second bounds; the earlier timeout's root cause remains unknown, not claimed repaired.

At `8b52164`, all 92 targets and the exact two environment/two state cases passed on six stable
platforms and nightly; all-six Clippy and Node jobs passed. Only two formatter hunks are applied.
Five release/SDK paths passed. Intel macOS release reached its 900-second wall ceiling; missing-binary
SDK failures are downstream. Nightly SDK's Responses completed/round0 admission probe observed zero
POSTs and `request_timeout/network` at unchanged 700 ms. Neither cause is established. Sanitized
available-row stage/duration diagnostics retain assertions, order and bounds; missing rows are not
proof a stage was unreached. Coverage measured 86.75% regions / 85.64% branches from a failed run;
archives were skipped. The corrected tree still needs its own hosted proof.

At `b83d872`, all 92 targets and four Environment cases passed six stable platforms and nightly.
All-six fmt/Clippy/release/SDK stdio/HTTP/notices/licenses/archives/checksums/extracted SDK and Node jobs
passed. Intel release took 674.532 seconds at unchanged bounds; its earlier timeout cause is unproven.
Only nightly SDK failed: Interactions/failed/round0, zero observed POSTs, `request_timeout/network`,
1909 ms elapsed and available started/intent/completed rows. Failed-run coverage was 86.71% regions /
85.64% branches, not passing acceptance. The current preparation observer measures synchronous
client-build and pre-send time before logging; no TLS, deadline, retry or ordering change is claimed.

At `dc8f7ec`, all ten jobs passed: all 93 targets on six stable platforms and nightly, strict
fmt/Clippy/release/SDK stdio/HTTP/notices/licenses/archives/checksums/extracted SDK and three Node jobs.
Nightly coverage passed at 86.80% regions / 85.73% branches. The unchanged 700 ms admission probe passed;
its earlier timeout cause is unproven. This is actual six-target internal distribution evidence,
not a signed Release or full baseline acceptance.

At `93cc4dd`, all 94 targets and the five exact write/sync/link/unlink/receipt-update cases passed
six stable platforms and nightly. Default real I/O, OS errors and existing SQL use the same
instance-owned port; see [persistence fault coverage](PERSISTENCE-FAULTS.md). Six stable jobs failed
only formatting in two files; nightly SDK failed Interactions/failed/round0 with zero POST handlers,
`request_timeout/network`, 1574 ms elapsed and 35 ms client/pre-send preparation at the unchanged
700 ms deadline. The remaining delay is unmeasured. Relative wall-clock stage projection adds only
numeric diagnostics, not a production repair, deadline increase or proof of network absence.
Failed-run coverage was 86.80% regions / 85.64% branches, not passing acceptance; archives were skipped.
At `3bc2789`, all ten jobs passed: all 94 targets and five persistence cases on six stable platforms
and nightly, all-six strict quality/release/SDK/notices/licenses/archives/checksums/extracted SDK and
three Node jobs. Complete coverage passed at 86.86% regions / 85.73% branches. The admission probe
passed at its unchanged 700 ms deadline; diagnostics do not establish the earlier timeout's cause.
At `12f2b36`, all 95 targets passed on four stable POSIX targets and nightly. Both Windows jobs passed
94 other targets and the actual descendant-abort regression, but the main DPAPI case returned exit1
before native decrypt. All-six Clippy/release/SDK stdio/HTTP and Node jobs passed. DPAPI formatting and
a nightly SDK Interactions admission timeout kept CI red; archives were skipped. Failed-run coverage
was 86.83% regions / 85.67% branches, not passing acceptance. Available stages placed intent at
1173 ms, transport preparation at 1212 ms and completion at 1332 ms; this is not a root-cause trace.
The fixture omitted `PATHEXT`; an isolated selected-environment probe reproduced failed `icacls`
lookup and successful lookup when that one existing variable was forwarded. At `b23f6e2`, all 96 targets
passed on five stable platforms and nightly. Shared polling's three regressions and source-original
first-poll/two-GET/pre-cancel case passed on all seven; its selected error factory remains distinct
from arbitrary JavaScript cancellation-reason transport. Windows x64 passed full DPAPI/ACL/native
round-trip/corrupt/no-overwrite checks. ARM passed protection but failed before ACL verification;
both descendant-abort cases passed. All-six Clippy/release/SDK stdio/HTTP and nightly SDK passed;
complete coverage was 86.90% regions / 85.63% branches. The unchanged 700 ms probe passed without
establishing its intermittent timeout's cause. Two formatting files and ARM's main case kept CI red;
archives were skipped. At `39d1cbb`, nine jobs passed: all-six formatting/Clippy/release/SDK, five
packaged targets and nightly 86.90% / 85.63% coverage. Only ARM DPAPI failed; fixed diagnostics showed
stage4/protected/allow/foreign all true. Disabling inheritance and replacing the current user's grant
did not remove other explicit grants. The script now supplies a protected current-SID-only DACL at
exclusive creation; an owned synthetic reproduction passed, but hosted native confirmation remains.
At `a759e27`, eight jobs/four POSIX packaged targets passed, with 86.93% / 85.67% coverage. Both Windows
main cases failed after changing the populated fixture parent's ACL denied child access; abort cases passed.
An owned reproduction confirms this separate fixture defect. Only a fresh private destination directory
now receives the noninheritable descriptor; original scripts/captures stay untouched. x64 inventory also
hit its unchanged idle bound; real collection-boundary events add diagnosis, not a proven root fix.
At `4f471d2`, all ten jobs passed: all 96 targets on six stable platforms and nightly, all-six
fmt/Clippy/release/SDK/notices/archives/checksums/extracted SDK and 86.93% regions / 85.67% branches.
Both Windows strict DPAPI/native Unicode/corrupt/no-overwrite and owned Job abort cases passed.
x64 inventory passed in 13.015 seconds under unchanged bounds; the earlier idle cause remains unknown.
Default-DACL fallback/foreign token grants remain account-dependent. Original ACL predicates and
bounds remain intact; intent measurements do not alter deadlines, retries, TLS or request ordering.
The new `comfy_same_store_acceptance` target preserves the two literal source schedules alongside
existing reopen regressions: same live Generation refresh, and new Generation with removed connections
but the same Store allocation/owner. It retains ordinary defaults, source receipt fields and exact
8x6 PNG originals; synthetic CUDA metadata is not GPU execution. There are now 97 targets and 467
assignments including partial mappings, three unassigned and all 470 pending. At `5b4ec24`, all 97
and both new cases passed six stable platforms and nightly, including both Windows DPAPI/abort/privacy
cases. All-six Clippy/release/SDK and three Node jobs passed, coverage 86.90% regions / 85.63% branches.
Only two formatting layouts failed, archives skipped; exact hosted formatting is applied. The prior x64
timeout cause, exact hashes/final literal review, DNS and [credential limits](CREDENTIAL-SNAPSHOTS.md) remain.

**All-six packaged execution passed at `4f471d2`; complete baseline-case assertion acceptance has not passed.** Passing
selected JSON integration fixtures is not full accepted-domain or 470-case parity. Old TypeScript
production source remains authoritative until hosted differential verification passes. A file
existing is not a verified mapping. Polling-case isolation and Comfy upload/preflight/recovery
regressions passed at `bc34fbf`; new reference-encoding assertions and cleanup/case-collector repairs
passed at `3c5958f`. The [pending ledger](native-parity.json) preserves all
50 frozen suite hashes and concrete source-authored mappings, with incomplete case equivalence.
Actual expanded collection retains platform-skipped cases; no suite grouping is promoted to 470-case acceptance.
The [case ledger](native-cases/index.json) has all 470 identities and remains entirely pending.
Targeted cases have concrete assertion ranges; explicit gaps include same-instance recovery versus
reopen, direct-wait cancellation reasons and public terminal-error integration. The unmasked-upload
and Omni multipart assertions passed all six stable targets and nightly at `98a5db8`;
final literal acceptance remains open. Structural validation
and full assertion acceptance are separate gates; setting a completion flag alone cannot pass.

| Boundary                                              | Native implementation                                                 | Authored verification                                             | Status                                                                                                                    |
| ----------------------------------------------------- | --------------------------------------------------------------------- | ----------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| Config defaults/full/compact/dotenv/UTF-16/precedence | `native/config/`                                                      | `tests/rust/config_parity.rs`, pinned-source oracle               | Native source-oracle and raw JSON fixtures passed at 8271b81; full case assignment pending                                |
| Credentials/env/private files/current-user DPAPI      | `native/config/credentials.rs`, `dpapi.rs`                            | Config fixture tests; same-user DPAPI oracle generated on Windows | Pending CI; no real credentials transferred                                                                               |
| Hot reload/64 snapshots/last good config              | `native/config/reload.rs`, `native/mcp/application.rs`                | Config reload cases                                               | Retained-route/ambiguity/local recovery passed at 8271b81; exact concurrent/source schedules pending                      |
| Stable request/account identity                       | `native/compat/`, `native/services/identity.rs`                       | Canonical source oracle                                           | Pending ICU/JavaScript identity parity                                                                                    |
| Durable SQLite v2/WAL/FULL/migration                  | `native/jobs/`                                                        | State/lifecycle/recovery cases                                    | Migration and eight real killed-child checkpoints passed at 8271b81; complete fault/reopen matrix pending                 |
| Admission/fences/terminal/reservations                | `native/jobs/lifecycle.rs`, `journal.rs`                              | Lifecycle/recovery cases                                          | Pending multi-host and process-crash coverage                                                                             |
| HTTP/redirect/DNS/assets/SSE/no retries               | `native/http/`                                                        | HTTP cases and generation loopback                                | Pending all bounded network/resource cases                                                                                |
| PNG/JPEG/WebP/original bytes/masks/previews           | `native/files/`                                                       | Real baseline-generated image metadata                            | Pending malformed/animated/container/path race parity                                                                     |
| Images/Gemini/Interactions/Responses/chat/OpenRouter  | `native/adapters/`                                                    | Source-generated wire and normalization cases                     | Pending native multipart capture and terminal SSE cases                                                                   |
| Comfy graph/references/node descriptors/poll/cancel   | `native/adapters/workflow.rs`, `comfy.rs`                             | Source coverage still being mapped                                | Pending complete native contracts                                                                                         |
| Dimensions/capability evidence/model policy           | `native/capabilities.rs`, `dimensions.rs`, `services/model_policy.rs` | Policy source oracle                                              | Pending live-free descriptor contracts                                                                                    |
| Generation/dedup/finalization/local recovery          | `native/services/`                                                    | Tiny loopback, original hash, recovery fixtures                   | Pending exhaustive original lifecycle contracts                                                                           |
| Ten tools/stdio/resources/negotiation/bridge          | `native/mcp/`, `native/main.rs`                                       | Real child `bridge` fixture, foreign CWD and EOF                  | Bridge/two-host/two-restart/legacy-auto-pin and HTTP outputs/resources passed at 8271b81; active race case ledger pending |
| Maintenance/readiness/client registration             | Native CLI replacement                                                | Existing maintenance cases not yet migrated                       | Pending; do not retire working scripts yet                                                                                |
| Native archives/hash/extraction/licenses/docs         | `scripts/native-package.py`                                           | Actual allowlist/checksum/notice/extracted SDK checks             | Passed all six native targets at544baa9; unsigned test artifacts, no verified signed Release                              |
| Coverage                                              | Hosted nightly only                                                   | LLVM regions >=85%, branches >=80%                                | Complete544baa9 report:86.25% regions /84.61% branches; both unchanged floors passed                                      |

## Release blockers

Every applicable baseline case needs a concrete native case or a source-driven differential replay
through the native public boundary. Suite-level grouping above is a navigation index, **not all 470
cases accounted for**. Case inventory and acceptance must fail if a case is unassigned, pending,
unexecuted or relies solely on the old Node implementation. No blanket "covered" or "retired" state.

Ordinary users will need no Node, Rust or Python runtime. Node is retained only in hosted pinned-source
oracle checks; Python helpers are CI tooling, never dependencies of the shipped binary. Release and
local old-runtime cleanup wait for their separately specified verification and owner cutover gates.
