# Testing

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
