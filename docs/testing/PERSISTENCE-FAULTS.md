# Persistence fault acceptance

`tests/rust/persistence_fault_acceptance.rs` covers the five frozen recovery cases for one-shot
write, sync, link, unlink and receipt-update failure. All five passed on six stable targets and
nightly at `93cc4dd`; its formatting and SDK failures blocked that run. The corrected `3bc2789`
batch passed all ten jobs, including all-six packaged SDK checks. All 470 dispositions remain pending.

## Shared production path

`Store::open` and `Store::open_with_lease_clock` use `NativePersistence`. Its defaults call the
original `write_all`, `sync_all`, `hard_link` and `remove_file` operations on real files and paths.
The existing exclusive temporary file, permissions, generation fences, directory synchronization,
physical-path/hash verification, journal and transactional output commit remain unchanged.

`Store::open_with_persistence` accepts an instance-owned implementation for the same production
path. It is not a configuration option, environment switch or process-global failpoint. The
receipt-update entry check defaults to success; actual SQLite errors retain their existing
sanitized storage error. No database schema or dependency changes are required.

## Exact failure schedule

Each case uses the source configuration, receipt fields, parsed `draw` request and original
8 × 6 RGBA PNG generated from the frozen source recipe. The same Store and Generation perform
initial finalization and public `get_job` recovery.

1. Arm one selected operation after setup; all other operations delegate to real native I/O.
2. Observe zero outputs after a file-operation failure, or the exact synthetic
   `Controlled receipt SQL failure` rejection for the source's whole-method receipt-update mock.
3. Restore the fault, require one retained result, then call public recovery with refresh enabled.
4. Require completed status, one output with identical original bytes, one output-index record
   and no pending result. Verify the selected fault fired exactly once.

The unlink case reaches the real stable-target publication boundary before recovery; it does not
substitute a permissions error or manually write the target. The receipt case mirrors the source
method mock, not a promise to expose raw SQLite diagnostics.

## Bounds and evidence limits

Each fixture owns a current-thread Tokio runtime and isolated workspace. Finish and recovery each
have a three-second cooperative bound; synchronous filesystem calls are still bounded externally
by the hosted test command. No fixture child, provider request, global fault state, blind sleep or
OS-environment mutation is introduced. Runtime and lease captures are dropped before workspace
cleanup, including unwind paths.

Cargo registers this as the 94th integration target. Hosted stable checks run on Windows, Linux
and macOS for x86_64 and ARM64; the separate nightly job retains the 85% regions / 80% branches
coverage floors. Source review and ledger assignment are not runtime or complete parity evidence.
These fixtures do not simulate power loss, guarantee every filesystem's durability or establish
signed Release readiness. See the [migration matrix](MIGRATION-MATRIX.md) for remaining acceptance.
