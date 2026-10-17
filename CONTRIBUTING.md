# Contributing

The native target uses Rust1.99 stable/edition2024, a real committed Cargo lockfile once resolved in
hosted CI, and responsibility-based modules below800 lines. Adapters stay pure; tools remain
provider-agnostic. Update related fixtures/docs with behavior changes. Never add paid POST retries,
provider fallback, prompt rewrites, hidden public uploads or unbounded queues/polling.

Rust compilation/builds/tests for this migration run only in hosted CI. The pipeline runs strict
rustfmt/Clippy, native public-boundary fixtures, pinned-source oracles, binary packaging and a separate
nightly coverage job. Production binaries remain stable; >=85% executable regions and >=80% branches
are mandatory and cannot be bypassed by exclusions/skips. Source-only checks are not runtime evidence.

All generated toolchain/dependency/test/cache/temp/target artifacts stay in the owning project/workspace.
Keep tiny deterministic real-source fixtures, bounded network/child lifetimes, two workers and finally
cleanup. No real keys/private files in tracked fixtures. Original returned image bytes/hash/metadata
must be inspected; a mocked success does not prove a wire contract or host GUI behavior.

The existing Node implementation remains during compatibility verification, never as an undeclared
runtime fallback. Complete all source contracts before retiring it. Preserve existing private config,
outputs, receipts/journals, snapshots and history. Local old-runtime cleanup waits owner host cutover
and verified native reconnection. Registration updates only an explicitly selected host entry with
backups; no automatic host settings mutation or runtime/model installation.

GPL-3.0-only first-party changes and required dependency/reference licenses/notices remain intact.
Publication and Release require owner authorization and exact final hosted verification; an uncompiled
migration candidate is not a ready Release.
